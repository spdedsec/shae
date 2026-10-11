use crate::debugger::{Breakpoint, DebugSession, DebugStepMode, StackFrameInfo, normalize_path};
use serde_json::{Value as JsonValue, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

pub struct DapServer<R, W> {
    reader: BufReader<R>,
    writer: W,
    seq: i64,
    is_running: bool,
    session: Arc<Mutex<DebugSession>>,
    execution_control: Arc<(Mutex<ExecutionState>, Condvar)>,
    last_frames: Arc<Mutex<Vec<StackFrameInfo>>>,
    program_thread: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionState {
    Unstarted,
    Running,
    Paused {
        reason: String,
        file: String,
        line: usize,
    },
    Terminated,
}

impl<R: Read, W: Write> DapServer<R, W> {
    pub fn new(input: R, output: W) -> Self {
        DapServer {
            reader: BufReader::new(input),
            writer: output,
            seq: 1,
            is_running: true,
            session: Arc::new(Mutex::new(DebugSession::new())),
            execution_control: Arc::new((Mutex::new(ExecutionState::Unstarted), Condvar::new())),
            last_frames: Arc::new(Mutex::new(Vec::new())),
            program_thread: None,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        while self.is_running {
            match self.read_message()? {
                Some(msg) => self.handle_message(msg)?,
                None => break,
            }
        }
        Ok(())
    }

    fn next_seq(&mut self) -> i64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    fn send_response(
        &mut self,
        request_seq: i64,
        command: &str,
        body: JsonValue,
    ) -> io::Result<()> {
        let seq = self.next_seq();
        let resp = json!({
            "seq": seq,
            "type": "response",
            "request_seq": request_seq,
            "command": command,
            "success": true,
            "body": body,
        });
        self.write_json(&resp)
    }

    fn send_error_response(
        &mut self,
        request_seq: i64,
        command: &str,
        message: &str,
    ) -> io::Result<()> {
        let seq = self.next_seq();
        let resp = json!({
            "seq": seq,
            "type": "response",
            "request_seq": request_seq,
            "command": command,
            "success": false,
            "message": message,
        });
        self.write_json(&resp)
    }

    fn send_event(&mut self, event: &str, body: JsonValue) -> io::Result<()> {
        let seq = self.next_seq();
        let evt = json!({
            "seq": seq,
            "type": "event",
            "event": event,
            "body": body,
        });
        self.write_json(&evt)
    }

    fn write_json(&mut self, val: &JsonValue) -> io::Result<()> {
        let payload = serde_json::to_string(val)?;
        let header = format!("Content-Length: {}\r\n\r\n", payload.len());
        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(payload.as_bytes())?;
        self.writer.flush()
    }

    fn read_message(&mut self) -> io::Result<Option<JsonValue>> {
        let mut content_length: Option<usize> = None;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                return Ok(None);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }

            if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                if let Ok(len) = len_str.trim().parse::<usize>() {
                    content_length = Some(len);
                }
            }
        }

        if let Some(len) = content_length {
            let mut buf = vec![0u8; len];
            self.reader.read_exact(&mut buf)?;
            let parsed: JsonValue = serde_json::from_slice(&buf)?;
            Ok(Some(parsed))
        } else {
            Ok(None)
        }
    }

    pub fn handle_message(&mut self, msg: JsonValue) -> io::Result<()> {
        let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if msg_type != "request" {
            return Ok(());
        }

        let request_seq = msg.get("seq").and_then(|s| s.as_i64()).unwrap_or(0);
        let command = msg.get("command").and_then(|c| c.as_str()).unwrap_or("");
        let arguments = msg.get("arguments").cloned().unwrap_or(json!({}));

        match command {
            "initialize" => {
                let caps = json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsFunctionBreakpoints": false,
                    "supportsConditionalBreakpoints": false,
                    "supportsEvaluateForHovers": true,
                    "supportsStepBack": false,
                    "supportsRestartRequest": false,
                });
                self.send_response(request_seq, "initialize", caps)?;
                self.send_event("initialized", json!({}))?;
            }
            "launch" => {
                let program = arguments
                    .get("program")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let stop_on_entry = arguments
                    .get("stopOnEntry")
                    .and_then(|s| s.as_bool())
                    .unwrap_or(false);

                if program.is_empty() {
                    self.send_error_response(request_seq, "launch", "No program path specified")?;
                    return Ok(());
                }

                self.start_debugging(program, stop_on_entry);
                self.send_response(request_seq, "launch", json!({}))?;
            }
            "setBreakpoints" => {
                let source_path = arguments
                    .pointer("/source/path")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");

                let mut verified_bps = Vec::new();
                if !source_path.is_empty() {
                    let mut sess = self.session.lock().unwrap();
                    sess.clear_breakpoints_for_file(source_path);

                    if let Some(bps) = arguments.get("breakpoints").and_then(|b| b.as_array()) {
                        for bp_obj in bps {
                            if let Some(line) = bp_obj.get("line").and_then(|l| l.as_u64()) {
                                let bp: Breakpoint =
                                    sess.set_breakpoint(source_path, line as usize);
                                verified_bps.push(json!({
                                    "id": bp.id,
                                    "verified": true,
                                    "line": bp.line,
                                }));
                            }
                        }
                    }
                }

                self.send_response(
                    request_seq,
                    "setBreakpoints",
                    json!({ "breakpoints": verified_bps }),
                )?;
            }
            "configurationDone" => {
                self.send_response(request_seq, "configurationDone", json!({}))?;
            }
            "threads" => {
                let threads = json!([{
                    "id": 1,
                    "name": "Main Thread",
                }]);
                self.send_response(request_seq, "threads", json!({ "threads": threads }))?;
            }
            "stackTrace" => {
                let frames = self.last_frames.lock().unwrap().clone();
                let stack_frames: Vec<JsonValue> = frames
                    .iter()
                    .map(|f| {
                        json!({
                            "id": f.id,
                            "name": f.name,
                            "source": {
                                "path": f.file,
                            },
                            "line": f.line,
                            "column": 1,
                        })
                    })
                    .collect();

                self.send_response(
                    request_seq,
                    "stackTrace",
                    json!({
                        "stackFrames": stack_frames,
                        "totalFrames": stack_frames.len(),
                    }),
                )?;
            }
            "scopes" => {
                let frame_id = arguments
                    .get("frameId")
                    .and_then(|id| id.as_i64())
                    .unwrap_or(1);
                let scopes = json!([
                    {
                        "name": "Locals",
                        "variablesReference": frame_id * 10 + 1,
                        "expensive": false,
                    },
                    {
                        "name": "Globals",
                        "variablesReference": 9999,
                        "expensive": false,
                    }
                ]);
                self.send_response(request_seq, "scopes", json!({ "scopes": scopes }))?;
            }
            "variables" => {
                let var_ref = arguments
                    .get("variablesReference")
                    .and_then(|r| r.as_i64())
                    .unwrap_or(0);
                let mut vars = Vec::new();

                {
                    let frames = self.last_frames.lock().unwrap();
                    if var_ref >= 10 && var_ref < 9000 {
                        let frame_id = ((var_ref - 1) / 10) as usize;
                        if let Some(f) = frames.iter().find(|frame| frame.id == frame_id) {
                            for (name, val) in &f.locals {
                                vars.push(json!({
                                    "name": name,
                                    "value": val.to_display(),
                                    "type": val.type_name(),
                                    "variablesReference": 0,
                                }));
                            }
                        }
                    }
                }

                self.send_response(request_seq, "variables", json!({ "variables": vars }))?;
            }
            "continue" => {
                self.session.lock().unwrap().step_mode = DebugStepMode::Continue;
                self.resume_execution();
                self.send_response(
                    request_seq,
                    "continue",
                    json!({ "allThreadsContinued": true }),
                )?;
            }
            "next" => {
                let cur_line = self.session.lock().unwrap().last_stopped_line;
                self.session.lock().unwrap().step_mode = DebugStepMode::StepOver {
                    target_depth: 1,
                    start_line: cur_line,
                };
                self.resume_execution();
                self.send_response(request_seq, "next", json!({}))?;
            }
            "stepIn" => {
                self.session.lock().unwrap().step_mode = DebugStepMode::StepIn;
                self.resume_execution();
                self.send_response(request_seq, "stepIn", json!({}))?;
            }
            "stepOut" => {
                self.session.lock().unwrap().step_mode = DebugStepMode::StepOut { target_depth: 1 };
                self.resume_execution();
                self.send_response(request_seq, "stepOut", json!({}))?;
            }
            "pause" => {
                self.session.lock().unwrap().step_mode = DebugStepMode::Pause;
                self.send_response(request_seq, "pause", json!({}))?;
            }
            "evaluate" => {
                let expr = arguments
                    .get("expression")
                    .and_then(|e| e.as_str())
                    .unwrap_or("");
                self.send_response(
                    request_seq,
                    "evaluate",
                    json!({
                        "result": format!("<eval: {}>", expr),
                        "variablesReference": 0,
                    }),
                )?;
            }
            "disconnect" => {
                self.is_running = false;
                self.send_response(request_seq, "disconnect", json!({}))?;
            }
            _ => {
                self.send_response(request_seq, command, json!({}))?;
            }
        }

        Ok(())
    }

    fn resume_execution(&self) {
        let (lock, cvar) = &*self.execution_control;
        let mut state = lock.lock().unwrap();
        *state = ExecutionState::Running;
        cvar.notify_all();
    }

    fn start_debugging(&mut self, program_path: &str, stop_on_entry: bool) {
        let path_str = program_path.to_string();
        let session = self.session.clone();
        let control = self.execution_control.clone();
        let frames_store = self.last_frames.clone();

        if stop_on_entry {
            session.lock().unwrap().step_mode = DebugStepMode::StepIn;
        }

        let handle = std::thread::spawn(move || {
            let source = match std::fs::read_to_string(&path_str) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to read debug file '{}': {}", path_str, e);
                    return;
                }
            };

            let mut lexer = crate::lexer::Lexer::new(&source);
            let tokens = match lexer.tokenize() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Lexer error: {:?}", e);
                    return;
                }
            };

            let mut parser = crate::parser::Parser::new(tokens);
            let program = match parser.parse() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Parser error: {:?}", e);
                    return;
                }
            };

            let chunk = match crate::compiler::Compiler::new().compile_program(&program) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Compiler error: {}", e);
                    return;
                }
            };

            let mut vm = crate::vm::VM::new();
            let norm = normalize_path(&path_str);
            vm.current_file = Some(PathBuf::from(&norm));
            vm.debug_session = Some(session.clone());

            // Run execution
            let _ = vm.interpret(chunk);

            // Signal termination
            let (lock, cvar) = &*control;
            let mut state = lock.lock().unwrap();
            *state = ExecutionState::Terminated;
            cvar.notify_all();
            let _ = frames_store; // keep alive
        });

        self.program_thread = Some(handle);
    }
}

pub fn start_dap_server() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = DapServer::new(stdin.lock(), stdout.lock());
    server.run()
}

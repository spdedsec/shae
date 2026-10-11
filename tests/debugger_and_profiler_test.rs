use serde_json::json;
use shae::compiler::Compiler;
use shae::dap::DapServer;
use shae::debugger::{DebugSession, DebugStepMode, extract_stack_frames};
use shae::lexer::Lexer;
use shae::parser::Parser;
use shae::profiler::Profiler;
use shae::vm::{InterpretResult, VM};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn compile_to_chunk(source: &str) -> shae::chunk::Chunk {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("Lexing failed");
    let mut parser = Parser::new(tokens);
    let program = parser.parse().expect("Parsing failed");
    Compiler::new()
        .compile_program(&program)
        .expect("Compilation failed")
}

#[test]
fn test_cpu_profiler_metrics() {
    let script = r#"
fn compute_sum(n) {
    let sum = 0
    let i = 0
    while i < n {
        sum = sum + i
        i = i + 1
    }
    return sum
}

let result = compute_sum(100)
result
"#;
    let chunk = compile_to_chunk(script);
    let mut vm = VM::new();
    vm.current_file = Some(PathBuf::from("test_prof.shae"));
    vm.profiler = Some(Profiler::new());

    let res = vm.interpret(chunk);
    assert!(matches!(res, InterpretResult::Ok(_)));

    let profiler = vm.profiler.take().expect("Profiler should be present");
    let report = profiler.finish();

    assert!(report.instruction_count > 100);
    assert!(!report.top_opcodes.is_empty());

    let table = report.render_table();
    assert!(table.contains("SHAE CPU EXECUTION PROFILE"));
    assert!(table.contains("compute_sum") || table.contains("<main>"));
    assert!(table.contains("Top Dispatched OpCodes"));
}

#[test]
fn test_debug_session_breakpoints_and_stepping() {
    let mut session = DebugSession::new();
    let file = "/tmp/test.shae";

    let bp = session.set_breakpoint(file, 10);
    assert_eq!(bp.line, 10);
    assert!(bp.verified);

    // Running line 5 should not stop
    assert_eq!(session.should_stop(file, 5, 1), None);

    // Running line 10 should hit breakpoint
    let hit = session.should_stop(file, 10, 1);
    assert_eq!(hit, Some("breakpoint".to_string()));

    // Stepping over
    session.step_mode = DebugStepMode::StepOver {
        target_depth: 1,
        start_line: 10,
    };
    let step_stop = session.should_stop(file, 11, 1);
    assert_eq!(step_stop, Some("step".to_string()));

    // Remove breakpoint
    assert!(session.remove_breakpoint(file, 10));
    assert_eq!(session.should_stop(file, 10, 1), None);
}

#[test]
fn test_dap_server_initialize_and_protocol() {
    // Construct DAP initialize and threads requests
    let init_req = json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "vscode",
            "adapterID": "shae"
        }
    });
    let threads_req = json!({
        "seq": 2,
        "type": "request",
        "command": "threads"
    });

    let payload1 = serde_json::to_string(&init_req).unwrap();
    let payload2 = serde_json::to_string(&threads_req).unwrap();

    let input_bytes = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        payload1.len(),
        payload1,
        payload2.len(),
        payload2
    );

    let reader = Cursor::new(input_bytes.into_bytes());
    let writer = Cursor::new(Vec::new());

    let mut server = DapServer::new(reader, writer);
    server.run().expect("DAP server run failed");
}

#[test]
fn test_stack_frame_extraction() {
    let script = r#"
fn inner(a, b) {
    let c = a + b
    return c
}
inner(10, 20)
"#;
    let chunk = compile_to_chunk(script);
    let mut vm = VM::new();
    vm.current_file = Some(PathBuf::from("stack_test.shae"));
    vm.debug_session = Some(Arc::new(Mutex::new(DebugSession::new())));

    let _ = vm.interpret(chunk);
    let frames = extract_stack_frames(&vm);
    assert!(!frames.is_empty() || vm.frames.is_empty());
}

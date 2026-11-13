use serde_json::{Value as JsonValue, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};

pub struct LspServer<R, W> {
    reader: BufReader<R>,
    writer: W,
    documents: HashMap<String, String>,
    is_running: bool,
}

impl<R: Read, W: Write> LspServer<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self {
            reader: BufReader::new(input),
            writer: output,
            documents: HashMap::new(),
            is_running: true,
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

    pub fn handle_message(&mut self, msg: JsonValue) -> io::Result<()> {
        let method = msg.get("method").and_then(|m| m.as_str());
        let id = msg.get("id").cloned();

        match method {
            Some("initialize") => {
                let result = json!({
                    "capabilities": {
                        "textDocumentSync": 1, // Full
                        "completionProvider": {
                            "triggerCharacters": [".", ":"]
                        },
                        "hoverProvider": true,
                        "documentFormattingProvider": true
                    },
                    "serverInfo": {
                        "name": "shae-lsp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                });
                self.send_response(id, result)?;
            }
            Some("initialized") => {
                // Notification from client; no response needed
            }
            Some("textDocument/didOpen") => {
                if let Some(params) = msg.get("params") {
                    if let (Some(uri), Some(text)) = (
                        params.pointer("/textDocument/uri").and_then(|u| u.as_str()),
                        params
                            .pointer("/textDocument/text")
                            .and_then(|t| t.as_str()),
                    ) {
                        self.documents.insert(uri.to_string(), text.to_string());
                        self.publish_diagnostics(uri, text)?;
                    }
                }
            }
            Some("textDocument/didChange") => {
                if let Some(params) = msg.get("params") {
                    if let Some(uri) = params.pointer("/textDocument/uri").and_then(|u| u.as_str())
                    {
                        if let Some(changes) =
                            params.pointer("/contentChanges").and_then(|c| c.as_array())
                        {
                            if let Some(last_change) = changes.last() {
                                if let Some(text) = last_change.get("text").and_then(|t| t.as_str())
                                {
                                    self.documents.insert(uri.to_string(), text.to_string());
                                    self.publish_diagnostics(uri, text)?;
                                }
                            }
                        }
                    }
                }
            }
            Some("textDocument/didClose") => {
                if let Some(params) = msg.get("params") {
                    if let Some(uri) = params.pointer("/textDocument/uri").and_then(|u| u.as_str())
                    {
                        self.documents.remove(uri);
                    }
                }
            }
            Some("textDocument/completion") => {
                let completions = self.compute_completions(&msg);
                self.send_response(id, json!(completions))?;
            }
            Some("textDocument/hover") => {
                let hover_info = self.compute_hover(&msg);
                self.send_response(id, json!(hover_info))?;
            }
            Some("textDocument/formatting") => {
                let edits = self.compute_formatting(&msg);
                self.send_response(id, json!(edits))?;
            }
            Some("shutdown") => {
                self.send_response(id, JsonValue::Null)?;
            }
            Some("exit") => {
                self.is_running = false;
            }
            _ => {
                // Unknown method or response
                if id.is_some() {
                    self.send_response(id, JsonValue::Null)?;
                }
            }
        }
        Ok(())
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

        let len = match content_length {
            Some(l) => l,
            None => return Ok(None),
        };

        let mut body = vec![0u8; len];
        self.reader.read_exact(&mut body)?;

        match serde_json::from_slice::<JsonValue>(&body) {
            Ok(json_val) => Ok(Some(json_val)),
            Err(_) => Ok(None),
        }
    }

    fn send_response(&mut self, id: Option<JsonValue>, result: JsonValue) -> io::Result<()> {
        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });
        self.write_json(&response)
    }

    fn write_json(&mut self, val: &JsonValue) -> io::Result<()> {
        let payload = serde_json::to_string(val).unwrap_or_default();
        let header = format!("Content-Length: {}\r\n\r\n", payload.len());
        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(payload.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn publish_diagnostics(&mut self, uri: &str, source: &str) -> io::Result<()> {
        let mut diagnostics = Vec::new();

        match crate::lexer::tokenize(source) {
            Err(e) => {
                let (line, col) = match &e {
                    crate::lexer::LexerError::UnexpectedChar { line, col, .. } => (*line, *col),
                    crate::lexer::LexerError::UnterminatedString { line, col } => (*line, *col),
                    crate::lexer::LexerError::InvalidEscape { line, col, .. } => (*line, *col),
                    _ => (1, 1),
                };
                let l = line.saturating_sub(1);
                let c = col.saturating_sub(1);
                diagnostics.push(json!({
                    "range": {
                        "start": { "line": l, "character": c },
                        "end": { "line": l, "character": c + 1 }
                    },
                    "severity": 1, // Error
                    "source": "shae",
                    "message": e.to_string()
                }));
            }
            Ok(tokens) => {
                let (program, parse_errors) = crate::parser::parse_with_recovery(tokens.clone());
                for pe in &parse_errors {
                    let (line, col) = match pe {
                        crate::parser::ParserError::UnexpectedToken { line, col, .. } => {
                            (*line, *col)
                        }
                        crate::parser::ParserError::Advice { line, col, .. } => (*line, *col),
                        _ => (1, 1),
                    };
                    let l = line.saturating_sub(1);
                    let c = col.saturating_sub(1);
                    diagnostics.push(json!({
                        "range": {
                            "start": { "line": l, "character": c },
                            "end": { "line": l, "character": c + 1 }
                        },
                        "severity": 1,
                        "source": "shae",
                        "message": pe.to_string()
                    }));
                }

                if !program.statements.is_empty() {
                    let mut linter = crate::linter::Linter::new();
                    let lints = linter.lint_program(&program);
                    for diag in lints {
                        let l = diag.span.line.saturating_sub(1);
                        let c = diag.span.col.saturating_sub(1);
                        let sev = match diag.severity {
                            crate::linter::DiagnosticSeverity::Error => 1,
                            crate::linter::DiagnosticSeverity::Warning => 2,
                        };
                        let msg = if let Some(ref h) = diag.hint {
                            format!("{} (Hint: {})", diag.message, h)
                        } else {
                            diag.message.clone()
                        };
                        diagnostics.push(json!({
                            "range": {
                                "start": { "line": l, "character": c },
                                "end": { "line": l, "character": c + 1 }
                            },
                            "severity": sev,
                            "source": "shae-linter",
                            "message": msg
                        }));
                    }

                    let mut typechecker = crate::typecheck::TypeChecker::new();
                    let type_errs = typechecker.check_program(&program);
                    for diag in type_errs {
                        let l = diag.span.line.saturating_sub(1);
                        let c = diag.span.col.saturating_sub(1);
                        let sev = match diag.severity {
                            crate::linter::DiagnosticSeverity::Error => 1,
                            crate::linter::DiagnosticSeverity::Warning => 2,
                        };
                        let msg = if let Some(ref h) = diag.hint {
                            format!("{} (Hint: {})", diag.message, h)
                        } else {
                            diag.message.clone()
                        };
                        diagnostics.push(json!({
                            "range": {
                                "start": { "line": l, "character": c },
                                "end": { "line": l, "character": c + 1 }
                            },
                            "severity": sev,
                            "source": "shae-typechecker",
                            "message": msg
                        }));
                    }
                }
            }
        }

        let notif = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": uri,
                "diagnostics": diagnostics
            }
        });
        self.write_json(&notif)
    }

    fn compute_completions(&self, msg: &JsonValue) -> Vec<JsonValue> {
        let mut items = Vec::new();

        // 1. Language Keywords
        let keywords = [
            ("fn", "fn name(args) { ... }", 14),
            ("let", "let var = ...", 14),
            ("if", "if condition { ... }", 14),
            ("else", "else { ... }", 14),
            ("while", "while condition { ... }", 14),
            ("for", "for item in iterable { ... }", 14),
            ("in", "in", 14),
            ("return", "return value", 14),
            ("struct", "struct Name { ... }", 14),
            ("enum", "enum Name { ... }", 14),
            ("match", "match value { ... }", 14),
            ("use", "use { a, b } from \"path\"", 14),
            ("from", "from \"path\"", 14),
            ("try", "try { ... } catch(e) { ... }", 14),
            ("catch", "catch(err) { ... }", 14),
        ];
        for (kw, detail, kind) in keywords {
            items.push(json!({
                "label": kw,
                "kind": kind,
                "detail": detail
            }));
        }

        // 2. Builtin Functions
        let builtins = [
            ("print", "print(val) - Print value to stdout", 3),
            (
                "println",
                "println(val) - Print value with newline to stdout",
                3,
            ),
            (
                "input",
                "input([prompt]) - Prompt and read line from stdin",
                3,
            ),
            ("read", "read(path) - Read file contents as string", 3),
            (
                "write",
                "write(path, content) - Write string content to file",
                3,
            ),
            ("fetch", "fetch(url) - Make HTTP GET request", 3),
            ("serve", "serve(port, handler) - Start HTTP server", 3),
            (
                "serve_tls",
                "serve_tls(port, handler, cert, key) - Start HTTPS server",
                3,
            ),
            (
                "route_match",
                "route_match(pattern, path) - Match route parameters",
                3,
            ),
            ("spawn", "spawn(fn) - Spawn background task thread", 3),
            ("join", "join(task) - Await and join task result", 3),
            (
                "channel",
                "channel([capacity]) - Create MPMC message channel",
                3,
            ),
            ("send", "send(ch, val) - Send value into channel", 3),
            (
                "recv",
                "recv(ch, [timeout_ms]) - Receive value from channel",
                3,
            ),
            (
                "try_recv",
                "try_recv(ch) - Non-blocking receive from channel",
                3,
            ),
            ("close", "close(ch) - Close channel", 3),
            (
                "assert",
                "assert(condition, [msg]) - Assert condition is truthy",
                3,
            ),
            (
                "assert_eq",
                "assert_eq(actual, expected) - Assert equality",
                3,
            ),
            (
                "jit",
                "jit(action, [target]) - JIT compilation control (status, enable, disable, compile)",
                3,
            ),
        ];
        for (name, detail, kind) in builtins {
            items.push(json!({
                "label": name,
                "kind": kind,
                "detail": detail
            }));
        }

        // 3. Standard Library Modules
        let std_modules = [
            (
                "std:fs",
                "File system operations (read, write, append, mkdir, exists, readDir)",
            ),
            (
                "std:path",
                "File path manipulation (join, dirname, basename, extname, isAbsolute)",
            ),
            (
                "std:sys",
                "System utilities (platform, arch, cwd, env, exit)",
            ),
            ("std:time", "Date and time utilities (now, nowMs, sleep)"),
            (
                "std:crypto",
                "Cryptographic hashes & random bytes (sha256, sha512, hmac, uuid)",
            ),
            ("std:codec", "Encoding & decoding (base64, url, hex)"),
            (
                "std:regex",
                "Regular expressions (is_match, find, find_all, replace, split)",
            ),
            (
                "std:http",
                "HTTP client & requests (get, post, put, delete, request, url_encode)",
            ),
            (
                "std:env",
                "Environment variables & .env loading (get, set, has, remove, all, load_dotenv)",
            ),
            (
                "std:cli",
                "CLI argument parsing and terminal styling (args, style)",
            ),
            (
                "std:collections",
                "High-performance data structures and sets (Set, new, add, remove, contains)",
            ),
            (
                "std:ffi",
                "Foreign Function Interface for dynamic native C libraries (.so, .dylib, .dll)",
            ),
            (
                "std:csv",
                "RFC 4180 CSV parser and serializer (parse, stringify)",
            ),
            (
                "std:sqlite",
                "Embedded SQLite database engine (open, execute, query, close)",
            ),
            (
                "std:gui",
                "Native 2D windowing, shape drawing (rect, circle, line), font rendering, input polling, and 60 FPS control",
            ),
            (
                "std:random",
                "Fast seedable pseudo-random number generator (int, float, bool, choice, shuffle, seed)",
            ),
            (
                "std:math",
                "Mathematical functions and constants (sqrt, abs, min, max, pow, sin, cos, tan, atan2, clamp, PI, E)",
            ),
            (
                "std:io",
                "Standard I/O stream operations (print, println, input, flush)",
            ),
            (
                "std:actor",
                "Isolated actors with FIFO mailboxes, request-response (ask), and fault-tolerant supervision trees",
            ),
            (
                "std:actor:cluster",
                "Distributed actor clustering with TLS / mutual-TLS encrypted transport",
            ),
            (
                "std:async",
                "Asynchronous concurrency combinators and coroutines (spawn, all, race, sleep, timeout)",
            ),
            (
                "std:unicode",
                "UAX #29 Unicode grapheme clustering and rune handling (graphemes, grapheme_len, slice_graphemes)",
            ),
            (
                "std:test",
                "Pure-Shae assertion and test runner framework (test, assert, assert_eq, assert_ne, assert_throws)",
            ),
            (
                "std:buffer",
                "Zero-copy typed binary buffers and struct memory packing (alloc, pack, unpack, slice)",
            ),
        ];
        for (mod_name, doc) in std_modules {
            items.push(json!({
                "label": mod_name,
                "kind": 9, // Module
                "detail": doc
            }));
        }

        // 4. In-document symbols (functions, variables)
        if let Some(uri) = msg
            .pointer("/params/textDocument/uri")
            .and_then(|u| u.as_str())
        {
            if let Some(source) = self.documents.get(uri) {
                if let Ok(tokens) = crate::lexer::tokenize(source) {
                    let (program, _) = crate::parser::parse_with_recovery(tokens);
                    for stmt in &program.statements {
                        match &stmt.kind {
                            crate::ast::StmtKind::FnDef {
                                name,
                                params,
                                return_type,
                                ..
                            } => {
                                let p_str = params
                                    .iter()
                                    .map(|p| {
                                        if let Some(ann) = &p.type_ann {
                                            format!("{}: {}", p.name, ann)
                                        } else {
                                            p.name.clone()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let ret_str = if let Some(ret) = return_type {
                                    format!(" -> {}", ret)
                                } else {
                                    String::new()
                                };
                                items.push(json!({
                                    "label": name,
                                    "kind": 3, // Function
                                    "detail": format!("fn {}({}){}", name, p_str, ret_str)
                                }));
                            }
                            crate::ast::StmtKind::StructDef { name, fields } => {
                                let f_str = fields
                                    .iter()
                                    .map(|f| {
                                        if let Some(ann) = &f.type_ann {
                                            format!("{}: {}", f.name, ann)
                                        } else {
                                            f.name.clone()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                items.push(json!({
                                    "label": name,
                                    "kind": 22, // Struct
                                    "detail": format!("struct {} {{ {} }}", name, f_str)
                                }));
                            }
                            crate::ast::StmtKind::EnumDef { name, .. } => {
                                items.push(json!({
                                    "label": name,
                                    "kind": 13, // Enum
                                    "detail": format!("enum {}", name)
                                }));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        items
    }

    fn compute_hover(&self, msg: &JsonValue) -> Option<JsonValue> {
        let uri = msg.pointer("/params/textDocument/uri")?.as_str()?;
        let line = msg.pointer("/params/position/line")?.as_u64()? as usize;
        let character = msg.pointer("/params/position/character")?.as_u64()? as usize;

        let source = self.documents.get(uri)?;
        let lines: Vec<&str> = source.lines().collect();
        let target_line = lines.get(line)?;

        let word = extract_word_at_pos(target_line, character)?;

        let doc = match word.as_str() {
            "print" => "**print(val)**\n\nPrints the value to stdout without a trailing newline.",
            "println" => "**println(val)**\n\nPrints the value to stdout with a trailing newline.",
            "input" => "**input([prompt])**\n\nPrompts the user and reads a line from stdin as a string without trailing newline.",
            "serve" => {
                "**serve(port, handler_or_router)**\n\nStarts a multi-threaded HTTP server with keep-alive connection pooling."
            }
            "serve_tls" => {
                "**serve_tls(port, handler_or_router, cert_path, key_path)**\n\nStarts an HTTPS server using TLS encryption."
            }
            "channel" => {
                "**channel([capacity])**\n\nCreates a thread-safe message-passing channel (bounded or unbounded)."
            }
            "spawn" => {
                "**spawn(fn)**\n\nSpawns a background worker thread executing the given function."
            }
            "join" => {
                "**join(task)**\n\nBlocks until the spawned task completes and returns its result."
            }
            "route_match" => {
                "**route_match(pattern, path)**\n\nMatches parameterized route patterns (e.g. `\"/users/:id\"`) and returns captured parameters."
            }
            "jit" => {
                "**jit(action, [target])**\n\nControls Cranelift JIT compilation (`\"status\"`, `\"enable\"`, `\"disable\"`, `\"compile\"`). Hot functions automatically compile to native x86_64/arm64 machine code."
            }
            "std:fs" => {
                "**std:fs**\n\nStandard library module for file system operations (read, write, append, mkdir, exists, readDir)."
            }
            "std:path" => {
                "**std:path**\n\nFile path manipulation (join, dirname, basename, extname, isAbsolute)."
            }
            "std:sys" => {
                "**std:sys**\n\nSystem utilities: platform, arch, cwd, exit, and cross-platform OS signal handling (on_signal, trigger_signal, clear_signals, poll_signals, signals)."
            }
            "std:time" => {
                "**std:time**\n\nDate, time, monotonic clocks, and thread sleeping utilities (now, nowMs, sleep)."
            }
            "std:crypto" => {
                "**std:crypto**\n\nCryptographic functions: SHA-256, SHA-512, HMAC, secure random bytes, and UUID v4."
            }
            "std:regex" => {
                "**std:regex**\n\nPCRE-compatible regular expressions: matching, searching, splitting, and substitution."
            }
            "std:codec" => {
                "**std:codec**\n\nEncoding/decoding codecs: Base64, URL percent-encoding, and Hex."
            }
            "std:http" => {
                "**std:http**\n\nHTTP client module supporting GET, POST, PUT, DELETE, query parameter encoding, and automatic JSON serialization."
            }
            "std:env" => {
                "**std:env**\n\nEnvironment variable management (get, set, has, remove, all) and .env file loading."
            }
            "std:cli" => {
                "**std:cli**\n\nCommand-line arguments and terminal ANSI styling utilities (args, style)."
            }
            "std:collections" => {
                "**std:collections**\n\nHigh-performance data structures including HashSets and collections."
            }
            "std:ffi" => {
                "**std:ffi**\n\nForeign Function Interface (FFI) to link and call C functions dynamically from shared libraries (.so, .dylib, .dll)."
            }
            "std:csv" => {
                "**std:csv**\n\nRFC 4180 compliant CSV parsing and serialization module (parse, stringify)."
            }
            "std:sqlite" => {
                "**std:sqlite**\n\nSQLite database bindings for fast, embedded relational storage and querying (open, execute, query, close)."
            }
            "std:gui" | "std:graphics" => {
                "**std:gui**\n\nNative 2D immediate-mode canvas & windowing engine.\n\nFunctions:\n- `window(title, width, height) -> win_id`\n- `is_open(win_id) -> bool`\n- `clear(win_id, [color])`\n- `rect(win_id, x, y, w, h, color)`\n- `rect_outline(win_id, x, y, w, h, color, [thick])`\n- `circle(win_id, cx, cy, r, color)`\n- `circle_outline(win_id, cx, cy, r, color)`\n- `line(win_id, x0, y0, x1, y1, color)`\n- `text(win_id, x, y, text, color, [scale])`\n- `is_key_down(win_id, key_name) -> bool`\n- `get_keys(win_id) -> array`\n- `mouse_pos(win_id) -> [x, y] | null`\n- `is_mouse_down(win_id, btn) -> bool`\n- `limit_fps(win_id, [target_fps])`\n- `render(win_id)`\n- `close(win_id)`"
            }
            "std:random" | "std:rand" => {
                "**std:random**\n\nFast seedable pseudo-random number generator.\n\nFunctions:\n- `int([max] | [min, max]) -> int`\n- `float([min, max]) -> float`\n- `bool([prob]) -> bool`\n- `choice(array) -> value`\n- `shuffle(array) -> array`\n- `seed(int_seed)`"
            }
            "std:math" => {
                "**std:math**\n\nMathematical functions and constants.\n\nFunctions:\n- `sqrt(n)`, `abs(n)`, `min(a, b)`, `max(a, b)`, `pow(base, exp)`\n- `floor(n)`, `ceil(n)`, `round(n)`, `clamp(val, min, max)`\n- Trigonometry: `sin(rad)`, `cos(rad)`, `tan(rad)`, `atan2(y, x)`\n\nConstants:\n- `PI` (3.14159...)\n- `E` (2.71828...)"
            }
            "std:io" => {
                "**std:io**\n\nStandard I/O stream utilities.\n\nFunctions:\n- `print(...args)`\n- `println(...args)`\n- `input([prompt]) -> string`\n- `flush()`"
            }
            "std:actor" => {
                "**std:actor**\n\nActor-based concurrency & fault-tolerant supervision.\n\nFunctions:\n- `spawn(handler)`, `spawn(initial_state, handler)`, `spawn_named(name, ...)`\n- `send(pid, msg)`, `ask(pid, msg, [timeout_ms])`\n- `register(name, pid)`, `whereis(name)`, `supervisor(config)`"
            }
            "std:actor:cluster" => {
                "**std:actor:cluster**\n\nDistributed actor mesh clustering with TLS & mutual-TLS encrypted transport."
            }
            "std:async" => {
                "**std:async**\n\nAsynchronous concurrency combinators and coroutines.\n\nFunctions:\n- `spawn(fn, ...args)`\n- `all(tasks) -> array`\n- `race(tasks) -> first_result`\n- `sleep(ms)`\n- `timeout(ms, task)`"
            }
            "std:unicode" => {
                "**std:unicode**\n\nUAX #29 Unicode grapheme clustering and rune safety.\n\nFunctions:\n- `graphemes(str) -> array`\n- `grapheme_len(str) -> int`\n- `slice_graphemes(str, start, end) -> str`\n- `runes(str) -> array`"
            }
            "std:test" => {
                "**std:test**\n\nPure-Shae assertion and testing framework.\n\nFunctions:\n- `test(name, fn)`\n- `assert(cond, [msg])`\n- `assert_eq(a, b, [msg])`\n- `assert_ne(a, b, [msg])`\n- `assert_throws(fn, [msg])`"
            }
            "std:buffer" => {
                "**std:buffer**\n\nZero-copy typed binary memory buffers and struct packing.\n\nFunctions:\n- `alloc(size)`, `from(bytes)`, `from_hex(str)`, `from_utf8(str)`\n- `calcsize(format)`, `pack(format, ...values)`, `unpack(format, buffer)`"
            }
            "fn" => "**fn keyword**\n\nDeclares a named function or lambda.",
            "let" => "**let keyword**\n\nBinds variables with optional destructuring.",
            "match" => {
                "**match expression**\n\nPattern matching with guards and exhaustiveness checking."
            }
            _ => return None,
        };

        Some(json!({
            "contents": {
                "kind": "markdown",
                "value": doc
            }
        }))
    }

    fn compute_formatting(&self, msg: &JsonValue) -> Vec<JsonValue> {
        let uri = match msg
            .pointer("/params/textDocument/uri")
            .and_then(|u| u.as_str())
        {
            Some(u) => u,
            None => return Vec::new(),
        };

        let source = match self.documents.get(uri) {
            Some(s) => s,
            None => return Vec::new(),
        };

        if let Ok(formatted) = crate::format_source(source) {
            if formatted != *source {
                let lines: Vec<&str> = source.lines().collect();
                let last_line = lines.len().saturating_sub(1);
                let last_col = lines.last().map(|l| l.len()).unwrap_or(0);

                return vec![json!({
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": last_line + 1, "character": last_col }
                    },
                    "newText": formatted
                })];
            }
        }

        Vec::new()
    }
}

fn extract_word_at_pos(line: &str, character: usize) -> Option<String> {
    if character > line.len() {
        return None;
    }

    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let pos = character.min(chars.len().saturating_sub(1));

    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_' || c == ':';

    let mut start = pos;
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }

    let mut end = pos;
    while end < chars.len() && is_ident_char(chars[end]) {
        end += 1;
    }

    if start == end {
        None
    } else {
        Some(chars[start..end].iter().collect())
    }
}

pub fn start_lsp_server() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = LspServer::new(stdin.lock(), stdout.lock());
    server.run()
}

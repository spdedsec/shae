use serde_json::{Value as JsonValue, json};
use shae::bundle;
use shae::lsp::LspServer;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct SharedOutput(Arc<Mutex<Vec<u8>>>);

impl Write for SharedOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

impl SharedOutput {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    fn take(&self) -> Vec<u8> {
        let mut guard = self.0.lock().unwrap();
        let bytes = guard.clone();
        guard.clear();
        bytes
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("shae_test_c10_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn format_lsp_message(val: &JsonValue) -> Vec<u8> {
    let payload = serde_json::to_string(val).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload).into_bytes()
}

fn parse_lsp_responses(output: &[u8]) -> Vec<JsonValue> {
    let mut responses = Vec::new();
    let mut cursor = 0;
    while cursor < output.len() {
        let slice = &output[cursor..];
        if let Some(header_end) = slice.windows(4).position(|w| w == b"\r\n\r\n") {
            let header_str = std::str::from_utf8(&slice[..header_end]).unwrap();
            let mut content_length = 0;
            for line in header_str.lines() {
                if let Some(l) = line.strip_prefix("Content-Length:") {
                    content_length = l.trim().parse().unwrap();
                }
            }
            let body_start = cursor + header_end + 4;
            let body_end = body_start + content_length;
            if body_end <= output.len() {
                let json_val: JsonValue =
                    serde_json::from_slice(&output[body_start..body_end]).unwrap();
                responses.push(json_val);
                cursor = body_end;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    responses
}

#[test]
fn test_lsp_initialize_and_capabilities() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let input = format_lsp_message(&req);
    let output = Vec::new();

    let mut server = LspServer::new(Cursor::new(input), Cursor::new(output));
    server.run().unwrap();

    let written = server.handle_message(req);
    assert!(written.is_ok());
}

#[test]
fn test_lsp_diagnostics_and_linting() {
    let output = SharedOutput::new();
    let mut server = LspServer::new(Cursor::new(Vec::new()), output.clone());

    // 1. Diagnostics on syntax error
    let bad_code = "fn broken( { let x = 1";
    server
        .publish_diagnostics("file:///test.shae", bad_code)
        .unwrap();

    let resps = parse_lsp_responses(&output.take());
    assert_eq!(resps.len(), 1);
    assert_eq!(resps[0]["method"], "textDocument/publishDiagnostics");
    let diags = resps[0]["params"]["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());
    assert_eq!(diags[0]["severity"], 1); // Error

    // 2. Diagnostics on unused variable warning
    let warn_code = "fn test() { let unused_var = 42\n 10 }";
    server
        .publish_diagnostics("file:///test_warn.shae", warn_code)
        .unwrap();
    let resps2 = parse_lsp_responses(&output.take());
    assert_eq!(resps2.len(), 1);
    let diags2 = resps2[0]["params"]["diagnostics"].as_array().unwrap();
    assert!(!diags2.is_empty());
    assert_eq!(diags2[0]["severity"], 2); // Warning
    assert!(
        diags2[0]["message"]
            .as_str()
            .unwrap()
            .contains("unused_var")
    );
}

#[test]
fn test_lsp_completion_and_hover_and_formatting() {
    let output = SharedOutput::new();
    let mut server = LspServer::new(Cursor::new(Vec::new()), output.clone());

    let doc = r#"fn compute(val) {
    let x = val * 2
    return x
}
"#;
    // Open document
    let open_msg = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///doc.shae",
                "text": doc
            }
        }
    });
    server.handle_message(open_msg).unwrap();
    let _ = output.take();

    // 1. Completion
    let comp_msg = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": "file:///doc.shae" },
            "position": { "line": 0, "character": 0 }
        }
    });
    server.handle_message(comp_msg).unwrap();
    let resps = parse_lsp_responses(&output.take());
    assert_eq!(resps.len(), 1);
    let items = resps[0]["result"].as_array().unwrap();
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.contains(&"fn"));
    assert!(labels.contains(&"serve"));
    assert!(labels.contains(&"std:crypto"));
    assert!(labels.contains(&"compute")); // in-document symbol

    // 2. Hover
    let hover_msg = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": "file:///doc.shae" },
            "position": { "line": 0, "character": 1 } // on 'fn'
        }
    });
    server.handle_message(hover_msg).unwrap();
    let resps2 = parse_lsp_responses(&output.take());
    assert_eq!(resps2.len(), 1);
    let hover_content = resps2[0]["result"]["contents"]["value"].as_str().unwrap();
    assert!(hover_content.contains("fn keyword"));

    // 3. Formatting
    let unformatted = "let    a=1+2\n";
    let open_unformatted = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///unformatted.shae",
                "text": unformatted
            }
        }
    });
    server.handle_message(open_unformatted).unwrap();
    let _ = output.take();

    let fmt_msg = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "textDocument/formatting",
        "params": {
            "textDocument": { "uri": "file:///unformatted.shae" }
        }
    });
    server.handle_message(fmt_msg).unwrap();
    let resps3 = parse_lsp_responses(&output.take());
    assert_eq!(resps3.len(), 1);
    let edits = resps3[0]["result"].as_array().unwrap();
    assert!(!edits.is_empty());
    assert_eq!(edits[0]["newText"], "let a = 1 + 2\n");
}

#[test]
fn test_bundle_collection_and_archive_roundtrip() {
    let temp = TempDir::new("bundle");
    let main_file = temp.path.join("main.shae");
    let helper_file = temp.path.join("helper.shae");

    fs::write(&helper_file, "fn get_answer() { 42 }\n").unwrap();
    fs::write(
        &main_file,
        "use { get_answer } from \"./helper.shae\"\nlet res = get_answer()\n",
    )
    .unwrap();

    let archive = bundle::collect_bundle(&main_file).expect("collect_bundle failed");
    assert!(archive.files.len() >= 2);
    assert!(
        archive.files.contains_key(
            &main_file
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string()
        )
    );
    assert!(
        archive.files.contains_key(
            &helper_file
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string()
        )
    );

    // Test standalone binary creation & trailer decoding
    let bin_path = temp.path.join("app.bin");
    bundle::create_standalone_binary(&main_file, &bin_path)
        .expect("create_standalone_binary failed");
    assert!(bin_path.exists());

    let decoded = bundle::read_embedded_bundle(&bin_path)
        .expect("read_embedded_bundle failed")
        .expect("embedded bundle should be present");

    assert_eq!(decoded.entry_path, archive.entry_path);
    assert_eq!(decoded.files.len(), archive.files.len());
}

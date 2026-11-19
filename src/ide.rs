use crate::eval::Evaluator;
use crate::fmt::format_source;
use crate::linter::Linter;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Deserialize)]
struct SaveRequest {
    path: String,
    content: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct RunRequest {
    code: Option<String>,
    path: Option<String>,
}

#[derive(Serialize)]
struct RunResponse {
    stdout: String,
    stderr: String,
    duration_ms: f64,
    exit_code: i32,
    result: Option<String>,
}

#[derive(Deserialize)]
struct CodeRequest {
    code: String,
}

#[derive(Serialize)]
struct FmtResponse {
    formatted: String,
    error: Option<String>,
}

#[derive(Serialize)]
struct CheckResponse {
    valid: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Deserialize)]
struct ReplRequest {
    input: String,
}

#[derive(Serialize)]
struct ReplResponse {
    output: String,
    is_error: bool,
}

// ---------------------------------------------------------------------------
// Embedded HTML5 / CSS3 / Vanilla JS IDE Frontend
// ---------------------------------------------------------------------------

const IDE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Shae Studio | The Creative & Systems Language</title>
<style>
:root {
  --bg-dark: #11111b;
  --bg-sidebar: #181825;
  --bg-editor: #1e1e2e;
  --bg-panel: #181825;
  --border: #313244;
  --text-main: #cdd6f4;
  --text-dim: #a6adc8;
  --accent-blue: #89b4fa;
  --accent-green: #a6e3a1;
  --accent-red: #f38ba8;
  --accent-yellow: #f9e2af;
  --accent-mauve: #cba6f7;
}

* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: 'JetBrains Mono', 'Fira Code', 'Segoe UI', monospace;
  background: var(--bg-dark);
  color: var(--text-main);
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* Header */
header {
  background: var(--bg-sidebar);
  border-bottom: 1px solid var(--border);
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  user-select: none;
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 700;
  font-size: 15px;
  color: var(--accent-blue);
  letter-spacing: 0.5px;
}
.brand span { color: var(--accent-green); }
.badge {
  font-size: 11px;
  background: var(--border);
  padding: 2px 6px;
  border-radius: 4px;
  color: var(--text-dim);
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}

button {
  background: var(--border);
  color: var(--text-main);
  border: none;
  border-radius: 4px;
  padding: 6px 12px;
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 6px;
  transition: all 0.15s ease;
}
button:hover { background: #45475a; }
button:active { transform: scale(0.98); }
button.primary {
  background: var(--accent-green);
  color: #11111b;
  font-weight: 600;
}
button.primary:hover { background: #b4f4af; }
select {
  background: var(--border);
  color: var(--text-main);
  border: 1px solid var(--border);
  padding: 6px 10px;
  border-radius: 4px;
  font-family: inherit;
  font-size: 12px;
  outline: none;
}

/* Layout */
.main-container {
  display: flex;
  flex: 1;
  overflow: hidden;
}

/* Sidebar */
aside {
  width: 240px;
  background: var(--bg-sidebar);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}

.sidebar-header {
  padding: 10px 14px;
  font-size: 12px;
  text-transform: uppercase;
  color: var(--text-dim);
  letter-spacing: 1px;
  border-bottom: 1px solid var(--border);
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.file-list {
  flex: 1;
  overflow-y: auto;
  list-style: none;
  padding: 6px 0;
}

.file-item {
  padding: 7px 14px;
  font-size: 13px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
  transition: background 0.1s;
}
.file-item:hover { background: #24253a; color: var(--text-main); }
.file-item.active { background: #313244; color: var(--accent-blue); font-weight: 600; }

/* Editor & Output Work Area */
.work-area {
  flex: 1;
  display: flex;
  flex-direction: column;
  background: var(--bg-editor);
}

.editor-wrapper {
  flex: 1;
  display: flex;
  position: relative;
  overflow: hidden;
}

.line-numbers {
  width: 48px;
  background: var(--bg-sidebar);
  border-right: 1px solid var(--border);
  color: #585b70;
  text-align: right;
  padding: 12px 8px;
  font-size: 13px;
  line-height: 20px;
  user-select: none;
  overflow: hidden;
}

textarea#editor {
  flex: 1;
  background: transparent;
  color: var(--text-main);
  border: none;
  outline: none;
  resize: none;
  padding: 12px 14px;
  font-family: inherit;
  font-size: 14px;
  line-height: 20px;
  white-space: pre;
  overflow-wrap: normal;
  overflow-x: auto;
  tab-size: 4;
}

/* Bottom Panel */
.panel {
  height: 220px;
  background: var(--bg-panel);
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}

.panel-tabs {
  display: flex;
  background: var(--bg-sidebar);
  border-bottom: 1px solid var(--border);
  user-select: none;
}

.tab {
  padding: 8px 16px;
  font-size: 12px;
  cursor: pointer;
  color: var(--text-dim);
  border-bottom: 2px solid transparent;
}
.tab:hover { color: var(--text-main); }
.tab.active {
  color: var(--accent-blue);
  border-bottom: 2px solid var(--accent-blue);
  font-weight: 600;
}

.panel-content {
  flex: 1;
  overflow-y: auto;
  padding: 10px 14px;
  font-size: 13px;
  line-height: 1.5;
}

pre#console-out {
  color: var(--text-main);
  white-space: pre-wrap;
  word-break: break-all;
}

.repl-box {
  display: flex;
  gap: 8px;
  margin-top: 6px;
}
.repl-box input {
  flex: 1;
  background: #11111b;
  border: 1px solid var(--border);
  color: var(--text-main);
  padding: 6px 10px;
  border-radius: 4px;
  font-family: inherit;
  font-size: 13px;
  outline: none;
}

/* Toast */
.toast {
  position: fixed;
  bottom: 24px;
  right: 24px;
  background: var(--border);
  color: var(--text-main);
  padding: 8px 16px;
  border-radius: 6px;
  font-size: 12px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.5);
  display: none;
  z-index: 1000;
}
</style>
</head>
<body>

<header>
  <div class="brand">
    <span>⚡</span> SHAE STUDIO
    <div class="badge">v0.6.0-dev</div>
    <div id="active-filename" class="badge" style="color:var(--accent-yellow)">main.shae</div>
  </div>
  <div class="toolbar">
    <select id="template-select" onchange="loadTemplate(this.value)">
      <option value="">Load Template...</option>
      <option value="snake">Snake Game (2D GUI)</option>
      <option value="particles">Particles Simulation (2D GUI)</option>
      <option value="synth">Audio Synthesizer (std:audio)</option>
      <option value="ui_calc">Calculator (std:ui)</option>
      <option value="actor">Actor Microservice (std:actor)</option>
      <option value="hello">Hello World</option>
    </select>
    <button onclick="formatCode()">Format (Fmt)</button>
    <button onclick="checkCode()">Check (Lint)</button>
    <button onclick="saveFile()">Save (Ctrl+S)</button>
    <button class="primary" onclick="runCode()">▶ Run (Ctrl+Enter)</button>
  </div>
</header>

<div class="main-container">
  <aside>
    <div class="sidebar-header">
      <span>Files</span>
      <button style="padding: 2px 6px; font-size:11px;" onclick="newFile()">+ New</button>
    </div>
    <ul class="file-list" id="file-list"></ul>
  </aside>

  <div class="work-area">
    <div class="editor-wrapper">
      <div class="line-numbers" id="line-numbers">1</div>
      <textarea id="editor" spellcheck="false" placeholder="// Write Shae code here..."></textarea>
    </div>

    <div class="panel">
      <div class="panel-tabs">
        <div class="tab active" onclick="switchTab('console')">Console Output</div>
        <div class="tab" onclick="switchTab('repl')">Interactive REPL</div>
        <div class="tab" onclick="switchTab('docs')">API Quick Reference</div>
      </div>
      <div class="panel-content" id="panel-body">
        <pre id="console-out">// Click "Run" or press Ctrl+Enter to execute Shae code...</pre>
      </div>
    </div>
  </div>
</div>

<div class="toast" id="toast"></div>

<script>
let currentFile = "main.shae";
const editor = document.getElementById("editor");
const lineNumbers = document.getElementById("line-numbers");
const consoleOut = document.getElementById("console-out");
const activeFilenameBadge = document.getElementById("active-filename");
let currentTab = "console";

function showToast(msg, color = "var(--text-main)") {
  const toast = document.getElementById("toast");
  toast.innerText = msg;
  toast.style.color = color;
  toast.style.display = "block";
  setTimeout(() => toast.style.display = "none", 2500);
}

function updateLineNumbers() {
  const lines = editor.value.split("\n").length;
  let nums = "";
  for (let i = 1; i <= lines; i++) { nums += i + "\n"; }
  lineNumbers.innerText = nums;
}

editor.addEventListener("input", updateLineNumbers);
editor.addEventListener("scroll", () => {
  lineNumbers.scrollTop = editor.scrollTop;
});

// Tab indentation handling
editor.addEventListener("keydown", (e) => {
  if (e.key === "Tab") {
    e.preventDefault();
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    editor.value = editor.value.substring(0, start) + "    " + editor.value.substring(end);
    editor.selectionStart = editor.selectionEnd = start + 4;
    updateLineNumbers();
  }
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
    e.preventDefault();
    runCode();
  }
  if ((e.ctrlKey || e.metaKey) && e.key === "s") {
    e.preventDefault();
    saveFile();
  }
});

async function loadFiles() {
  try {
    const res = await fetch("/api/files");
    const files = await res.json();
    const list = document.getElementById("file-list");
    list.innerHTML = "";
    files.forEach(f => {
      const li = document.createElement("li");
      li.className = "file-item" + (f.name === currentFile ? " active" : "");
      li.innerHTML = (f.is_dir ? "📁 " : "📄 ") + f.name;
      if (!f.is_dir) {
        li.onclick = () => openFile(f.name);
      }
      list.appendChild(li);
    });
  } catch (err) {
    console.error("Failed to load files", err);
  }
}

async function openFile(filename) {
  currentFile = filename;
  activeFilenameBadge.innerText = filename;
  try {
    const res = await fetch(`/api/file?path=${encodeURIComponent(filename)}`);
    const data = await res.json();
    editor.value = data.content || "";
    updateLineNumbers();
    loadFiles();
  } catch (err) {
    showToast("Error opening " + filename, "var(--accent-red)");
  }
}

async function saveFile() {
  try {
    const res = await fetch("/api/save", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: currentFile, content: editor.value })
    });
    if (res.ok) {
      showToast("Saved " + currentFile, "var(--accent-green)");
      loadFiles();
    } else {
      showToast("Failed to save", "var(--accent-red)");
    }
  } catch (err) {
    showToast("Network error saving file", "var(--accent-red)");
  }
}

async function runCode() {
  switchTab("console");
  consoleOut.innerText = "⏳ Running " + currentFile + "...\n";
  try {
    const start = performance.now();
    const res = await fetch("/api/run", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: editor.value, path: currentFile })
    });
    const data = await res.json();
    let out = "";
    if (data.stdout) out += data.stdout + "\n";
    if (data.stderr) out += "[stderr]\n" + data.stderr + "\n";
    if (data.result && data.result !== "null") out += "=> " + data.result + "\n";
    out += `\n[Finished in ${data.duration_ms.toFixed(2)} ms | Exit code: ${data.exit_code}]`;
    consoleOut.innerText = out;
  } catch (err) {
    consoleOut.innerText = "Execution failed: " + err;
  }
}

async function formatCode() {
  try {
    const res = await fetch("/api/fmt", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: editor.value })
    });
    const data = await res.json();
    if (data.error) {
      showToast("Format error: " + data.error, "var(--accent-red)");
    } else {
      editor.value = data.formatted;
      updateLineNumbers();
      showToast("Code formatted!", "var(--accent-blue)");
    }
  } catch (err) {
    showToast("Format failed", "var(--accent-red)");
  }
}

async function checkCode() {
  switchTab("console");
  consoleOut.innerText = "🔍 Checking code syntax & linter...\n";
  try {
    const res = await fetch("/api/check", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: editor.value })
    });
    const data = await res.json();
    if (data.valid && data.warnings.length === 0) {
      consoleOut.innerText = "✅ 0 errors, 0 warnings. Code is clean!";
    } else {
      let out = "";
      if (data.errors.length > 0) {
        out += "❌ Errors (" + data.errors.length + "):\n";
        data.errors.forEach(e => out += "  • " + e + "\n");
      }
      if (data.warnings.length > 0) {
        out += "⚠️ Warnings (" + data.warnings.length + "):\n";
        data.warnings.forEach(w => out += "  • " + w + "\n");
      }
      consoleOut.innerText = out;
    }
  } catch (err) {
    consoleOut.innerText = "Check failed: " + err;
  }
}

function switchTab(tab) {
  currentTab = tab;
  const tabs = document.querySelectorAll(".panel-tabs .tab");
  tabs.forEach(t => t.classList.remove("active"));
  const body = document.getElementById("panel-body");

  if (tab === "console") {
    tabs[0].classList.add("active");
    body.innerHTML = `<pre id="console-out">${consoleOut ? consoleOut.innerText : ""}</pre>`;
  } else if (tab === "repl") {
    tabs[1].classList.add("active");
    body.innerHTML = `
      <pre id="repl-history">// Shae Interactive REPL Console</pre>
      <div class="repl-box">
        <input type="text" id="repl-input" placeholder="Enter Shae expression (e.g. 10 * 20 + 5) and press Enter" onkeydown="handleReplKey(event)">
      </div>
    `;
  } else if (tab === "docs") {
    tabs[2].classList.add("active");
    body.innerHTML = `
      <div style="font-size:12px; line-height:1.6;">
        <h4 style="color:var(--accent-blue)">Standard Library Quick Reference</h4>
        <p>• <b>std:gui</b>: window, clear, rect, circle, line, text, render, is_key_down, mouse_pos, load_image, draw_image, draw_sprite</p>
        <p>• <b>std:audio</b>: beep(freq, ms), tone(freq, wave, ms, vol), play(path), loop(path), stop(), set_volume(vol)</p>
        <p>• <b>std:ui</b>: button(win, txt, x, y), slider(win, lbl, x, y, w, min, max, cur), checkbox, panel, label, progress_bar</p>
        <p>• <b>std:random</b>: int(a, b), float(), choice(arr), shuffle(arr), seed(val)</p>
        <p>• <b>std:math</b>: sqrt, sin, cos, tan, pow, min, max, clamp, floor, ceil, PI, E</p>
        <p>• <b>std:actor</b>: spawn(state, fn), send(pid, msg), ask(pid, msg, timeout), supervisor(cfg)</p>
      </div>
    `;
  }
}

async function handleReplKey(e) {
  if (e.key === "Enter") {
    const input = document.getElementById("repl-input");
    const val = input.value.trim();
    if (!val) return;
    const history = document.getElementById("repl-history");
    history.innerText += "\n> " + val;
    input.value = "";
    try {
      const res = await fetch("/api/repl", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ input: val })
      });
      const data = await res.json();
      history.innerText += "\n" + (data.is_error ? "❌ " : "=> ") + data.output;
      history.scrollTop = history.scrollHeight;
    } catch (err) {
      history.innerText += "\n❌ REPL error: " + err;
    }
  }
}

function newFile() {
  const name = prompt("Enter new Shae file name (e.g. game.shae):");
  if (name && name.trim()) {
    openFile(name.trim());
  }
}

function loadTemplate(kind) {
  if (!kind) return;
  if (kind === "snake") {
    editor.value = `let gui = use "std:gui"\nlet rand = use "std:random"\n\nlet win = gui.window("Shae Snake Game", 640, 480)\nlet x = 320\nlet y = 240\n\nwhile gui.is_open(win) {\n    gui.clear(win, 0x181825)\n    if gui.is_key_down(win, "Left") { x = x - 4 }\n    if gui.is_key_down(win, "Right") { x = x + 4 }\n    if gui.is_key_down(win, "Up") { y = y - 4 }\n    if gui.is_key_down(win, "Down") { y = y + 4 }\n\n    gui.rect(win, x, y, 20, 20, 0xA6E3A1)\n    gui.text(win, 20, 20, "ARROW KEYS TO MOVE", 0xF9E2AF, 2)\n    gui.render(win)\n    gui.limit_fps(win, 60)\n}\n`;
  } else if (kind === "synth") {
    editor.value = `let audio = use "std:audio"\nlet gui = use "std:gui"\n\nlet win = gui.window("Shae Audio Synthesizer", 600, 300)\nprintln("Press keys 1-7 or A-G to play tones!")\n\nwhile gui.is_open(win) {\n    gui.clear(win, 0x1E1E2E)\n    gui.text(win, 30, 40, "SHAE SYNTHESIZER PIANO", 0x89B4FA, 2)\n    gui.text(win, 30, 90, "Press 1=C4, 2=D4, 3=E4, 4=F4, 5=G4, 6=A4, 7=B4", 0xCDD6F4, 1)\n\n    if gui.is_key_down(win, "1") { audio.beep(261.63, 150) }\n    if gui.is_key_down(win, "2") { audio.beep(293.66, 150) }\n    if gui.is_key_down(win, "3") { audio.beep(329.63, 150) }\n    if gui.is_key_down(win, "4") { audio.beep(349.23, 150) }\n    if gui.is_key_down(win, "5") { audio.beep(392.00, 150) }\n    if gui.is_key_down(win, "6") { audio.beep(440.00, 150) }\n    if gui.is_key_down(win, "7") { audio.beep(493.88, 150) }\n\n    gui.render(win)\n    gui.limit_fps(win, 30)\n}\n`;
  } else if (kind === "ui_calc") {
    editor.value = `let gui = use "std:gui"\nlet ui = use "std:ui"\n\nlet win = gui.window("Shae Calculator", 400, 450)\nlet display = "0"\n\nwhile gui.is_open(win) {\n    gui.clear(win, 0x11111B)\n    ui.panel(win, "CALCULATOR", 20, 20, 360, 400, 0x181825, 0x313244)\n    ui.label(win, display, 40, 60, 0xA6E3A1, 3)\n\n    if ui.button(win, "C", 40, 120, 60, 45) { display = "0" }\n    if ui.button(win, "7", 40, 180, 60, 45) { if display == "0" { display = "7" } else { display = display + "7" } }\n    if ui.button(win, "8", 120, 180, 60, 45) { if display == "0" { display = "8" } else { display = display + "8" } }\n    if ui.button(win, "9", 200, 180, 60, 45) { if display == "0" { display = "9" } else { display = display + "9" } }\n    if ui.button(win, "+", 280, 180, 60, 45) { display = display + " + " }\n\n    gui.render(win)\n    gui.limit_fps(win, 60)\n}\n`;
  } else if (kind === "actor") {
    editor.value = `let actor = use "std:actor"\n\nlet worker = actor.spawn(0, fn(state, msg) {\n    println("Worker received: " + str(msg))\n    return state + 1\n})\n\nactor.send(worker, "Task 1")\nactor.send(worker, "Task 2")\nprintln("Actor spawned successfully!")\n`;
  } else {
    editor.value = `println("Hello, Shae Universe!")\n`;
  }
  updateLineNumbers();
}

// Initial load
loadFiles();
openFile("main.shae");
</script>
</body>
</html>
"#;

// ---------------------------------------------------------------------------
// Server Engine
// ---------------------------------------------------------------------------

pub fn start_ide_server(
    workspace_dir: &Path,
    host: &str,
    port: u16,
    open_browser: bool,
) -> Result<(), std::io::Error> {
    let root = fs::canonicalize(workspace_dir)?;
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let listener = TcpListener::bind(addr)?;
    let local_addr = listener.local_addr()?;
    println!(
        "\n🚀 \x1b[1;32mShae Studio Web IDE running at:\x1b[0m \x1b[1;36mhttp://{}\x1b[0m",
        local_addr
    );
    println!("   📁 Workspace: {}", root.display());
    println!("   Press Ctrl+C to terminate the IDE server.\n");

    if open_browser {
        let url = format!("http://{}", local_addr);
        let _ = std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .or_else(|_| std::process::Command::new("open").arg(&url).spawn());
    }

    let repl_evaluator = Arc::new(Mutex::new(Evaluator::new()));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root_clone = root.clone();
                let repl_clone = Arc::clone(&repl_evaluator);
                std::thread::spawn(move || {
                    let _ = handle_ide_client(stream, &root_clone, repl_clone);
                });
            }
            Err(e) => {
                eprintln!("Connection error: {}", e);
            }
        }
    }
    Ok(())
}

fn handle_ide_client(
    mut stream: TcpStream,
    root: &Path,
    repl: Arc<Mutex<Evaluator>>,
) -> Result<(), std::io::Error> {
    let mut buf = [0u8; 65536];
    let bytes_read = stream.read(&mut buf)?;
    if bytes_read == 0 {
        return Ok(());
    }

    let request_str = String::from_utf8_lossy(&buf[..bytes_read]);
    let mut lines = request_str.lines();
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let full_path = parts.next().unwrap_or("/");

    let (path, query) = if let Some(idx) = full_path.find('?') {
        (&full_path[..idx], &full_path[idx + 1..])
    } else {
        (full_path, "")
    };

    // Extract body if present
    let body = if let Some(idx) = request_str.find("\r\n\r\n") {
        &request_str[idx + 4..]
    } else {
        ""
    };

    match (method, path) {
        ("GET", "/") => {
            send_response(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                IDE_HTML.as_bytes(),
            )?;
        }
        ("GET", "/api/files") => {
            let files = list_files(root);
            let json = serde_json::to_string(&files).unwrap_or("[]".to_string());
            send_response(&mut stream, 200, "application/json", json.as_bytes())?;
        }
        ("GET", "/api/file") => {
            let target_param = query.split('&').find_map(|pair| {
                let mut kv = pair.split('=');
                if kv.next() == Some("path") {
                    kv.next().map(|v| percent_decode(v))
                } else {
                    None
                }
            });

            let file_rel = target_param.unwrap_or_else(|| "main.shae".to_string());
            let full_file = root.join(&file_rel);
            let content = fs::read_to_string(&full_file).unwrap_or_default();
            let json = serde_json::json!({ "content": content }).to_string();
            send_response(&mut stream, 200, "application/json", json.as_bytes())?;
        }
        ("POST", "/api/save") => {
            if let Ok(req) = serde_json::from_str::<SaveRequest>(body) {
                let full_file = root.join(&req.path);
                if let Some(parent) = full_file.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                match fs::write(&full_file, req.content) {
                    Ok(_) => {
                        send_response(&mut stream, 200, "application/json", b"{\"status\":\"ok\"}")?
                    }
                    Err(e) => send_response(
                        &mut stream,
                        500,
                        "application/json",
                        format!("{{\"error\":\"{}\"}}", e).as_bytes(),
                    )?,
                }
            } else {
                send_response(&mut stream, 400, "text/plain", b"Invalid save payload")?;
            }
        }
        ("POST", "/api/run") => {
            if let Ok(req) = serde_json::from_str::<RunRequest>(body) {
                let code_to_run = req.code.unwrap_or_default();
                let start = Instant::now();

                let mut ev = Evaluator::new();
                let mut captured_stderr = String::new();
                let mut exit_code = 0;
                let mut result_str = None;

                crate::test_runner::start_stdout_capture();

                // Lex, parse, evaluate
                let mut lexer = crate::lexer::Lexer::new(&code_to_run);
                match lexer.tokenize() {
                    Ok(tokens) => {
                        let mut parser = crate::parser::Parser::new(tokens);
                        match parser.parse() {
                            Ok(stmts) => match ev.eval_program(&stmts) {
                                Ok(val) => {
                                    result_str = Some(val.to_display());
                                }
                                Err(e) => {
                                    captured_stderr = format!("Runtime Error: {}", e.message);
                                    exit_code = 1;
                                }
                            },
                            Err(e) => {
                                captured_stderr = format!("Parser Error: {}", e);
                                exit_code = 1;
                            }
                        }
                    }
                    Err(e) => {
                        captured_stderr = format!("Lexer Error: {}", e);
                        exit_code = 1;
                    }
                }

                let captured_stdout = crate::test_runner::stop_stdout_capture();

                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                let res = RunResponse {
                    stdout: captured_stdout,
                    stderr: captured_stderr,
                    duration_ms: elapsed_ms,
                    exit_code,
                    result: result_str,
                };
                let json = serde_json::to_string(&res).unwrap_or_default();
                send_response(&mut stream, 200, "application/json", json.as_bytes())?;
            } else {
                send_response(&mut stream, 400, "text/plain", b"Invalid run payload")?;
            }
        }
        ("POST", "/api/fmt") => {
            if let Ok(req) = serde_json::from_str::<CodeRequest>(body) {
                match format_source(&req.code) {
                    Ok(formatted) => {
                        let res = FmtResponse {
                            formatted,
                            error: None,
                        };
                        let json = serde_json::to_string(&res).unwrap();
                        send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                    }
                    Err(e) => {
                        let res = FmtResponse {
                            formatted: req.code,
                            error: Some(format!("{}", e)),
                        };
                        let json = serde_json::to_string(&res).unwrap();
                        send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                    }
                }
            } else {
                send_response(&mut stream, 400, "text/plain", b"Invalid fmt payload")?;
            }
        }
        ("POST", "/api/check") => {
            if let Ok(req) = serde_json::from_str::<CodeRequest>(body) {
                let mut warnings = Vec::new();
                let mut errors = Vec::new();

                let mut lexer = crate::lexer::Lexer::new(&req.code);
                match lexer.tokenize() {
                    Ok(tokens) => {
                        let mut parser = crate::parser::Parser::new(tokens);
                        match parser.parse() {
                            Ok(stmts) => {
                                let mut linter = Linter::new();
                                let diagnostics = linter.lint_program(&stmts);
                                for diag in diagnostics {
                                    let msg = format!(
                                        "{}:{}: {}",
                                        diag.span.line, diag.span.col, diag.message
                                    );
                                    if diag.severity == crate::linter::DiagnosticSeverity::Error {
                                        errors.push(msg);
                                    } else {
                                        warnings.push(msg);
                                    }
                                }
                            }
                            Err(e) => {
                                errors.push(format!("Syntax error: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("Lexer error: {}", e));
                    }
                }

                let valid = errors.is_empty();
                let res = CheckResponse {
                    valid,
                    warnings,
                    errors,
                };
                let json = serde_json::to_string(&res).unwrap();
                send_response(&mut stream, 200, "application/json", json.as_bytes())?;
            } else {
                send_response(&mut stream, 400, "text/plain", b"Invalid check payload")?;
            }
        }
        ("POST", "/api/repl") => {
            if let Ok(req) = serde_json::from_str::<ReplRequest>(body) {
                let mut ev = repl.lock().unwrap();
                let mut lexer = crate::lexer::Lexer::new(&req.input);
                match lexer.tokenize() {
                    Ok(tokens) => {
                        let mut parser = crate::parser::Parser::new(tokens);
                        match parser.parse() {
                            Ok(stmts) => match ev.eval_program(&stmts) {
                                Ok(val) => {
                                    let res = ReplResponse {
                                        output: val.to_display(),
                                        is_error: false,
                                    };
                                    let json = serde_json::to_string(&res).unwrap();
                                    send_response(
                                        &mut stream,
                                        200,
                                        "application/json",
                                        json.as_bytes(),
                                    )?;
                                }
                                Err(e) => {
                                    let res = ReplResponse {
                                        output: format!("Runtime error: {}", e.message),
                                        is_error: true,
                                    };
                                    let json = serde_json::to_string(&res).unwrap();
                                    send_response(
                                        &mut stream,
                                        200,
                                        "application/json",
                                        json.as_bytes(),
                                    )?;
                                }
                            },
                            Err(e) => {
                                let res = ReplResponse {
                                    output: format!("Syntax error: {}", e),
                                    is_error: true,
                                };
                                let json = serde_json::to_string(&res).unwrap();
                                send_response(
                                    &mut stream,
                                    200,
                                    "application/json",
                                    json.as_bytes(),
                                )?;
                            }
                        }
                    }
                    Err(e) => {
                        let res = ReplResponse {
                            output: format!("Lexer error: {}", e),
                            is_error: true,
                        };
                        let json = serde_json::to_string(&res).unwrap();
                        send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                    }
                }
            } else {
                send_response(&mut stream, 400, "text/plain", b"Invalid repl payload")?;
            }
        }
        _ => {
            send_response(&mut stream, 404, "text/plain", b"404 Not Found")?;
        }
    }
    Ok(())
}

fn list_files(root: &Path) -> Vec<FileEntry> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden directories and build targets
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }

            let is_dir = path.is_dir();
            let size = if is_dir {
                0
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            };

            result.push(FileEntry {
                name,
                path: path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string(),
                is_dir,
                size,
            });
        }
    }
    // Sort directories first, then alphabetical
    result.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            a.name.cmp(&b.name)
        }
    });
    result
}

fn percent_decode(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

fn send_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let status_line = match status {
        200 => "HTTP/1.1 200 OK",
        400 => "HTTP/1.1 400 Bad Request",
        404 => "HTTP/1.1 404 Not Found",
        500 => "HTTP/1.1 500 Internal Server Error",
        _ => "HTTP/1.1 200 OK",
    };

    let response_headers = format!(
        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status_line,
        content_type,
        body.len()
    );

    stream.write_all(response_headers.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

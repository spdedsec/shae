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

use std::collections::HashMap;
use std::sync::Condvar;

#[derive(Serialize, Deserialize, Clone)]
pub struct DiagnosticDto {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub message: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugStackFrameDto {
    pub id: usize,
    pub name: String,
    pub line: usize,
    pub file: String,
    pub locals: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugStateResponse {
    pub active: bool,
    pub stopped: bool,
    pub reason: Option<String>,
    pub current_line: usize,
    pub stack_frames: Vec<DebugStackFrameDto>,
    pub variables: HashMap<String, String>,
    pub is_terminated: bool,
}

#[derive(Deserialize)]
struct DebugStartRequest {
    code: String,
    #[serde(default)]
    breakpoints: Vec<usize>,
}

#[derive(Deserialize)]
struct DebugStepRequest {
    #[serde(default)]
    mode: Option<String>,
}

pub struct ActiveIdeDebugger {
    pub session: Arc<Mutex<crate::debugger::DebugSession>>,
    pub pause_cv: Arc<(Mutex<bool>, Condvar)>,
    pub handle: Option<std::thread::JoinHandle<()>>,
}

static ACTIVE_DEBUGGER: std::sync::OnceLock<Mutex<Option<ActiveIdeDebugger>>> =
    std::sync::OnceLock::new();

fn get_active_debugger() -> &'static Mutex<Option<ActiveIdeDebugger>> {
    ACTIVE_DEBUGGER.get_or_init(|| Mutex::new(None))
}

fn build_debug_state_response(s: &crate::debugger::DebugSession) -> DebugStateResponse {
    let mut stack_frames = Vec::new();
    let mut variables = HashMap::new();

    for (idx, frame) in s.snapshot_frames.iter().enumerate() {
        let mut frame_locals = HashMap::new();
        for (k, v) in &frame.locals {
            frame_locals.insert(k.clone(), v.to_display());
            if idx == 0 {
                variables.insert(k.clone(), v.to_display());
            }
        }
        stack_frames.push(DebugStackFrameDto {
            id: frame.id,
            name: frame.name.clone(),
            line: frame.line,
            file: frame.file.clone(),
            locals: frame_locals,
        });
    }

    DebugStateResponse {
        active: !s.is_terminated,
        stopped: s.is_stopped,
        reason: s.stop_reason.clone(),
        current_line: s.last_stopped_line,
        stack_frames,
        variables,
        is_terminated: s.is_terminated,
    }
}

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
    diagnostics: Vec<DiagnosticDto>,
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
    <div id="diagnostics-badge" class="badge" style="color:var(--accent-green); cursor:pointer;" onclick="checkCode()">✓ Clean</div>
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
    <button onclick="formatCode()">Format</button>
    <button onclick="checkCode()">Check</button>
    <button onclick="saveFile()">Save</button>
    <button class="primary" onclick="runCode()">▶ Run</button>
    <button id="btn-dbg-start" style="background:#cba6f7; color:#11111b; font-weight:600;" onclick="startDebug()">🐞 Debug</button>
    <button id="btn-dbg-step" style="display:none; background:#89b4fa; color:#11111b; font-weight:600;" onclick="stepDebug('over')">↷ Step</button>
    <button id="btn-dbg-into" style="display:none; background:#89b4fa; color:#11111b; font-weight:600;" onclick="stepDebug('into')">↓ Into</button>
    <button id="btn-dbg-continue" style="display:none; background:#a6e3a1; color:#11111b; font-weight:600;" onclick="continueDebug()">⏩ Continue</button>
    <button id="btn-dbg-stop" style="display:none; background:#f38ba8; color:#11111b; font-weight:600;" onclick="stopDebug()">⏹ Stop</button>
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
        <div class="tab" onclick="switchTab('debug')">Visual Debugger (DAP)</div>
        <div class="tab" onclick="switchTab('gui')">std:gui Live Preview</div>
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
  const badge = document.getElementById("diagnostics-badge");
  consoleOut.innerText = "🔍 Checking code syntax & linter...\n";
  try {
    const res = await fetch("/api/check", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: editor.value })
    });
    const data = await res.json();
    if (badge) {
      if (data.valid && (!data.warnings || data.warnings.length === 0)) {
        badge.textContent = "✓ Clean";
        badge.style.color = "var(--accent-green)";
      } else if (data.errors && data.errors.length > 0) {
        badge.textContent = `⚠ ${data.errors.length} error(s)`;
        badge.style.color = "var(--accent-red)";
      } else {
        badge.textContent = `⚡ ${data.warnings.length} warning(s)`;
        badge.style.color = "var(--accent-yellow)";
      }
    }
    if (data.valid && (!data.warnings || data.warnings.length === 0)) {
      consoleOut.innerText = "✅ 0 errors, 0 warnings. Code is clean!";
    } else {
      let out = "";
      if (data.errors && data.errors.length > 0) {
        out += "❌ Errors (" + data.errors.length + "):\n";
        data.errors.forEach(e => out += "  • " + e + "\n");
      }
      if (data.warnings && data.warnings.length > 0) {
        out += "⚠️ Warnings (" + data.warnings.length + "):\n";
        data.warnings.forEach(w => out += "  • " + w + "\n");
      }
      consoleOut.innerText = out;
    }
  } catch (err) {
    consoleOut.innerText = "Check failed: " + err;
  }
}


let activeDebugState = null;
let guiPollInterval = null;

async function startDebug() {
  const code = editor.value;
  switchTab('debug');
  showToast("Starting debugger session...", "var(--accent-mauve)");
  try {
    const res = await fetch('/api/debug/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ code: code, breakpoints: [] })
    });
    activeDebugState = await res.json();
    updateDebugUI();
  } catch (err) {
    showToast("Debug start error: " + err, "var(--accent-red)");
  }
}

async function stepDebug(mode) {
  try {
    const res = await fetch('/api/debug/step', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ mode: mode })
    });
    activeDebugState = await res.json();
    updateDebugUI();
  } catch (err) {
    showToast("Step error: " + err, "var(--accent-red)");
  }
}

async function continueDebug() {
  try {
    const res = await fetch('/api/debug/continue', { method: 'POST' });
    activeDebugState = await res.json();
    updateDebugUI();
  } catch (err) {
    showToast("Continue error: " + err, "var(--accent-red)");
  }
}

async function stopDebug() {
  try {
    await fetch('/api/debug/stop', { method: 'POST' });
    activeDebugState = null;
    updateDebugUI();
    showToast("Debugger stopped", "var(--text-dim)");
  } catch (err) {
    console.error(err);
  }
}

function updateDebugUI() {
  const btnStart = document.getElementById('btn-dbg-start');
  const btnStep = document.getElementById('btn-dbg-step');
  const btnInto = document.getElementById('btn-dbg-into');
  const btnCont = document.getElementById('btn-dbg-continue');
  const btnStop = document.getElementById('btn-dbg-stop');

  if (activeDebugState && activeDebugState.active && !activeDebugState.is_terminated) {
    if (btnStart) btnStart.style.display = 'none';
    if (btnStep) btnStep.style.display = 'inline-flex';
    if (btnInto) btnInto.style.display = 'inline-flex';
    if (btnCont) btnCont.style.display = 'inline-flex';
    if (btnStop) btnStop.style.display = 'inline-flex';
  } else {
    if (btnStart) btnStart.style.display = 'inline-flex';
    if (btnStep) btnStep.style.display = 'none';
    if (btnInto) btnInto.style.display = 'none';
    if (btnCont) btnCont.style.display = 'none';
    if (btnStop) btnStop.style.display = 'none';
  }

  if (currentTab === 'debug') {
    renderDebugPanel();
  }
}

function renderDebugPanel() {
  const panel = document.getElementById('panel-body');
  if (!activeDebugState) {
    panel.innerHTML = '<div style="padding:16px; color:var(--text-dim);">No active debug session. Click "🐞 Debug" in the toolbar to step through Shae code.</div>';
    return;
  }
  let html = '<div style="display:flex; height:100%; gap:16px; padding:12px; overflow:hidden;">';
  html += '<div style="flex:1; border-right:1px solid var(--border); overflow-y:auto;">';
  html += '<div style="font-weight:700; margin-bottom:8px; color:var(--accent-mauve);">CALL STACK</div>';
  html += '<ul style="list-style:none;">';
  if (activeDebugState.stack_frames && activeDebugState.stack_frames.length > 0) {
    activeDebugState.stack_frames.forEach(f => {
      html += `<li style="padding:4px 8px; border-radius:4px; font-size:12px; background:#24253a; margin-bottom:4px;"><b>${f.name}</b> (${f.file}:${f.line})</li>`;
    });
  } else {
    html += '<li style="font-size:12px; color:var(--text-dim);">&lt;main&gt; (line ' + activeDebugState.current_line + ')</li>';
  }
  html += '</ul></div>';

  html += '<div style="flex:1; overflow-y:auto;">';
  html += '<div style="font-weight:700; margin-bottom:8px; color:var(--accent-blue);">LOCAL VARIABLES</div>';
  html += '<table style="width:100%; font-size:12px; border-collapse:collapse;">';
  html += '<thead><tr style="text-align:left; color:var(--text-dim); border-bottom:1px solid var(--border);"><th style="padding:4px;">Variable</th><th style="padding:4px;">Value</th></tr></thead><tbody>';
  if (activeDebugState.variables && Object.keys(activeDebugState.variables).length > 0) {
    for (const [k, v] of Object.entries(activeDebugState.variables)) {
      html += `<tr style="border-bottom:1px solid rgba(255,255,255,0.05);"><td style="padding:4px; font-weight:600; color:var(--accent-yellow);">${k}</td><td style="padding:4px; color:var(--text-main); font-family:monospace;">${v}</td></tr>`;
    }
  } else {
    html += '<tr><td colspan="2" style="padding:6px; color:var(--text-dim);">No locals initialized yet</td></tr>';
  }
  html += '</tbody></table></div></div>';
  panel.innerHTML = html;
}

function renderGuiPanel() {
  const panel = document.getElementById('panel-body');
  panel.innerHTML = '<div style="display:flex; flex-direction:column; align-items:center; justify-content:center; height:100%; padding:12px; background:#000;">' +
    '<canvas id="gui-preview-canvas" width="400" height="240" style="max-height:100%; border:1px solid var(--border); border-radius:4px; image-rendering:pixelated;"></canvas>' +
    '<div style="font-size:11px; color:var(--text-dim); margin-top:6px;">Live std:gui Framebuffer View</div></div>';
  pollGuiFrame();
}

async function pollGuiFrame() {
  if (currentTab !== 'gui') return;
  try {
    const res = await fetch('/api/gui/frame');
    if (res.ok) {
      const data = await res.json();
      const canvas = document.getElementById('gui-preview-canvas');
      if (canvas && data.width > 0 && data.height > 0) {
        canvas.width = data.width;
        canvas.height = data.height;
        const ctx = canvas.getContext('2d');
        if (ctx && data.buffer && data.buffer.length > 0) {
          const pixelBytes = new Uint8ClampedArray(data.buffer.length * 4);
          for (let i = 0; i < data.buffer.length; i++) {
            const u = data.buffer[i];
            pixelBytes[i * 4] = (u >> 16) & 0xFF;
            pixelBytes[i * 4 + 1] = (u >> 8) & 0xFF;
            pixelBytes[i * 4 + 2] = u & 0xFF;
            pixelBytes[i * 4 + 3] = (u >> 24) & 0xFF || 255;
          }
          const img = new ImageData(pixelBytes, data.width, data.height);
          ctx.putImageData(img, 0, 0);
        }
      }
    }
  } catch (e) {
    console.warn("GUI frame poll error:", e);
  }
}

function switchTab(tab) {
  currentTab = tab;
  if (guiPollInterval) { clearInterval(guiPollInterval); guiPollInterval = null; }
  const tabs = document.querySelectorAll(".panel-tabs .tab");
  tabs.forEach(t => t.classList.remove("active"));
  const body = document.getElementById("panel-body");

  if (tab === "console") {
    if (tabs[0]) tabs[0].classList.add("active");
    body.innerHTML = `<pre id="console-out">${consoleOut ? consoleOut.innerText : ""}</pre>`;
  } else if (tab === "repl") {
    if (tabs[1]) tabs[1].classList.add("active");
    body.innerHTML = `
      <pre id="repl-history">// Shae Interactive REPL Console</pre>
      <div class="repl-box">
        <input type="text" id="repl-input" placeholder="Enter Shae expression (e.g. 10 * 20 + 5) and press Enter" onkeydown="handleReplKey(event)">
      </div>
    `;
  } else if (tab === "debug") {
    if (tabs[2]) tabs[2].classList.add("active");
    renderDebugPanel();
  } else if (tab === "gui") {
    if (tabs[3]) tabs[3].classList.add("active");
    renderGuiPanel();
    guiPollInterval = setInterval(pollGuiFrame, 150);
  } else if (tab === "docs") {
    if (tabs[4]) tabs[4].classList.add("active");
    body.innerHTML = `
      <div style="font-size:12px; line-height:1.6;">
        <h4 style="color:var(--accent-blue)">Standard Library Quick Reference</h4>
        <p>• <b>std:gui</b>: window, clear, rect, circle, line, text, render, is_key_down, mouse_pos, load_image, draw_image, draw_sprite</p>
        <p>• <b>std:ws</b>: connect, listen, send, send_binary, recv, ping, pong, close, is_open</p>
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

pub fn handle_ide_client_test(
    stream: TcpStream,
    root: &Path,
    repl: Arc<Mutex<Evaluator>>,
) -> Result<(), std::io::Error> {
    handle_ide_client(stream, root, repl)
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
        ("GET", "/api/gui/frame") => {
            let registry = crate::gui::get_window_registry().lock().unwrap();
            if let Some((_id, win)) = registry.iter().next() {
                let frame_resp = serde_json::json!({
                    "width": win.width,
                    "height": win.height,
                    "buffer": win.buffer,
                });
                send_response(
                    &mut stream,
                    200,
                    "application/json",
                    frame_resp.to_string().as_bytes(),
                )?;
            } else {
                let frame_resp = serde_json::json!({
                    "width": 0,
                    "height": 0,
                    "buffer": [],
                });
                send_response(
                    &mut stream,
                    200,
                    "application/json",
                    frame_resp.to_string().as_bytes(),
                )?;
            }
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

                let mut diagnostics = Vec::new();
                for w in &warnings {
                    diagnostics.push(DiagnosticDto {
                        line: 1,
                        col: 1,
                        end_line: 1,
                        end_col: 1,
                        message: w.clone(),
                        severity: "warning".to_string(),
                    });
                }
                for e in &errors {
                    diagnostics.push(DiagnosticDto {
                        line: 1,
                        col: 1,
                        end_line: 1,
                        end_col: 1,
                        message: e.clone(),
                        severity: "error".to_string(),
                    });
                }
                let valid = errors.is_empty();
                let res = CheckResponse {
                    valid,
                    warnings,
                    errors,
                    diagnostics,
                };
                let json = serde_json::to_string(&res).unwrap();
                send_response(&mut stream, 200, "application/json", json.as_bytes())?;
            } else {
                send_response(&mut stream, 400, "text/plain", b"Invalid check payload")?;
            }
        }
        ("POST", "/api/debug/start") => {
            if let Ok(req) = serde_json::from_str::<DebugStartRequest>(body) {
                let session = Arc::new(Mutex::new(crate::debugger::DebugSession::new()));
                let cv = Arc::new((Mutex::new(false), Condvar::new()));
                session.lock().unwrap().pause_hook = Some(cv.clone());

                for bp in &req.breakpoints {
                    session.lock().unwrap().set_breakpoint("<script>", *bp);
                }
                if req.breakpoints.is_empty() {
                    session.lock().unwrap().step_mode = crate::debugger::DebugStepMode::StepIn;
                } else {
                    session.lock().unwrap().step_mode = crate::debugger::DebugStepMode::Continue;
                }

                let code_str = req.code.clone();
                let session_clone = session.clone();
                let cv_clone = cv.clone();

                let handle = std::thread::spawn(move || {
                    let mut lexer = crate::lexer::Lexer::new(&code_str);
                    if let Ok(tokens) = lexer.tokenize() {
                        let mut parser = crate::parser::Parser::new(tokens);
                        if let Ok(stmts) = parser.parse() {
                            let compiler = crate::compiler::Compiler::new();
                            if let Ok(chunk) = compiler.compile_program(&stmts) {
                                let mut vm = crate::vm::VM::new();
                                vm.current_file = Some(std::path::PathBuf::from("<script>"));
                                vm.debug_session = Some(session_clone.clone());
                                let _ = vm.interpret(chunk);
                            }
                        }
                    }
                    let mut s = session_clone.lock().unwrap();
                    s.is_terminated = true;
                    s.is_stopped = false;
                    let (lock, cvar) = &*cv_clone;
                    *lock.lock().unwrap() = true;
                    cvar.notify_all();
                });

                let start_wait = std::time::Instant::now();
                while start_wait.elapsed() < std::time::Duration::from_millis(500) {
                    {
                        let s = session.lock().unwrap();
                        if s.is_stopped || s.is_terminated {
                            break;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }

                let state_resp = {
                    let s = session.lock().unwrap();
                    build_debug_state_response(&s)
                };

                *get_active_debugger().lock().unwrap() = Some(ActiveIdeDebugger {
                    session,
                    pause_cv: cv,
                    handle: Some(handle),
                });

                let json = serde_json::to_string(&state_resp).unwrap();
                send_response(&mut stream, 200, "application/json", json.as_bytes())?;
            } else {
                send_response(
                    &mut stream,
                    400,
                    "application/json",
                    br#"{"error":"Invalid debug start payload"}"#,
                )?;
            }
        }
        ("POST", "/api/debug/step") => {
            let opt_dbg = get_active_debugger().lock().unwrap();
            if let Some(dbg) = opt_dbg.as_ref() {
                let mut s = dbg.session.lock().unwrap();
                if s.is_terminated {
                    let resp = build_debug_state_response(&s);
                    let json = serde_json::to_string(&resp).unwrap();
                    send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                } else {
                    let req: DebugStepRequest =
                        serde_json::from_str(body).unwrap_or(DebugStepRequest { mode: None });
                    let mode = req.mode.as_deref().unwrap_or("over");
                    match mode {
                        "into" => s.step_mode = crate::debugger::DebugStepMode::StepIn,
                        "out" => {
                            s.step_mode = crate::debugger::DebugStepMode::StepOut {
                                target_depth: s.snapshot_frames.len(),
                            }
                        }
                        _ => {
                            s.step_mode = crate::debugger::DebugStepMode::StepOver {
                                target_depth: s.snapshot_frames.len(),
                                start_line: s.last_stopped_line,
                            }
                        }
                    }
                    s.is_stopped = false;
                    s.stop_reason = None;
                    drop(s);
                    let (lock, cvar) = &*dbg.pause_cv;
                    *lock.lock().unwrap() = true;
                    cvar.notify_all();

                    let step_wait = std::time::Instant::now();
                    while step_wait.elapsed() < std::time::Duration::from_millis(500) {
                        {
                            let s2 = dbg.session.lock().unwrap();
                            if s2.is_stopped || s2.is_terminated {
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }

                    let s2 = dbg.session.lock().unwrap();
                    let resp = build_debug_state_response(&s2);
                    let json = serde_json::to_string(&resp).unwrap();
                    send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                }
            } else {
                send_response(
                    &mut stream,
                    400,
                    "application/json",
                    br#"{"error":"No active debug session"}"#,
                )?;
            }
        }
        ("POST", "/api/debug/continue") => {
            let opt_dbg = get_active_debugger().lock().unwrap();
            if let Some(dbg) = opt_dbg.as_ref() {
                let mut s = dbg.session.lock().unwrap();
                if s.is_terminated {
                    let resp = build_debug_state_response(&s);
                    let json = serde_json::to_string(&resp).unwrap();
                    send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                } else {
                    s.step_mode = crate::debugger::DebugStepMode::Continue;
                    s.is_stopped = false;
                    s.stop_reason = None;
                    drop(s);
                    let (lock, cvar) = &*dbg.pause_cv;
                    *lock.lock().unwrap() = true;
                    cvar.notify_all();

                    let cont_wait = std::time::Instant::now();
                    while cont_wait.elapsed() < std::time::Duration::from_millis(500) {
                        {
                            let s2 = dbg.session.lock().unwrap();
                            if s2.is_stopped || s2.is_terminated {
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }

                    let s2 = dbg.session.lock().unwrap();
                    let resp = build_debug_state_response(&s2);
                    let json = serde_json::to_string(&resp).unwrap();
                    send_response(&mut stream, 200, "application/json", json.as_bytes())?;
                }
            } else {
                send_response(
                    &mut stream,
                    400,
                    "application/json",
                    br#"{"error":"No active debug session"}"#,
                )?;
            }
        }
        ("GET", "/api/debug/state") => {
            let opt_dbg = get_active_debugger().lock().unwrap();
            if let Some(dbg) = opt_dbg.as_ref() {
                let s = dbg.session.lock().unwrap();
                let resp = build_debug_state_response(&s);
                let json = serde_json::to_string(&resp).unwrap();
                send_response(&mut stream, 200, "application/json", json.as_bytes())?;
            } else {
                let resp = DebugStateResponse {
                    active: false,
                    stopped: false,
                    reason: None,
                    current_line: 0,
                    stack_frames: Vec::new(),
                    variables: HashMap::new(),
                    is_terminated: true,
                };
                let json = serde_json::to_string(&resp).unwrap();
                send_response(&mut stream, 200, "application/json", json.as_bytes())?;
            }
        }
        ("POST", "/api/debug/stop") => {
            let mut opt_dbg = get_active_debugger().lock().unwrap();
            if let Some(dbg) = opt_dbg.take() {
                let mut s = dbg.session.lock().unwrap();
                s.is_terminated = true;
                s.step_mode = crate::debugger::DebugStepMode::Continue;
                drop(s);
                let (lock, cvar) = &*dbg.pause_cv;
                *lock.lock().unwrap() = true;
                cvar.notify_all();
            }
            send_response(
                &mut stream,
                200,
                "application/json",
                br#"{"status":"stopped"}"#,
            )?;
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

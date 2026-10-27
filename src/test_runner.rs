//! Shae Test Runner & Native Assertion Framework
//! (`src/test_runner.rs`, `shae test`)
//!
//! Provides automatic test discovery, test filtering, assertion failure diffing,
//! execution timing, stdout capture, and multi-engine test execution (VM and AST).

use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TestOptions {
    pub target: Option<String>,
    pub filter: Option<String>,
    pub use_vm: bool,
    pub nocapture: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed(String),
    Skipped,
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub file: String,
    pub status: TestStatus,
    pub duration: Duration,
    pub captured_output: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestRunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration: Duration,
    pub cases: Vec<TestCase>,
}

impl TestRunSummary {
    pub fn empty() -> Self {
        Self {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            total_duration: Duration::ZERO,
            cases: Vec::new(),
        }
    }
}

pub struct ActiveTestSession {
    pub filter: Option<String>,
    pub nocapture: bool,
    pub current_file: Option<String>,
    pub current_captured_stdout: String,
    pub cases: Vec<TestCase>,
}

static SESSIONS: LazyLock<Mutex<HashMap<ThreadId, ActiveTestSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn is_capturing() -> bool {
    let tid = std::thread::current().id();
    let lock = SESSIONS.lock().unwrap();
    lock.get(&tid).map(|s| !s.nocapture).unwrap_or(false)
}

pub fn capture_print(text: &str) {
    let tid = std::thread::current().id();
    let mut lock = SESSIONS.lock().unwrap();
    if let Some(session) = lock.get_mut(&tid) {
        if !session.current_captured_stdout.is_empty() {
            session.current_captured_stdout.push('\n');
        }
        session.current_captured_stdout.push_str(text);
    }
}

/// Discovers test files in the workspace or the given path.
pub fn discover_test_files(target: Option<&str>) -> Result<Vec<PathBuf>, String> {
    if let Some(path_str) = target {
        let p = Path::new(path_str);
        if !p.exists() {
            return Err(format!("Path '{}' not found", path_str));
        }
        if p.is_file() {
            return Ok(vec![p.to_path_buf()]);
        }
        let mut files = Vec::new();
        find_test_files_recursive(p, &mut files);
        files.sort();
        return Ok(files);
    }

    // Default discovery:
    // 1. If tests/shae exists and has tests, use that.
    let shae_dir = Path::new("tests/shae");
    if shae_dir.is_dir() {
        let mut files = Vec::new();
        find_test_files_recursive(shae_dir, &mut files);
        if !files.is_empty() {
            files.sort();
            return Ok(files);
        }
    }

    // 2. tests/
    let tests_dir = Path::new("tests");
    if tests_dir.is_dir() {
        let mut files = Vec::new();
        find_test_files_recursive(tests_dir, &mut files);
        if !files.is_empty() {
            files.sort();
            return Ok(files);
        }
    }

    // 3. current directory
    let mut files = Vec::new();
    find_test_files_recursive(Path::new("."), &mut files);
    files.sort();
    Ok(files)
}

fn find_test_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "target"
                    && name != ".git"
                    && name != ".packages"
                    && name != ".shae"
                    && name != "node_modules"
                    && name != "doc"
                {
                    find_test_files_recursive(&path, out);
                }
            } else if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if (file_name.ends_with("_test.shae")
                    || (file_name.starts_with("test_") && file_name.ends_with(".shae")))
                    && !out.contains(&path)
                {
                    out.push(path);
                }
            }
        }
    }
}

/// Executes a test case defined by `test(description, callable)`.
pub fn execute_test(
    desc: &str,
    callable: &Value,
    ev: &mut Evaluator,
    span: Span,
) -> Result<Value, RuntimeError> {
    let tid = std::thread::current().id();
    let (is_active, filter, nocapture, current_file) = {
        let lock = SESSIONS.lock().unwrap();
        if let Some(session) = lock.get(&tid) {
            (
                true,
                session.filter.clone(),
                session.nocapture,
                session.current_file.clone(),
            )
        } else {
            (false, None, true, None)
        }
    };

    if is_active {
        // Filter check: case-insensitive match on description or current file
        if let Some(ref pat) = filter {
            let pat_lower = pat.to_lowercase();
            let desc_lower = desc.to_lowercase();
            let file_matches = current_file
                .as_deref()
                .map(|f| f.to_lowercase().contains(&pat_lower))
                .unwrap_or(false);
            if !desc_lower.contains(&pat_lower) && !file_matches {
                let mut lock = SESSIONS.lock().unwrap();
                if let Some(session) = lock.get_mut(&tid) {
                    session.cases.push(TestCase {
                        name: desc.to_string(),
                        file: session.current_file.clone().unwrap_or_default(),
                        status: TestStatus::Skipped,
                        duration: Duration::ZERO,
                        captured_output: None,
                    });
                }
                println!("test {} ... [33mignored[0m", desc);
                return Ok(Value::Null);
            }
        }

        // Clear captured output buffer for this test
        if !nocapture {
            let mut lock = SESSIONS.lock().unwrap();
            if let Some(session) = lock.get_mut(&tid) {
                session.current_captured_stdout.clear();
            }
        }

        let start = Instant::now();
        let call_res = ev.call_value(callable, vec![], span);
        let result = match call_res {
            Ok(Value::Task(task_arc)) => {
                let mut task = task_arc.lock().unwrap();
                task.join()
            }
            other => other,
        };
        let duration = start.elapsed();

        let captured = {
            let lock = SESSIONS.lock().unwrap();
            lock.get(&tid)
                .map(|s| s.current_captured_stdout.clone())
                .unwrap_or_default()
        };

        let mut lock = SESSIONS.lock().unwrap();
        if let Some(session) = lock.get_mut(&tid) {
            match result {
                Ok(_) => {
                    session.cases.push(TestCase {
                        name: desc.to_string(),
                        file: session.current_file.clone().unwrap_or_default(),
                        status: TestStatus::Passed,
                        duration,
                        captured_output: if captured.is_empty() {
                            None
                        } else {
                            Some(captured)
                        },
                    });
                    println!(
                        "test {} ... [32mok[0m ({:.2}ms)",
                        desc,
                        duration.as_secs_f64() * 1000.0
                    );
                }
                Err(err) => {
                    let err_msg = err.message.clone();
                    session.cases.push(TestCase {
                        name: desc.to_string(),
                        file: session.current_file.clone().unwrap_or_default(),
                        status: TestStatus::Failed(err_msg.clone()),
                        duration,
                        captured_output: if captured.is_empty() {
                            None
                        } else {
                            Some(captured.clone())
                        },
                    });
                    println!(
                        "test {} ... [31mFAILED[0m ({:.2}ms)",
                        desc,
                        duration.as_secs_f64() * 1000.0
                    );
                    println!("    {}", err_msg);
                    if !nocapture && !captured.is_empty() {
                        println!("    ---- test stdout ----");
                        for line in captured.lines() {
                            println!("    {}", line);
                        }
                    }
                }
            }
        }
        // Return Ok(Value::Null) so other tests in the file continue executing
        Ok(Value::Null)
    } else {
        // Standalone run outside `shae test` CLI
        let start = Instant::now();
        let call_res = ev.call_value(callable, vec![], span);
        let result = match call_res {
            Ok(Value::Task(task_arc)) => {
                let mut task = task_arc.lock().unwrap();
                task.join()
            }
            other => other,
        };
        let duration = start.elapsed();
        match result {
            Ok(val) => {
                println!(
                    "test {} ... [32mok[0m ({:.2}ms)",
                    desc,
                    duration.as_secs_f64() * 1000.0
                );
                Ok(val)
            }
            Err(err) => {
                eprintln!(
                    "test {} ... [31mFAILED[0m ({:.2}ms)",
                    desc,
                    duration.as_secs_f64() * 1000.0
                );
                eprintln!("    {}", err.message);
                Err(err)
            }
        }
    }
}

/// Runs the complete test suite matching the specified options.
pub fn run_test_suite(options: &TestOptions) -> TestRunSummary {
    let test_files = match discover_test_files(options.target.as_deref()) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("Error discovering test files: {}", e);
            let mut summary = TestRunSummary::empty();
            summary.failed = 1;
            return summary;
        }
    };

    if test_files.is_empty() {
        let path_display = options.target.as_deref().unwrap_or("tests");
        println!("No test files (*_test.shae) found in '{}'.", path_display);
        return TestRunSummary::empty();
    }

    let tid = std::thread::current().id();
    {
        let mut lock = SESSIONS.lock().unwrap();
        lock.insert(
            tid,
            ActiveTestSession {
                filter: options.filter.clone(),
                nocapture: options.nocapture,
                current_file: None,
                current_captured_stdout: String::new(),
                cases: Vec::new(),
            },
        );
    }

    let engine_name = if options.use_vm {
        "Bytecode VM"
    } else {
        "Tree-Walking AST Evaluator"
    };
    println!(
        "
running {} test file(s) across {}
",
        test_files.len(),
        engine_name
    );

    let suite_start = Instant::now();

    for file_path in &test_files {
        let file_str = file_path.to_string_lossy().to_string();

        {
            let mut lock = SESSIONS.lock().unwrap();
            if let Some(session) = lock.get_mut(&tid) {
                session.current_file = Some(file_str.clone());
            }
        }

        let cases_before = {
            let lock = SESSIONS.lock().unwrap();
            lock.get(&tid).map(|s| s.cases.len()).unwrap_or(0)
        };

        let file_start = Instant::now();
        let run_res = if options.use_vm {
            crate::run_file_vm(file_path)
        } else {
            crate::run_file(file_path)
        };
        let file_duration = file_start.elapsed();

        let cases_after = {
            let lock = SESSIONS.lock().unwrap();
            lock.get(&tid).map(|s| s.cases.len()).unwrap_or(0)
        };

        let cases_added = cases_after.saturating_sub(cases_before);

        if cases_added == 0 {
            // Script-style test file (no `test(...)` calls)
            // Filter check:
            if let Some(ref pat) = options.filter {
                let pat_lower = pat.to_lowercase();
                if !file_str.to_lowercase().contains(&pat_lower) {
                    let mut lock = SESSIONS.lock().unwrap();
                    if let Some(session) = lock.get_mut(&tid) {
                        session.cases.push(TestCase {
                            name: file_str.clone(),
                            file: file_str.clone(),
                            status: TestStatus::Skipped,
                            duration: Duration::ZERO,
                            captured_output: None,
                        });
                    }
                    println!("test {} ... [33mignored[0m", file_str);
                    continue;
                }
            }

            let mut lock = SESSIONS.lock().unwrap();
            if let Some(session) = lock.get_mut(&tid) {
                match run_res {
                    Ok(_) => {
                        session.cases.push(TestCase {
                            name: file_str.clone(),
                            file: file_str.clone(),
                            status: TestStatus::Passed,
                            duration: file_duration,
                            captured_output: None,
                        });
                        println!(
                            "test {} ... [32mok[0m ({:.2}ms)",
                            file_str,
                            file_duration.as_secs_f64() * 1000.0
                        );
                    }
                    Err(err) => {
                        let err_msg = err.to_string();
                        session.cases.push(TestCase {
                            name: file_str.clone(),
                            file: file_str.clone(),
                            status: TestStatus::Failed(err_msg.clone()),
                            duration: file_duration,
                            captured_output: None,
                        });
                        println!(
                            "test {} ... [31mFAILED[0m ({:.2}ms)",
                            file_str,
                            file_duration.as_secs_f64() * 1000.0
                        );
                        println!("    {}", err_msg);
                    }
                }
            }
        } else {
            // Test file had `test(...)` blocks
            // If the whole file execution failed with an error outside test()
            if let Err(err) = run_res {
                let err_msg = err.to_string();
                let mut lock = SESSIONS.lock().unwrap();
                if let Some(session) = lock.get_mut(&tid) {
                    session.cases.push(TestCase {
                        name: format!("{} (file top-level)", file_str),
                        file: file_str.clone(),
                        status: TestStatus::Failed(err_msg.clone()),
                        duration: file_duration,
                        captured_output: None,
                    });
                }
                println!("test {} [file error] ... [31mFAILED[0m", file_str);
                println!("    {}", err_msg);
            }
        }
    }

    let total_duration = suite_start.elapsed();

    let cases = {
        let mut lock = SESSIONS.lock().unwrap();
        lock.remove(&tid).map(|s| s.cases).unwrap_or_default()
    };

    let total = cases.len();
    let passed = cases
        .iter()
        .filter(|c| matches!(c.status, TestStatus::Passed))
        .count();
    let failed = cases
        .iter()
        .filter(|c| matches!(c.status, TestStatus::Failed(_)))
        .count();
    let skipped = cases
        .iter()
        .filter(|c| matches!(c.status, TestStatus::Skipped))
        .count();

    let failures: Vec<&TestCase> = cases
        .iter()
        .filter(|c| matches!(c.status, TestStatus::Failed(_)))
        .collect();
    if !failures.is_empty() {
        println!(
            "
failures:
"
        );
        for case in &failures {
            println!("---- {} ----", case.name);
            if let TestStatus::Failed(ref msg) = case.status {
                for line in msg.lines() {
                    println!("    {}", line);
                }
            }
            if let Some(ref out) = case.captured_output {
                println!(
                    "
    [captured stdout]"
                );
                for line in out.lines() {
                    println!("    {}", line);
                }
            }
            println!();
        }
    }

    println!();
    if failed == 0 {
        println!(
            "test result: [32mok[0m. {} passed; {} failed; {} skipped; finished in {:.2}ms
",
            passed,
            failed,
            skipped,
            total_duration.as_secs_f64() * 1000.0
        );
    } else {
        println!(
            "test result: [31mFAILED[0m. {} passed; {} failed; {} skipped; finished in {:.2}ms
",
            passed,
            failed,
            skipped,
            total_duration.as_secs_f64() * 1000.0
        );
    }

    TestRunSummary {
        total,
        passed,
        failed,
        skipped,
        total_duration,
        cases,
    }
}

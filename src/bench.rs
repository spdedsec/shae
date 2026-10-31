//! Shae Benchmark Suite Runner (`shae bench`)
//!
//! Discovers and executes benchmark scripts under `benchmarks/*.shae`.
//! Supports name filtering, AST/VM execution engines, configurable iteration counts,
//! and outputs formatted performance metrics (iterations, total time, average latency, ops/sec).

use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub name: String,
    pub file: String,
    pub iterations: usize,
    pub total_duration: Duration,
    pub avg_latency: Duration,
    pub throughput: f64,
}

#[derive(Debug, Clone, Default)]
pub struct BenchOptions {
    pub target: Option<String>,
    pub filter: Option<String>,
    pub use_vm: bool,
    pub iterations_override: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct BenchSummary {
    pub total_benchmarks: usize,
    pub total_duration: Duration,
    pub results: Vec<BenchmarkResult>,
}

pub struct ActiveBenchSession {
    pub filter: Option<String>,
    pub current_file: Option<String>,
    pub iterations_override: Option<usize>,
    pub results: Vec<BenchmarkResult>,
    pub is_active: bool,
}

static BENCH_SESSIONS: LazyLock<Mutex<HashMap<ThreadId, ActiveBenchSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 0.001 {
        format!("{:.2} µs", secs * 1_000_000.0)
    } else if secs < 1.0 {
        format!("{:.2} ms", secs * 1000.0)
    } else {
        format!("{:.3} s", secs)
    }
}

pub fn format_latency(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{} ns/op", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.2} µs/op", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2} ms/op", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.3} s/op", nanos as f64 / 1_000_000_000.0)
    }
}

pub fn format_throughput(ops_sec: f64) -> String {
    if ops_sec >= 1_000_000.0 {
        format!("{:.2} M ops/s", ops_sec / 1_000_000.0)
    } else if ops_sec >= 1_000.0 {
        format!("{:.2} K ops/s", ops_sec / 1_000.0)
    } else {
        format!("{:.1} ops/s", ops_sec)
    }
}

pub fn execute_bench(
    name: &str,
    default_iters: usize,
    callable: &Value,
    ev: &mut Evaluator,
    span: Span,
) -> Result<Value, RuntimeError> {
    if !callable.is_callable() {
        return Err(RuntimeError::new(format!(
            "'bench' expects a callable function, got {}",
            callable.type_name()
        ))
        .at(span));
    }

    let tid = std::thread::current().id();
    let (filter, current_file, iters_override, is_active) = {
        let lock = BENCH_SESSIONS.lock().unwrap();
        if let Some(session) = lock.get(&tid) {
            (
                session.filter.clone(),
                session.current_file.clone(),
                session.iterations_override,
                session.is_active,
            )
        } else {
            (None, None, None, false)
        }
    };

    if let Some(ref pat) = filter {
        let pat_lower = pat.to_lowercase();
        let name_lower = name.to_lowercase();
        let file_matches = current_file
            .as_deref()
            .map(|f| f.to_lowercase().contains(&pat_lower))
            .unwrap_or(false);
        if !name_lower.contains(&pat_lower) && !file_matches {
            return Ok(Value::Null);
        }
    }

    let iters = iters_override.unwrap_or(default_iters).max(1);

    // Warmup run
    let _ = ev.call_value(callable, vec![], span)?;

    // Measurement
    let start = Instant::now();
    for _ in 0..iters {
        let _ = ev.call_value(callable, vec![], span)?;
    }
    let total_duration = start.elapsed();
    let avg_latency = total_duration / (iters as u32);
    let throughput = if total_duration.as_secs_f64() > 0.0 {
        iters as f64 / total_duration.as_secs_f64()
    } else {
        0.0
    };

    let result = BenchmarkResult {
        name: name.to_string(),
        file: current_file.clone().unwrap_or_else(|| "<standalone>".to_string()),
        iterations: iters,
        total_duration,
        avg_latency,
        throughput,
    };

    if is_active {
        let mut lock = BENCH_SESSIONS.lock().unwrap();
        if let Some(session) = lock.get_mut(&tid) {
            session.results.push(result.clone());
        }
    } else {
        println!(
            "  ✓ {:<42} {:>10} iters | {:>12} | {:>14} | {:>16}",
            name,
            iters,
            format_duration(total_duration),
            format_latency(avg_latency),
            format_throughput(throughput)
        );
    }

    let mut map = indexmap::IndexMap::new();
    map.insert("name".to_string(), Value::String(name.to_string()));
    map.insert("iterations".to_string(), Value::Int(iters as i64));
    map.insert(
        "total_ms".to_string(),
        Value::Float(total_duration.as_secs_f64() * 1000.0),
    );
    map.insert(
        "latency_us".to_string(),
        Value::Float(avg_latency.as_micros() as f64),
    );
    map.insert("throughput".to_string(), Value::Float(throughput));

    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

pub fn render_benchmark_table(summary: &BenchSummary, engine_label: &str) -> String {
    let mut out = String::new();
    out.push_str("========================================================================================================\n");
    out.push_str(&format!(
        "                               SHAE BENCHMARK SUITE ({})\n",
        engine_label
    ));
    out.push_str("========================================================================================================\n");
    out.push_str(&format!(
        "{:<44} {:>10} {:>14} {:>16} {:>16}\n",
        "BENCHMARK", "ITERATIONS", "TOTAL TIME", "AVG LATENCY", "THROUGHPUT"
    ));
    out.push_str(&"-".repeat(104));
    out.push('\n');

    let mut current_file = String::new();
    for r in &summary.results {
        if r.file != current_file {
            current_file = r.file.clone();
            out.push_str(&format!("Suite: {}\n", current_file));
        }
        let bench_display = if r.name.len() > 42 {
            format!("{}...", &r.name[..39])
        } else {
            r.name.clone()
        };

        out.push_str(&format!(
            "  {:<42} {:>10} {:>14} {:>16} {:>16}\n",
            bench_display,
            r.iterations,
            format_duration(r.total_duration),
            format_latency(r.avg_latency),
            format_throughput(r.throughput)
        ));
    }

    out.push_str(&"-".repeat(104));
    out.push('\n');
    let avg_throughput: f64 = if !summary.results.is_empty() {
        summary.results.iter().map(|r| r.throughput).sum::<f64>() / summary.results.len() as f64
    } else {
        0.0
    };
    out.push_str(&format!(
        "Summary: {} benchmarks executed in {} (Avg Throughput: {})\n",
        summary.total_benchmarks,
        format_duration(summary.total_duration),
        format_throughput(avg_throughput)
    ));
    out.push_str("========================================================================================================\n");
    out
}

pub fn discover_benchmark_files(target: Option<&str>) -> Result<Vec<PathBuf>, String> {
    if let Some(target_str) = target {
        let p = Path::new(target_str);
        if !p.exists() {
            let benchmarks_dir = Path::new("benchmarks");
            if benchmarks_dir.is_dir() {
                let mut files = Vec::new();
                find_benchmark_files_recursive(benchmarks_dir, &mut files);
                files.sort();
                return Ok(files);
            }
            return Err(format!("Benchmark target '{}' not found", target_str));
        }
        if p.is_file() {
            return Ok(vec![p.to_path_buf()]);
        }
        let mut files = Vec::new();
        find_benchmark_files_recursive(p, &mut files);
        files.sort();
        return Ok(files);
    }

    let benchmarks_dir = Path::new("benchmarks");
    if benchmarks_dir.is_dir() {
        let mut files = Vec::new();
        find_benchmark_files_recursive(benchmarks_dir, &mut files);
        files.sort();
        return Ok(files);
    }

    Ok(Vec::new())
}

fn find_benchmark_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') && name != "target" {
                    find_benchmark_files_recursive(&path, out);
                }
            } else if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".shae") && !out.contains(&path) {
                    out.push(path);
                }
            }
        }
    }
}

pub fn run_benchmarks(options: &BenchOptions) -> Result<BenchSummary, String> {
    let files = discover_benchmark_files(options.target.as_deref())?;
    if files.is_empty() {
        return Err("No benchmark files found under 'benchmarks/'".to_string());
    }

    let files_to_run: Vec<PathBuf> = if let Some(ref pat) = options.filter {
        let pat_lower = pat.to_lowercase();
        let matched: Vec<_> = files
            .iter()
            .filter(|p| p.to_string_lossy().to_lowercase().contains(&pat_lower))
            .cloned()
            .collect();
        if !matched.is_empty() {
            matched
        } else {
            files
        }
    } else {
        files
    };

    let total_start = Instant::now();
    let mut all_results = Vec::new();

    let engine_label = if options.use_vm {
        "Bytecode VM"
    } else {
        "Tree-Walking AST Evaluator"
    };

    let tid = std::thread::current().id();

    for file in &files_to_run {
        let file_str = file.to_string_lossy().to_string();

        {
            let mut lock = BENCH_SESSIONS.lock().unwrap();
            let session = lock.entry(tid).or_insert_with(|| ActiveBenchSession {
                filter: None,
                current_file: None,
                iterations_override: None,
                results: Vec::new(),
                is_active: false,
            });
            session.filter = options.filter.clone();
            session.current_file = Some(file_str.clone());
            session.iterations_override = options.iterations_override;
            session.results.clear();
            session.is_active = true;
        }

        let file_start = Instant::now();
        let res = crate::run_file_with_engine(file, options.use_vm);
        let file_duration = file_start.elapsed();

        let mut file_results = {
            let mut lock = BENCH_SESSIONS.lock().unwrap();
            if let Some(session) = lock.get_mut(&tid) {
                session.is_active = false;
                std::mem::take(&mut session.results)
            } else {
                Vec::new()
            }
        };

        match res {
            Ok(_) => {
                if file_results.is_empty() {
                    let name = file
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| file_str.clone());

                    let should_include = if let Some(ref pat) = options.filter {
                        let pat_lower = pat.to_lowercase();
                        name.to_lowercase().contains(&pat_lower)
                            || file_str.to_lowercase().contains(&pat_lower)
                    } else {
                        true
                    };

                    if should_include {
                        let iters = options.iterations_override.unwrap_or(1);
                        let avg_latency = file_duration / (iters as u32);
                        let throughput = if file_duration.as_secs_f64() > 0.0 {
                            iters as f64 / file_duration.as_secs_f64()
                        } else {
                            0.0
                        };
                        file_results.push(BenchmarkResult {
                            name,
                            file: file_str,
                            iterations: iters,
                            total_duration: file_duration,
                            avg_latency,
                            throughput,
                        });
                    }
                }
                all_results.extend(file_results);
            }
            Err(e) => {
                eprintln!("Error executing benchmark file '{}': {}", file_str, e);
            }
        }
    }

    {
        let mut lock = BENCH_SESSIONS.lock().unwrap();
        lock.remove(&tid);
    }

    let summary = BenchSummary {
        total_benchmarks: all_results.len(),
        total_duration: total_start.elapsed(),
        results: all_results,
    };

    print!("{}", render_benchmark_table(&summary, engine_label));
    Ok(summary)
}

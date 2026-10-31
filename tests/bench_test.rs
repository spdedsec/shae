// Rust Integration Test for Shae Benchmark Suite and Doc Server
// tests/bench_test.rs

use shae::bench::{self, BenchOptions};
use shae::doc;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_bench_discovery_and_execution_vm() {
    let options = BenchOptions {
        target: Some("benchmarks/fibonacci.shae".to_string()),
        filter: None,
        use_vm: true,
        iterations_override: Some(10),
    };

    let summary = bench::run_benchmarks(&options).expect("Benchmarks should run successfully");
    assert!(!summary.results.is_empty(), "Should have benchmark results");
    assert!(summary.total_duration > Duration::ZERO);
    for r in &summary.results {
        assert_eq!(r.iterations, 10);
        assert!(r.avg_latency > Duration::ZERO);
        assert!(r.throughput > 0.0);
    }
}

#[test]
fn test_bench_execution_ast() {
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let options = BenchOptions {
                target: Some("benchmarks/fibonacci.shae".to_string()),
                filter: Some("Iterative".to_string()),
                use_vm: false,
                iterations_override: Some(5),
            };

            let summary = bench::run_benchmarks(&options).expect("AST benchmark should run");
            assert_eq!(summary.results.len(), 1, "Filter should select only 1 benchmark");
            assert!(summary.results[0].name.contains("Iterative"));
            assert_eq!(summary.results[0].iterations, 5);
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn test_bench_filter_by_name() {
    let options = BenchOptions {
        target: Some("benchmarks/buffer_throughput.shae".to_string()),
        filter: Some("Zero-Copy".to_string()),
        use_vm: true,
        iterations_override: Some(20),
    };

    let summary = bench::run_benchmarks(&options).expect("Buffer benchmark filter should run");
    assert_eq!(summary.results.len(), 1);
    assert!(summary.results[0].name.contains("Zero-Copy"));
}

#[test]
fn test_doc_generation_and_live_server() {
    let out_dir = std::env::temp_dir().join("shae_doc_test_bench");
    let _ = fs::remove_dir_all(&out_dir);

    // Generate documentation
    let pkg_doc = doc::generate_docs(Path::new("."), &out_dir, Some("all"))
        .expect("Documentation generation should succeed");
    assert!(!pkg_doc.modules.is_empty(), "Should document modules");

    let index_file = out_dir.join("index.html");
    assert!(index_file.exists(), "index.html must be generated");

    // Test live HTTP server
    let shutdown = Arc::new(AtomicBool::new(false));
    let (addr, handle) = doc::start_doc_server(&out_dir, "127.0.0.1", 0, Some(shutdown.clone()))
        .expect("Doc server should bind to local port");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let url = format!("http://{}/index.html", addr);
    let resp = client.get(&url).send();
    assert!(resp.is_ok(), "HTTP GET request to doc server failed");
    let resp_body = resp.unwrap();
    assert_eq!(resp_body.status(), reqwest::StatusCode::OK);
    let text = resp_body.text().unwrap_or_default();
    assert!(text.contains("Shae Documentation") || text.contains("html"));

    // Graceful shutdown
    shutdown.store(true, Ordering::Relaxed);
    let _ = handle.join();
    let _ = fs::remove_dir_all(&out_dir);
}

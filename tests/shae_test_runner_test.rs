//! Shae Built-in Pure-Shae Test Runner & Native Assertion Framework Integration Tests
//! tests/shae_test_runner_test.rs

use shae::test_runner::{TestOptions, TestStatus, discover_test_files, run_test_suite};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn test_dual_engine_sample_suite_vm() {
    let options = TestOptions {
        target: Some("tests/shae/sample_suite_test.shae".to_string()),
        filter: None,
        use_vm: true,
        nocapture: true,
    };

    let summary = run_test_suite(&options);
    assert_eq!(
        summary.failed, 0,
        "Expected 0 failures in sample suite on VM"
    );
    assert_eq!(
        summary.passed, 6,
        "Expected 6 passed tests in sample suite on VM"
    );
    assert_eq!(summary.skipped, 0, "Expected 0 skipped tests");
}

#[test]
fn test_dual_engine_sample_suite_ast() {
    let options = TestOptions {
        target: Some("tests/shae/sample_suite_test.shae".to_string()),
        filter: None,
        use_vm: false,
        nocapture: true,
    };

    let summary = run_test_suite(&options);
    assert_eq!(
        summary.failed, 0,
        "Expected 0 failures in sample suite on AST"
    );
    assert_eq!(
        summary.passed, 6,
        "Expected 6 passed tests in sample suite on AST"
    );
    assert_eq!(summary.skipped, 0, "Expected 0 skipped tests");
}

#[test]
fn test_runner_filter_matching() {
    let options = TestOptions {
        target: Some("tests/shae/sample_suite_test.shae".to_string()),
        filter: Some("arithmetic".to_string()),
        use_vm: true,
        nocapture: true,
    };

    let summary = run_test_suite(&options);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.skipped, 5);

    // Filter matching nothing
    let none_options = TestOptions {
        target: Some("tests/shae/sample_suite_test.shae".to_string()),
        filter: Some("xyz_nonexistent_pattern_123".to_string()),
        use_vm: true,
        nocapture: true,
    };
    let none_summary = run_test_suite(&none_options);
    assert_eq!(none_summary.failed, 0);
    assert_eq!(none_summary.passed, 0);
    assert_eq!(none_summary.skipped, 6);
}

#[test]
fn test_runner_discovery() {
    let files = discover_test_files(Some("tests/shae")).expect("Failed to discover tests");
    assert!(
        files.len() >= 10,
        "Expected at least 10 test files in tests/shae, found {}",
        files.len()
    );
    assert!(
        files.iter().any(|f| f.ends_with("sample_suite_test.shae")),
        "sample_suite_test.shae not discovered"
    );
    assert!(
        files.iter().any(|f| f.ends_with("math_test.shae")),
        "math_test.shae not discovered"
    );
}

#[test]
fn test_assertion_failure_reporting_and_diff() {
    let temp_dir = std::env::temp_dir().join("shae_test_runner_fail_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let fail_file = temp_dir.join("failing_case_test.shae");
    let script = r#"
use { test, assert_eq } from "std:test"

test("should pass", fn() {
    assert_eq(1 + 1, 2);
});

test("should fail with diff", fn() {
    assert_eq(100, 200, "Custom diff message");
});
"#;
    fs::write(&fail_file, script).unwrap();

    let options = TestOptions {
        target: Some(fail_file.to_string_lossy().to_string()),
        filter: None,
        use_vm: true,
        nocapture: true,
    };

    let summary = run_test_suite(&options);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 1);

    let failed_case = summary
        .cases
        .iter()
        .find(|c| matches!(c.status, TestStatus::Failed(_)))
        .expect("Failed case not found");
    if let TestStatus::Failed(ref err) = failed_case.status {
        assert!(
            err.contains("expected 200, but got 100"),
            "Expected diff in error, got: {}",
            err
        );
        assert!(
            err.contains("Custom diff message"),
            "Expected message in error, got: {}",
            err
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_native_assertion_framework_primitives() {
    // 1. assert_ne
    let script_ne_ok = r#"
use { assert_ne } from "std:test"
assert_ne("hello", "world");
assert_ne(1, 2);
"#;
    assert!(shae::run_vm(script_ne_ok).is_ok());

    let script_ne_fail = r#"
use { assert_ne } from "std:test"
assert_ne(42, 42, "Values must not match");
"#;
    let res = shae::run_vm(script_ne_fail);
    assert!(res.is_err());
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("expected values to differ"));
    assert!(err_str.contains("Values must not match"));

    // 2. assert_throws
    let script_throws_ok = r#"
use { assert_throws } from "std:test"
assert_throws(fn() {
    panic("intentional");
});
"#;
    assert!(shae::run_vm(script_throws_ok).is_ok());

    let script_throws_fail = r#"
use { assert_throws } from "std:test"
assert_throws(fn() {
    return 42;
}, "Must throw error");
"#;
    let res_throws = shae::run_vm(script_throws_fail);
    assert!(res_throws.is_err());
    let err_str = res_throws.unwrap_err().to_string();
    assert!(err_str.contains("expected callable to throw"));
    assert!(err_str.contains("returned 42"));
}

#[test]
fn test_live_documentation_server() {
    let temp_dir = std::env::temp_dir().join("shae_live_doc_server_test");
    let out_dir = temp_dir.join("doc_out");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(temp_dir.join("src")).unwrap();

    let manifest = r#"[package]
name = "doc_demo"
version = "1.0.0"
entry = "src/lib.shae"
"#;
    fs::write(temp_dir.join("shae.toml"), manifest).unwrap();

    let code = r#"//! Live doc demo module.

/// Returns the doubled value of x.
fn double(x) {
    return x * 2;
}
"#;
    fs::write(temp_dir.join("src/lib.shae"), code).unwrap();

    // Generate docs
    let pkg = shae::doc::generate_docs(&temp_dir, &out_dir, Some("all"))
        .expect("Failed to generate documentation");
    assert_eq!(pkg.package_name, "doc_demo");

    // Start live server on port 0
    let shutdown = Arc::new(AtomicBool::new(false));
    let (addr, handle) =
        shae::doc::start_doc_server(&out_dir, "127.0.0.1", 0, Some(shutdown.clone()))
            .expect("Failed to start live doc server");

    // Make HTTP request using reqwest
    let url = format!("http://{}:{}/index.html", addr.ip(), addr.port());
    let resp = reqwest::blocking::get(&url).expect("Failed to fetch from live doc server");
    assert_eq!(resp.status().as_u16(), 200);

    let body = resp.text().expect("Failed to read response body");
    assert!(body.contains("doc_demo v1.0.0"));
    assert!(body.contains("double"));

    // Also test root URL (which serves index.html)
    let root_url = format!("http://{}:{}/", addr.ip(), addr.port());
    let root_resp = reqwest::blocking::get(&root_url).expect("Failed to fetch root URL");
    assert_eq!(root_resp.status().as_u16(), 200);

    // Stop server
    shutdown.store(true, Ordering::Relaxed);
    // Connect to unblock accept loop if waiting
    let _ = std::net::TcpStream::connect(addr);
    let _ = handle.join();

    let _ = fs::remove_dir_all(&temp_dir);
}

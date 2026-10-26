//! Milestone 4: Standard Documentation Engine (shae doc) Integration Test Suite
//! tests/doc_gen_test.rs

use shae::doc::{DocItemKind, ModuleDoc, PackageDoc, generate_docs};
use std::fs;

#[test]
fn test_doc_extraction_single_module() {
    let source = r#"//! Math and utilities module for Shae.
//! Provides geometric and arithmetic operations.

/// Computes the factorial of a non-negative integer n.
fn factorial(n) {
    if n <= 1 { return 1; }
    return n * factorial(n - 1);
}

/// Asynchronously fetches resource from a remote URL.
async fn fetch_remote(url, timeout_ms) {
    return "ok";
}

/// 2D Point structure in coordinate space.
struct Point {
    x: num,
    y: num,
}

/// Color enumeration for terminal rendering.
enum Color {
    Red,
    Green,
    Blue,
}

/// Maximum allowed cache buffer size in bytes.
const MAX_BUFFER = 4096;
"#;

    let module_doc = ModuleDoc::extract(source, "math_utils", "src/math.shae");

    assert_eq!(module_doc.name, "math_utils");
    assert_eq!(module_doc.path, "src/math.shae");
    assert!(
        module_doc.description.contains("Math and utilities module"),
        "Module doc missing description: {}",
        module_doc.description
    );
    assert_eq!(module_doc.items.len(), 5);

    // 1. Function
    let fact_item = module_doc
        .items
        .iter()
        .find(|i| i.name == "factorial")
        .expect("factorial not found");
    assert!(matches!(fact_item.kind, DocItemKind::Function));
    assert_eq!(fact_item.signature, "fn factorial(n)");
    assert_eq!(fact_item.params, vec!["n"]);
    assert!(fact_item.description.contains("Computes the factorial"));

    // 2. Async Function
    let fetch_item = module_doc
        .items
        .iter()
        .find(|i| i.name == "fetch_remote")
        .expect("fetch_remote not found");
    assert!(matches!(fetch_item.kind, DocItemKind::AsyncFunction));
    assert_eq!(
        fetch_item.signature,
        "async fn fetch_remote(url, timeout_ms)"
    );
    assert_eq!(fetch_item.params, vec!["url", "timeout_ms"]);
    assert!(
        fetch_item
            .description
            .contains("Asynchronously fetches resource")
    );

    // 3. Struct
    let struct_item = module_doc
        .items
        .iter()
        .find(|i| i.name == "Point")
        .expect("Point not found");
    assert!(matches!(struct_item.kind, DocItemKind::Struct));
    assert_eq!(struct_item.fields, vec!["x: num", "y: num"]);
    assert!(struct_item.description.contains("2D Point structure"));

    // 4. Enum
    let enum_item = module_doc
        .items
        .iter()
        .find(|i| i.name == "Color")
        .expect("Color not found");
    assert!(matches!(enum_item.kind, DocItemKind::Enum));
    assert_eq!(enum_item.variants, vec!["Red", "Green", "Blue"]);
    assert!(enum_item.description.contains("Color enumeration"));

    // 5. Constant
    let const_item = module_doc
        .items
        .iter()
        .find(|i| i.name == "MAX_BUFFER")
        .expect("MAX_BUFFER not found");
    assert!(matches!(const_item.kind, DocItemKind::Constant));
    assert!(
        const_item
            .description
            .contains("Maximum allowed cache buffer")
    );
}

#[test]
fn test_markdown_and_html_rendering() {
    let source = r#"//! Core network transport layer.

/// Connects to a cluster node over TLS.
async fn connect_tls(addr, cert_path) {
    return true;
}

/// Node configuration container.
struct ClusterConfig {
    port: num,
    use_tls: bool,
}
"#;

    let module_doc = ModuleDoc::extract(source, "network", "src/network.shae");
    let pkg_doc = PackageDoc {
        package_name: "shae-mesh".to_string(),
        version: "1.2.0".to_string(),
        description: "Decentralized mesh networking for Shae".to_string(),
        modules: vec![module_doc],
    };

    // Test Markdown
    let md = pkg_doc.to_markdown();
    assert!(
        md.contains("# shae-mesh `v1.2.0`"),
        "Markdown missing header"
    );
    assert!(
        md.contains("Decentralized mesh networking"),
        "Markdown missing description"
    );
    assert!(
        md.contains("async fn connect_tls(addr, cert_path)"),
        "Markdown missing async fn"
    );
    assert!(
        md.contains("struct ClusterConfig"),
        "Markdown missing struct"
    );
    assert!(md.contains("```shae"), "Markdown missing code block");

    // Test HTML
    let html = pkg_doc.to_html();
    assert!(html.contains("<!DOCTYPE html>"), "HTML missing doctype");
    assert!(
        html.contains("<title>shae-mesh v1.2.0 — Shae Documentation</title>"),
        "HTML missing title"
    );
    assert!(html.contains("shae-mesh"), "HTML missing package name");
    assert!(html.contains("badge-async"), "HTML missing async badge");
    assert!(html.contains("badge-struct"), "HTML missing struct badge");
    assert!(
        html.contains("id=\"doc-search\""),
        "HTML missing search input"
    );
    assert!(
        html.contains("connect_tls"),
        "HTML missing connect_tls symbol"
    );
}

#[test]
fn test_end_to_end_project_doc_generation() {
    let temp_dir = std::env::temp_dir().join("shae_doc_test_project");
    let out_dir = temp_dir.join("docs_out");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(temp_dir.join("src")).expect("Failed to create test project dir");

    // Create shae.toml
    let manifest_content = r#"[package]
name = "math_calc"
version = "0.5.0"
entry = "src/main.shae"
description = "High precision calculator library"
authors = ["Shae Devs"]
"#;
    fs::write(temp_dir.join("shae.toml"), manifest_content).expect("Failed to write manifest");

    // Create src/main.shae
    let main_shae = r#"//! Main calculator entrypoint.

/// Evaluates mathematical expression.
fn evaluate(expr) {
    return 42;
}
"#;
    fs::write(temp_dir.join("src/main.shae"), main_shae).expect("Failed to write main.shae");

    // Create src/utils.shae
    let utils_shae = r#"//! Utility helper functions.

/// Formats number as currency string.
fn format_currency(amount) {
    return "$" + to_string(amount);
}
"#;
    fs::write(temp_dir.join("src/utils.shae"), utils_shae).expect("Failed to write utils.shae");

    // Run generate_docs with "all" format
    let res = generate_docs(&temp_dir, &out_dir, Some("all"));
    assert!(res.is_ok(), "generate_docs failed: {:?}", res);

    let pkg = res.unwrap();
    assert_eq!(pkg.package_name, "math_calc");
    assert_eq!(pkg.version, "0.5.0");
    assert_eq!(pkg.description, "High precision calculator library");
    assert_eq!(pkg.modules.len(), 2);

    // Verify written files
    assert!(
        out_dir.join("index.html").exists(),
        "index.html was not generated"
    );
    assert!(
        out_dir.join("README.md").exists(),
        "README.md was not generated"
    );
    assert!(
        out_dir.join("doc.json").exists(),
        "doc.json was not generated"
    );

    let html_content = fs::read_to_string(out_dir.join("index.html")).unwrap();
    assert!(html_content.contains("math_calc v0.5.0 — Shae Documentation"));
    assert!(html_content.contains("evaluate"));
    assert!(html_content.contains("format_currency"));

    let json_content = fs::read_to_string(out_dir.join("doc.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_content).unwrap();
    assert_eq!(parsed["package_name"], "math_calc");
    assert_eq!(parsed["modules"].as_array().unwrap().len(), 2);

    // Clean up
    let _ = fs::remove_dir_all(&temp_dir);
}

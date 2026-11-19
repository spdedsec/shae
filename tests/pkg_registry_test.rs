use serde_json::json;
use shae::pkg::{
    self, compute_sha256, resolve_package_file, resolve_registry_url, Lockfile, PkgError,
    ShaepkgArchive, ShaepkgFiles, SHAEPKG_MAGIC,
};
use shae::run_file;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir()
            .join(format!("shae_test_reg_{}_{}", name, std::process::id()));
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

struct MockRegistry {
    url: String,
    routes: Arc<Mutex<BTreeMap<String, (u16, String, Vec<u8>)>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockRegistry {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp listener");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);

        let routes: Arc<Mutex<BTreeMap<String, (u16, String, Vec<u8>)>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let routes_clone = Arc::clone(&routes);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);

        listener
            .set_nonblocking(true)
            .expect("set nonblocking on listener");

        let handle = thread::spawn(move || {
            while !shutdown_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0u8; 4096];
                        let n = stream.read(&mut buf).unwrap_or(0);
                        let req_text = String::from_utf8_lossy(&buf[..n]);

                        let mut path = "/";
                        if let Some(first_line) = req_text.lines().next() {
                            let parts: Vec<&str> = first_line.split_whitespace().collect();
                            if parts.len() >= 2 {
                                path = parts[1];
                            }
                        }

                        let clean_path = path.split('?').next().unwrap_or(path);

                        let routes_lock = routes_clone.lock().unwrap();
                        if let Some((status, content_type, body)) = routes_lock.get(clean_path) {
                            let resp = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                status,
                                content_type,
                                body.len()
                            );
                            let _ = stream.write_all(resp.as_bytes());
                            let _ = stream.write_all(body);
                        } else {
                            let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                            let _ = stream.write_all(not_found.as_bytes());
                        }
                        let _ = stream.flush();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            url,
            routes,
            shutdown,
            handle: Some(handle),
        }
    }

    fn register_json(&self, path: &str, json_content: &str) {
        self.routes.lock().unwrap().insert(
            path.to_string(),
            (200, "application/json".to_string(), json_content.as_bytes().to_vec()),
        );
    }

    fn register_bytes(&self, path: &str, content_type: &str, bytes: Vec<u8>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), (200, content_type.to_string(), bytes));
    }
}

impl Drop for MockRegistry {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn test_remote_index_resolution_download_sha256_and_lockfile() {
    let mock = MockRegistry::new();
    let temp_root = TempDir::new("happy_path");
    let math_lib_dir = temp_root.path.join("math_lib");
    let app_dir = temp_root.path.join("consumer_app");

    // 1. Build a library package `math`
    let _ = pkg::init_project(&math_lib_dir, Some("math")).unwrap();
    let math_code = r#"
fn add(a, b) {
    a + b
}

fn multiply(a, b) {
    a * b
}
"#;
    fs::write(math_lib_dir.join("main.shae"), math_code).unwrap();

    let utils_code = r#"
fn square(x) {
    x * x
}
"#;
    fs::create_dir_all(math_lib_dir.join("src")).unwrap();
    fs::write(math_lib_dir.join("src/utils.shae"), utils_code).unwrap();

    // 2. Pack the library into .shaepkg archive and compute SHA-256
    let archive_bytes = pkg::create_shaepkg(&math_lib_dir).expect("pack math package");
    let expected_checksum = compute_sha256(&archive_bytes);

    // 3. Register on mock registry
    let index_json = json!({
        "name": "math",
        "latest": "1.0.0",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "download_url": "/downloads/math-1.0.0.shaepkg",
                "checksum": expected_checksum,
                "dependencies": {}
            }
        }
    })
    .to_string();

    mock.register_json("/packages/math.json", &index_json);
    mock.register_bytes(
        "/downloads/math-1.0.0.shaepkg",
        "application/octet-stream",
        archive_bytes,
    );

    // 4. Init consumer application and add dependency from registry
    let _ = pkg::init_project(&app_dir, Some("consumer_app")).unwrap();
    let lockfile = pkg::add_dependency_from_registry(
        &app_dir,
        "math",
        Some("1.0.0"),
        &mock.url,
    )
    .expect("add_dependency_from_registry succeeded");

    // 5. Verify .packages/math/ was created and populated
    let installed_main = app_dir.join(".packages/math/main.shae");
    let installed_utils = app_dir.join(".packages/math/src/utils.shae");
    assert!(installed_main.exists(), ".packages/math/main.shae should exist");
    assert!(installed_utils.exists(), ".packages/math/src/utils.shae should exist");

    // 6. Verify shae.lock was updated with checksum and registry source
    assert!(lockfile.packages.contains_key("math"));
    let locked_math = &lockfile.packages["math"];
    assert_eq!(locked_math.version.as_deref(), Some("1.0.0"));
    assert_eq!(locked_math.source, format!("registry+{}", mock.url));
    assert_eq!(locked_math.checksum.as_deref(), Some(expected_checksum.as_str()));

    // Verify lockfile on disk
    let disk_lock = Lockfile::load_from_dir(&app_dir)
        .unwrap()
        .expect("shae.lock must exist on disk");
    assert_eq!(
        disk_lock.packages["math"].checksum.as_deref(),
        Some(expected_checksum.as_str())
    );

    // 7. Verify Shae script can import from .packages and execute
    let consumer_script = r#"
use { add, multiply } from "math"
use { square } from "math/utils"

let sum = add(10, 20)
let product = multiply(sum, 2)
let sq = square(5)

[sum, product, sq]
"#;
    let app_main = app_dir.join("main.shae");
    fs::write(&app_main, consumer_script).unwrap();

    let result = run_file(&app_main).expect("run script importing from .packages");
    assert_eq!(result.to_json(), json!([30, 60, 25]));
}

#[test]
fn test_checksum_mismatch_rejection() {
    let mock = MockRegistry::new();
    let temp_root = TempDir::new("mismatch_reject");
    let app_dir = temp_root.path.join("app");

    // 1. Pack a package
    let lib_dir = temp_root.path.join("tampered_lib");
    let _ = pkg::init_project(&lib_dir, Some("tampered")).unwrap();
    let archive_bytes = pkg::create_shaepkg(&lib_dir).unwrap();

    // 2. Put a fraudulent checksum in the index
    let fake_checksum = "0000000000000000000000000000000000000000000000000000000000000000";
    let index_json = json!({
        "name": "tampered",
        "latest": "1.0.0",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "download_url": "/downloads/tampered.shaepkg",
                "checksum": fake_checksum,
                "dependencies": {}
            }
        }
    })
    .to_string();

    mock.register_json("/packages/tampered.json", &index_json);
    mock.register_bytes(
        "/downloads/tampered.shaepkg",
        "application/octet-stream",
        archive_bytes,
    );

    let _ = pkg::init_project(&app_dir, Some("app")).unwrap();
    let res = pkg::add_dependency_from_registry(&app_dir, "tampered", None, &mock.url);

    // 3. Assert ChecksumMismatch error is returned
    match res {
        Err(PkgError::ChecksumMismatch(pkg_name, expected, computed)) => {
            assert_eq!(pkg_name, "tampered");
            assert_eq!(expected, fake_checksum);
            assert_ne!(computed, fake_checksum);
        }
        other => panic!("Expected ChecksumMismatch error, got {:?}", other),
    }

    // 4. Assert extraction was aborted: .packages/tampered must NOT exist
    let tampered_pkg_dir = app_dir.join(".packages/tampered");
    assert!(
        !tampered_pkg_dir.exists(),
        "Tampered archive must not be extracted to disk"
    );
}

#[test]
fn test_archive_path_traversal_defense() {
    let mock = MockRegistry::new();
    let temp_root = TempDir::new("traversal_defense");
    let app_dir = temp_root.path.join("app");
    let _ = pkg::init_project(&app_dir, Some("app")).unwrap();

    // Construct a malicious .shaepkg archive with path traversal
    let mut files = BTreeMap::new();
    files.insert("../../outside_evil.txt".to_string(), "malicious code".to_string());
    files.insert("main.shae".to_string(), "print(1)".to_string());

    let bad_archive = ShaepkgArchive {
        name: "evil_pkg".to_string(),
        version: "1.0.0".to_string(),
        files: ShaepkgFiles::Map(files),
    };
    let json_bytes = serde_json::to_vec(&bad_archive).unwrap();
    let mut bad_payload = Vec::new();
    bad_payload.extend_from_slice(SHAEPKG_MAGIC);
    bad_payload.extend_from_slice(&json_bytes);

    let checksum = compute_sha256(&bad_payload);
    let index_json = json!({
        "name": "evil_pkg",
        "latest": "1.0.0",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "download_url": "/downloads/evil.shaepkg",
                "checksum": checksum,
                "dependencies": {}
            }
        }
    })
    .to_string();

    mock.register_json("/packages/evil_pkg.json", &index_json);
    mock.register_bytes("/downloads/evil.shaepkg", "application/octet-stream", bad_payload);

    let res = pkg::add_dependency_from_registry(&app_dir, "evil_pkg", None, &mock.url);

    // Assert traversal attack is caught by Archive error
    match res {
        Err(PkgError::Archive(msg)) => {
            assert!(
                msg.contains("Path traversal attack") || msg.contains(".."),
                "Archive error should mention traversal: {}",
                msg
            );
        }
        other => panic!("Expected PkgError::Archive for traversal attack, got {:?}", other),
    }

    // Verify escaped file does not exist
    let escaped_file = temp_root.path.join("outside_evil.txt");
    assert!(
        !escaped_file.exists(),
        "Path traversal file must never be written outside destination"
    );
}

#[test]
fn test_registry_endpoint_resolution_precedence() {
    let temp_root = TempDir::new("precedence");
    let app_dir = temp_root.path.join("app");
    fs::create_dir_all(&app_dir).unwrap();

    // 1. Fallback default
    let default_url = resolve_registry_url(Some(&app_dir), None);
    assert_eq!(default_url, "https://registry.shae-lang.org");

    // 2. shae.toml [registry] default = "..."
    let manifest_toml = r#"
[package]
name = "app"
version = "0.1.0"
entry = "main.shae"

[registry]
default = "http://manifest.registry.org"
"#;
    fs::write(app_dir.join("shae.toml"), manifest_toml).unwrap();
    let manifest_url = resolve_registry_url(Some(&app_dir), None);
    assert_eq!(manifest_url, "http://manifest.registry.org");

    // 3. SHAE_REGISTRY_URL environment variable overrides manifest
    unsafe {
        std::env::set_var("SHAE_REGISTRY_URL", "http://env.registry.org");
    }
    let env_url = resolve_registry_url(Some(&app_dir), None);
    assert_eq!(env_url, "http://env.registry.org");

    // 4. CLI flag overrides everything
    let cli_url = resolve_registry_url(Some(&app_dir), Some("http://cli.registry.org"));
    assert_eq!(cli_url, "http://cli.registry.org");

    // Clean up env var
    unsafe {
        std::env::remove_var("SHAE_REGISTRY_URL");
    }
}

#[test]
fn test_resolve_package_file_precedence_and_fallback() {
    let temp_root = TempDir::new("resolution_precedence");
    let app_dir = temp_root.path.join("app");
    fs::create_dir_all(&app_dir).unwrap();

    // 1. Put different versions in .packages/mylib and .shae/packages/mylib
    let pkg_new_dir = app_dir.join(".packages/mylib");
    let pkg_old_dir = app_dir.join(".shae/packages/mylib");

    fs::create_dir_all(&pkg_new_dir).unwrap();
    fs::create_dir_all(&pkg_old_dir).unwrap();

    fs::write(pkg_new_dir.join("main.shae"), "fn get_version() { \"new\" }").unwrap();
    fs::write(pkg_old_dir.join("main.shae"), "fn get_version() { \"old\" }").unwrap();

    // 2. resolve_package_file should prioritize .packages
    let resolved = resolve_package_file(&app_dir, "mylib").expect("must resolve mylib");
    assert_eq!(resolved, pkg_new_dir.join("main.shae"));

    // 3. Delete .packages/mylib and verify fallback to .shae/packages/mylib
    fs::remove_dir_all(&pkg_new_dir).unwrap();
    let resolved_fallback = resolve_package_file(&app_dir, "mylib").expect("must resolve fallback");
    assert_eq!(resolved_fallback, pkg_old_dir.join("main.shae"));
}

#[test]
fn test_reinstall_from_lockfile() {
    let mock = MockRegistry::new();
    let temp_root = TempDir::new("reinstall");
    let app_dir = temp_root.path.join("app");
    let lib_dir = temp_root.path.join("lib");

    let _ = pkg::init_project(&lib_dir, Some("greeter")).unwrap();
    fs::write(lib_dir.join("main.shae"), "fn greet(name) {\n    \"Hello, \" + name\n}").unwrap();

    let archive_bytes = pkg::create_shaepkg(&lib_dir).unwrap();
    let checksum = compute_sha256(&archive_bytes);

    let index_json = json!({
        "name": "greeter",
        "latest": "1.0.0",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "download_url": "/downloads/greeter.shaepkg",
                "checksum": checksum,
                "dependencies": {}
            }
        }
    })
    .to_string();

    mock.register_json("/packages/greeter.json", &index_json);
    mock.register_bytes("/downloads/greeter.shaepkg", "application/octet-stream", archive_bytes);

    let _ = pkg::init_project(&app_dir, Some("app")).unwrap();
    pkg::add_dependency_from_registry(&app_dir, "greeter", Some("1.0.0"), &mock.url).unwrap();

    // Verify it is installed
    assert!(app_dir.join(".packages/greeter/main.shae").exists());

    // Wipe out .packages/ directory
    fs::remove_dir_all(app_dir.join(".packages")).unwrap();
    assert!(!app_dir.join(".packages/greeter").exists());

    // Reinstall using install_dependencies
    let reloaded_lock = pkg::install_dependencies(&app_dir).expect("reinstall dependencies");
    assert!(reloaded_lock.packages.contains_key("greeter"));
    assert!(app_dir.join(".packages/greeter/main.shae").exists());

    // Verify execution
    let app_code = "use { greet } from \"greeter\"\ngreet(\"World\")";
    let app_main = app_dir.join("main.shae");
    fs::write(&app_main, app_code).unwrap();
    let result = run_file(&app_main).unwrap();
    assert_eq!(result.to_json(), json!("Hello, World"));
}

#[test]
fn test_cli_add_syntax_variations() {
    let mock = MockRegistry::new();
    let temp_root = TempDir::new("cli_syntax");
    let app_dir = temp_root.path.join("app");
    let lib_dir = temp_root.path.join("calc");

    let _ = pkg::init_project(&lib_dir, Some("calc")).unwrap();
    fs::write(lib_dir.join("main.shae"), "fn double(x) { x * 2 }").unwrap();

    let archive_bytes = pkg::create_shaepkg(&lib_dir).unwrap();
    let checksum = compute_sha256(&archive_bytes);

    let index_json = json!({
        "name": "calc",
        "latest": "2.0.0",
        "versions": {
            "2.0.0": {
                "version": "2.0.0",
                "download_url": "/downloads/calc-2.0.0.shaepkg",
                "checksum": checksum,
                "dependencies": {}
            }
        }
    })
    .to_string();

    mock.register_json("/packages/calc.json", &index_json);
    mock.register_bytes("/downloads/calc-2.0.0.shaepkg", "application/octet-stream", archive_bytes);

    let _ = pkg::init_project(&app_dir, Some("app")).unwrap();

    // Test shae pkg add calc@2.0.0 --registry <url>
    let binary = env!("CARGO_BIN_EXE_shae");
    let output = Command::new(binary)
        .current_dir(&app_dir)
        .args(["pkg", "add", "calc@2.0.0", "--registry", &mock.url])
        .output()
        .expect("run shae pkg add");

    assert!(
        output.status.success(),
        "shae pkg add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(app_dir.join(".packages/calc/main.shae").exists());

    // Test top-level shae add with --version flag
    let output2 = Command::new(binary)
        .current_dir(&app_dir)
        .args(["add", "calc", "--version", "2.0.0", "--registry", &mock.url])
        .output()
        .expect("run shae add");

    assert!(
        output2.status.success(),
        "shae add failed: {}",
        String::from_utf8_lossy(&output2.stderr)
    );
}

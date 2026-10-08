use shae::pkg::{self, PackageManifest, Lockfile};
use shae::run_file;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("shae_test_pkg_{}_{}", name, std::process::id()));
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

#[test]
fn test_pkg_init_manifest_and_structure() {
    let temp = TempDir::new("init");
    let manifest = pkg::init_project(&temp.path, Some("my_cool_app")).expect("init_project failed");

    assert_eq!(manifest.package.name, "my_cool_app");
    assert_eq!(manifest.package.version, "0.1.0");
    assert_eq!(manifest.package.entry, "main.shae");

    // Check files on disk
    let toml_path = temp.path.join("shae.toml");
    let main_path = temp.path.join("main.shae");
    let gitignore_path = temp.path.join(".gitignore");

    assert!(toml_path.exists());
    assert!(main_path.exists());
    assert!(gitignore_path.exists());

    let loaded = PackageManifest::load_from_dir(&temp.path).expect("load manifest failed");
    assert_eq!(loaded.package.name, "my_cool_app");
}

#[test]
fn test_pkg_path_dependency_and_module_resolution() {
    let temp_root = TempDir::new("path_dep");
    let lib_dir = temp_root.path.join("calculator");
    let app_dir = temp_root.path.join("app");

    // 1. Create library package
    let _ = pkg::init_project(&lib_dir, Some("calculator")).unwrap();
    let lib_code = r#"
fn add(a, b) {
    a + b
}

fn multiply(a, b) {
    a * b
}
"#;
    fs::write(lib_dir.join("main.shae"), lib_code).unwrap();

    // 2. Create app package
    let _ = pkg::init_project(&app_dir, Some("app")).unwrap();

    // 3. Add path dependency
    let lockfile = pkg::add_dependency(&app_dir, "calculator", "../calculator", None, None, None)
        .expect("add dependency failed");

    assert!(lockfile.packages.contains_key("calculator"));
    assert_eq!(lockfile.packages["calculator"].source, "path:../calculator");

    // 4. Verify package is installed in .shae/packages/calculator
    let installed_lib_file = app_dir.join(".shae/packages/calculator/main.shae");
    assert!(installed_lib_file.exists());

    // 5. In app, write consumer script and run it
    let app_code = r#"
use { add, multiply } from "calculator"

let s = add(10, 20)
let m = multiply(s, 2)

[s, m]
"#;
    let app_main = app_dir.join("main.shae");
    fs::write(&app_main, app_code).unwrap();

    let val = run_file(&app_main).expect("Execution with package import failed");
    assert_eq!(val.to_json(), serde_json::json!([30, 60]));
}

#[test]
fn test_pkg_git_dependency_with_local_git_repo() {
    let temp_root = TempDir::new("git_dep");
    let git_repo_dir = temp_root.path.join("remote_repo");
    let app_dir = temp_root.path.join("app");

    fs::create_dir_all(&git_repo_dir).unwrap();

    // Initialize git repo
    let run_git = |args: &[&str], dir: &std::path::Path| {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("git execution failed");
        assert!(status.success());
    };

    run_git(&["init"], &git_repo_dir);
    run_git(&["config", "user.name", "Test User"], &git_repo_dir);
    run_git(&["config", "user.email", "test@example.com"], &git_repo_dir);

    // Create manifest and code in git repo
    let _ = pkg::init_project(&git_repo_dir, Some("greeter")).unwrap();
    let greeter_code = r#"
fn greet(name) {
    "Greetings, " + name + "!"
}
"#;
    fs::write(git_repo_dir.join("main.shae"), greeter_code).unwrap();
    run_git(&["add", "-A"], &git_repo_dir);
    run_git(&["commit", "-m", "Initial commit"], &git_repo_dir);

    // Get commit SHA
    let rev_out = Command::new("git")
        .current_dir(&git_repo_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let initial_sha = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();

    // 2. Init consumer app and add git dependency
    let _ = pkg::init_project(&app_dir, Some("app")).unwrap();
    let git_url = format!("file://{}", git_repo_dir.display());

    let lock = pkg::add_dependency(&app_dir, "greeter", &git_url, None, None, None)
        .expect("add git dependency failed");

    assert_eq!(lock.packages["greeter"].commit.as_ref().unwrap(), &initial_sha);

    // 3. Test running script using git dependency
    let app_code = r#"
use { greet } from "greeter"

greet("Shae Developer")
"#;
    let app_main = app_dir.join("main.shae");
    fs::write(&app_main, app_code).unwrap();

    let val = run_file(&app_main).expect("Git package execution failed");
    assert_eq!(val.to_json(), serde_json::json!("Greetings, Shae Developer!"));

    // 4. Test Lockfile persistence
    let reloaded_lock = Lockfile::load_from_dir(&app_dir).unwrap().expect("lockfile should exist");
    assert_eq!(reloaded_lock.packages["greeter"].commit.as_ref().unwrap(), &initial_sha);
}

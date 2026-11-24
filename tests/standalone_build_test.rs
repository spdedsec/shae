use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "shae_build_test_{}_{}_{}",
            name,
            std::process::id(),
            fastrand::u64(..)
        ));
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

fn get_shae_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_shae") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_shae"))
}

#[test]
fn test_standalone_build_and_execution_without_source() {
    let temp = TempDir::new("standalone_exec");
    let shae_bin = get_shae_binary();

    let helper_file = temp.path.join("helper.shae");
    let main_file = temp.path.join("main.shae");
    let output_bin = temp.path.join("my_app_bin");

    fs::write(
        &helper_file,
        r#"
fn compute_sum(a, b) {
    return a + b
}
"#,
    )
    .unwrap();

    fs::write(
        &main_file,
        r#"
use { compute_sum } from "./helper.shae"
use { args } from "std:cli"

let cli_args = args()
let total = compute_sum(100, 42)
print("APP_OUTPUT: total={total}, args_count={cli_args.len}")
for arg in cli_args {
    print("ARG: {arg}")
}
"#,
    )
    .unwrap();

    // 1. Build standalone executable
    let build_status = Command::new(&shae_bin)
        .args([
            "build",
            main_file.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--standalone",
        ])
        .status()
        .expect("failed to execute shae build");
    assert!(build_status.success(), "shae build must exit 0");
    assert!(output_bin.exists(), "Output binary must exist");

    // 2. Execute standalone binary with CLI arguments while source files exist
    let output1 = Command::new(&output_bin)
        .args(["param1", "param2"])
        .output()
        .expect("failed to execute generated standalone binary");
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(stdout1.contains("APP_OUTPUT: total=142, args_count=3"));
    assert!(stdout1.contains("ARG: param1"));
    assert!(stdout1.contains("ARG: param2"));

    // 3. Remove all source code files from disk
    fs::remove_file(&main_file).unwrap();
    fs::remove_file(&helper_file).unwrap();

    // 4. Re-execute standalone binary - must execute 100% self-contained from embedded payload
    let output2 = Command::new(&output_bin)
        .args(["standalone_check", "zero_disk_dependency"])
        .output()
        .expect("failed to execute generated standalone binary without sources");
    assert!(
        output2.status.success(),
        "Standalone binary must succeed with zero source files on disk"
    );
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("APP_OUTPUT: total=142, args_count=3"));
    assert!(stdout2.contains("ARG: standalone_check"));
    assert!(stdout2.contains("ARG: zero_disk_dependency"));
}

#[test]
fn test_standalone_build_default_output_naming() {
    let temp = TempDir::new("default_name");
    let shae_bin = get_shae_binary();

    let script = temp.path.join("greeter.shae");
    fs::write(&script, r#"print("GREETINGS_FROM_DEFAULT_NAME")"#).unwrap();

    // Invoke without -o flag
    let status = Command::new(&shae_bin)
        .current_dir(&temp.path)
        .args(["build", script.to_str().unwrap(), "--standalone"])
        .status()
        .expect("failed to execute shae build --standalone");
    assert!(status.success());

    #[cfg(windows)]
    let expected_bin = temp.path.join("greeter.exe");
    #[cfg(not(windows))]
    let expected_bin = temp.path.join("greeter");

    assert!(
        expected_bin.exists(),
        "Default binary {:?} must exist",
        expected_bin
    );

    let output = Command::new(&expected_bin)
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GREETINGS_FROM_DEFAULT_NAME"));
}

#[test]
fn test_standalone_build_custom_output_path() {
    let temp = TempDir::new("custom_output");
    let shae_bin = get_shae_binary();

    let script = temp.path.join("custom_app.shae");
    let custom_bin = temp
        .path
        .join("sub_dir")
        .join("nested")
        .join("my_nested_app");

    fs::write(&script, r#"print("NESTED_BIN_RUNS")"#).unwrap();

    let status = Command::new(&shae_bin)
        .args([
            "build",
            script.to_str().unwrap(),
            "-o",
            custom_bin.to_str().unwrap(),
            "--standalone",
        ])
        .status()
        .expect("failed to build nested binary");
    assert!(status.success());
    assert!(custom_bin.exists());

    let output = Command::new(&custom_bin).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("NESTED_BIN_RUNS"));
}

#[test]
fn test_standalone_build_cli_args_forwarding() {
    let temp = TempDir::new("args_forwarding");
    let shae_bin = get_shae_binary();

    let script = temp.path.join("args_test.shae");
    let output_bin = temp.path.join("args_app");

    fs::write(
        &script,
        r#"
let sys = use "std:sys"
let args_list = sys.args()
for a in args_list {
    print("GOT_ARG: " + a)
}
"#,
    )
    .unwrap();

    let status = Command::new(&shae_bin)
        .args([
            "build",
            script.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--standalone",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let output = Command::new(&output_bin)
        .args(["--alpha", "beta", "42", "gamma=xyz"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GOT_ARG: --alpha"));
    assert!(stdout.contains("GOT_ARG: beta"));
    assert!(stdout.contains("GOT_ARG: 42"));
    assert!(stdout.contains("GOT_ARG: gamma=xyz"));
}

#[test]
fn test_standalone_build_exit_code_propagation() {
    let temp = TempDir::new("exit_code");
    let shae_bin = get_shae_binary();

    let script = temp.path.join("exit_test.shae");
    let output_bin = temp.path.join("exit_app");

    fs::write(
        &script,
        r#"
let sys = use "std:sys"
sys.exit(42)
"#,
    )
    .unwrap();

    let status = Command::new(&shae_bin)
        .args([
            "build",
            script.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--standalone",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let output = Command::new(&output_bin).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(42),
        "Exit code must propagate to OS process"
    );
}

#[test]
fn test_standalone_build_parity_with_bundle() {
    let temp = TempDir::new("parity_bundle");
    let shae_bin = get_shae_binary();

    let script = temp.path.join("parity.shae");
    let bin_build = temp.path.join("bin_via_build");
    let bin_bundle = temp.path.join("bin_via_bundle");

    fs::write(
        &script,
        r#"
fn fact(n) {
    if n <= 1 { return 1 }
    return n * fact(n - 1)
}
let f = fact(6)
print("FACT_6={f}")
"#,
    )
    .unwrap();

    // 1. Build via `shae build --standalone`
    let s1 = Command::new(&shae_bin)
        .args([
            "build",
            script.to_str().unwrap(),
            "-o",
            bin_build.to_str().unwrap(),
            "--standalone",
        ])
        .status()
        .unwrap();
    assert!(s1.success());

    // 2. Build via `shae bundle`
    let s2 = Command::new(&shae_bin)
        .args([
            "bundle",
            script.to_str().unwrap(),
            "-o",
            bin_bundle.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(s2.success());

    // Both binaries must execute identically
    let out1 = Command::new(&bin_build).output().unwrap();
    let out2 = Command::new(&bin_bundle).output().unwrap();

    assert!(out1.status.success());
    assert!(out2.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out1.stdout),
        String::from_utf8_lossy(&out2.stdout)
    );
    assert!(String::from_utf8_lossy(&out1.stdout).contains("FACT_6=720"));
}

#[test]
fn test_standalone_build_missing_file_error() {
    let temp = TempDir::new("missing_file");
    let shae_bin = get_shae_binary();

    let bogus_file = temp.path.join("does_not_exist_xyz.shae");
    let output_bin = temp.path.join("should_not_exist");

    let output = Command::new(&shae_bin)
        .args([
            "build",
            bogus_file.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--standalone",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success(), "Command must fail for missing file");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist") || stderr.contains("not found"));
    assert!(!output_bin.exists(), "Output binary must not be generated");
}

#[test]
fn test_standalone_build_syntax_error_handling() {
    let temp = TempDir::new("syntax_err");
    let shae_bin = get_shae_binary();

    let broken_file = temp.path.join("broken.shae");
    let output_bin = temp.path.join("should_not_exist_broken");

    fs::write(&broken_file, "fn incomplete( { let x = \n").unwrap();

    let output = Command::new(&shae_bin)
        .args([
            "build",
            broken_file.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--standalone",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "Command must fail on syntax error"
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error") || stderr.contains("Failed to compile"));
    assert!(
        !output_bin.exists(),
        "Output binary must not be generated on error"
    );
}

#[test]
fn test_standalone_build_release_strip_bytecode_mode() {
    let temp = TempDir::new("standalone_strip");
    let shae_bin = get_shae_binary();

    let main_file = temp.path.join("main.shae");
    let output_bin = temp.path.join("stripped_app");

    fs::write(
        &main_file,
        r#"
let multiplier = 5
let val = 20 * multiplier
print("STRIPPED_OUTPUT: {val}")
"#,
    )
    .unwrap();

    // Build with --release / --strip
    let build_status = Command::new(&shae_bin)
        .args([
            "build",
            main_file.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--release",
        ])
        .status()
        .expect("failed to execute shae build --release");
    assert!(build_status.success());

    // Remove source file
    fs::remove_file(&main_file).unwrap();

    // Execute stripped binary
    let output = Command::new(&output_bin)
        .output()
        .expect("failed to run stripped binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("STRIPPED_OUTPUT: 100"));
}

#[test]
fn test_shae_build_manifest_autodetection() {
    let temp = TempDir::new("manifest_autodetect");
    let shae_bin = get_shae_binary();

    let src_dir = temp.path.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let manifest_file = temp.path.join("shae.toml");
    fs::write(
        &manifest_file,
        r#"
[package]
name = "custom_tool"
version = "0.9.0"
entry = "src/entry.shae"
"#,
    )
    .unwrap();

    let entry_file = src_dir.join("entry.shae");
    fs::write(
        &entry_file,
        r#"
print("MANIFEST_ENTRY_EXECUTED_SUCCESSFULLY: custom_tool v0.9.0")
"#,
    )
    .unwrap();

    // Run `shae build` in the package directory without specifying file
    let build_status = Command::new(&shae_bin)
        .current_dir(&temp.path)
        .args(["build"])
        .status()
        .expect("failed to run shae build with manifest");
    assert!(build_status.success());

    let expected_bin = temp.path.join("custom_tool");
    assert!(
        expected_bin.exists(),
        "Expected auto-detected binary custom_tool to exist"
    );

    // Execute the auto-detected binary
    let output = Command::new(&expected_bin)
        .output()
        .expect("failed to run auto-detected binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MANIFEST_ENTRY_EXECUTED_SUCCESSFULLY: custom_tool v0.9.0"));
}

#[test]
fn test_shae_build_bytecode_target() {
    let temp = TempDir::new("bytecode_target");
    let shae_bin = get_shae_binary();

    let script_file = temp.path.join("calc.shae");
    let bc_out = temp.path.join("calc.shaec");

    fs::write(
        &script_file,
        r#"
let x = 12
let y = 30
print("BYTECODE_CALC: {x + y}")
"#,
    )
    .unwrap();

    let build_status = Command::new(&shae_bin)
        .args([
            "build",
            script_file.to_str().unwrap(),
            "--bytecode",
            "-o",
            bc_out.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run shae build --bytecode");
    assert!(build_status.success());
    assert!(bc_out.exists());

    // Verify bytecode magic bytes
    let bc_bytes = fs::read(&bc_out).unwrap();
    assert!(bc_bytes.starts_with(shae::chunk::BYTECODE_MAGIC));

    // Execute bytecode file via `shae run`
    let run_output = Command::new(&shae_bin)
        .args(["run", bc_out.to_str().unwrap()])
        .output()
        .expect("failed to run bytecode via shae run");
    assert!(run_output.status.success());
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("BYTECODE_CALC: 42"));
}

#[test]
fn test_shae_build_wasm_target() {
    let temp = TempDir::new("wasm_target");
    let shae_bin = get_shae_binary();

    let script_file = temp.path.join("calc.shae");
    let wasm_out = temp.path.join("calc.wasm");

    fs::write(
        &script_file,
        r#"
fn main() {
    return 42
}
"#,
    )
    .unwrap();

    let build_status = Command::new(&shae_bin)
        .args([
            "build",
            script_file.to_str().unwrap(),
            "--wasm",
            "-o",
            wasm_out.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run shae build --wasm");
    assert!(build_status.success());
    assert!(wasm_out.exists());

    let wasm_bytes = fs::read(&wasm_out).unwrap();
    assert!(
        wasm_bytes.starts_with(b"\0asm"),
        "Must have WASM binary magic"
    );
}

#[test]
fn test_shae_build_repackaging_protection() {
    let temp = TempDir::new("repackaging_prot");
    let shae_bin = get_shae_binary();

    let script1 = temp.path.join("v1.shae");
    let bin1 = temp.path.join("app_v1");

    fs::write(&script1, "print(\"VERSION_1\")\n").unwrap();

    // 1. Build binary 1
    let status1 = Command::new(&shae_bin)
        .args([
            "build",
            script1.to_str().unwrap(),
            "-o",
            bin1.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status1.success());
    let size1 = fs::metadata(&bin1).unwrap().len();

    // 2. Use binary 1 to build binary 2
    let script2 = temp.path.join("v2.shae");
    let bin2 = temp.path.join("app_v2");
    fs::write(&script2, "print(\"VERSION_2\")\n").unwrap();

    let status2 = Command::new(&bin1)
        .args([
            "build",
            script2.to_str().unwrap(),
            "-o",
            bin2.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status2.success());
    let size2 = fs::metadata(&bin2).unwrap().len();

    // Repackaging must not accumulate duplicate payloads
    let diff = if size2 > size1 {
        size2 - size1
    } else {
        size1 - size2
    };
    assert!(
        diff < 2048,
        "Repackaging should not accumulate old bundles: size1={}, size2={}",
        size1,
        size2
    );

    let output2 = Command::new(&bin2).output().unwrap();
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("VERSION_2"));
    assert!(!stdout2.contains("VERSION_1"));
}

#[test]
#[cfg(target_os = "linux")]
fn test_standalone_binary_ldd_zero_shae_dependencies() {
    let temp = TempDir::new("ldd_test");
    let shae_bin = get_shae_binary();

    let script = temp.path.join("ldd_check.shae");
    let output_bin = temp.path.join("ldd_app");

    fs::write(&script, r#"print("LDD_OK")"#).unwrap();

    let status = Command::new(&shae_bin)
        .args([
            "build",
            script.to_str().unwrap(),
            "-o",
            output_bin.to_str().unwrap(),
            "--standalone",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    // Check ldd output
    let ldd_out = Command::new("ldd").arg(&output_bin).output();

    if let Ok(ldd_res) = ldd_out {
        let stdout = String::from_utf8_lossy(&ldd_res.stdout);
        // Ensure no external shae, python, node or non-system libraries are referenced
        assert!(
            !stdout.contains("/workspaces/shae"),
            "Binary must not reference workspace paths in ldd"
        );
    }

    let run_out = Command::new(&output_bin).output().unwrap();
    assert!(run_out.status.success());
    assert!(String::from_utf8_lossy(&run_out.stdout).contains("LDD_OK"));
}

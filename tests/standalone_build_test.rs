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
    assert!(output2.status.success(), "Standalone binary must succeed with zero source files on disk");
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("APP_OUTPUT: total=142, args_count=3"));
    assert!(stdout2.contains("ARG: standalone_check"));
    assert!(stdout2.contains("ARG: zero_disk_dependency"));
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
    assert!(expected_bin.exists(), "Expected auto-detected binary custom_tool to exist");

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
    assert!(wasm_bytes.starts_with(b"\0asm"), "Must have WASM binary magic");
}

#[test]
fn test_shae_build_repackaging_protection() {
    let temp = TempDir::new("repackaging_prot");
    let shae_bin = get_shae_binary();

    let script1 = temp.path.join("v1.shae");
    let bin1 = temp.path.join("app_v1");

    fs::write(&script1, "print(\"VERSION_1\")\n").unwrap();

    // 1. Build binary 1 using shae build
    let status1 = Command::new(&shae_bin)
        .args(["build", script1.to_str().unwrap(), "-o", bin1.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status1.success());
    let size1 = fs::metadata(&bin1).unwrap().len();

    // 2. Repackage binary 1 into binary 2 with script 2
    let script2 = temp.path.join("v2.shae");
    let bin2 = temp.path.join("app_v2");
    fs::write(&script2, "print(\"VERSION_2\")\n").unwrap();

    // Use create_standalone_binary_from_stub with bin1 as the stub
    shae::bundle::create_standalone_binary_from_stub(&bin1, &script2, &bin2, true, false)
        .expect("repackaging from bundled stub should succeed");
    assert!(bin2.exists());
    let size2 = fs::metadata(&bin2).unwrap().len();

    // Repackaging must strip old payloads cleanly without bloat
    let diff = if size2 > size1 { size2 - size1 } else { size1 - size2 };
    assert!(
        diff < 2048,
        "Repackaging should not accumulate old bundles: size1={}, size2={}, diff={}",
        size1,
        size2,
        diff
    );

    // Verify binary 2 executes script 2 and NOT script 1
    let output2 = Command::new(&bin2).output().unwrap();
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("VERSION_2"));
    assert!(!stdout2.contains("VERSION_1"));
}

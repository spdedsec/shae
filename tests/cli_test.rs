use std::process::Command;

#[test]
fn test_cli_direct_execution() {
    let output = Command::new(env!("CARGO_BIN_EXE_shae"))
        .arg("tests/shae/sample_target.shae")
        .output()
        .expect("Failed to execute shae");
    assert!(output.status.success());
}

#[test]
fn test_cli_jit_execution() {
    let output = Command::new(env!("CARGO_BIN_EXE_shae"))
        .args(["jit", "tests/shae/sample_target.shae"])
        .output()
        .expect("Failed to execute shae jit");
    assert!(output.status.success());
}

#[test]
fn test_cli_jit_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_shae"))
        .args(["jit", "--help"])
        .output()
        .expect("Failed to execute shae jit --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Shae Cranelift JIT"));
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("shae jit <file.shae>"));
}

#[test]
fn test_cli_unknown_subcommand_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_shae"))
        .arg("unknown_bogus_subcommand")
        .output()
        .expect("Failed to execute shae");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: unknown subcommand 'unknown_bogus_subcommand'"));
    assert!(stderr.contains("Available subcommands:"));
    assert!(stderr.contains("run, jit, repl, compile, check, disasm, fmt, bench, test, doc, clean"));
    assert!(stderr.contains("Did you mean to run a script?"));
    assert!(stderr.contains("shae run <file>"));
    assert!(stderr.contains("shae <file.shae>"));
}

use shae::llvm::LlvmCompiler;
use shae::run_file;
use shae::run_file_vm;
use std::fs;
use std::process::Command;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

fn has_clang() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_pure_shae_self_llvm_dual_engine() {
    run_with_large_stack(|| {
        for path in &[
            "tests/shae/self_llvm_test.shae",
            "tests/shae/llvm_codegen_test.shae",
        ] {
            // Bytecode VM
            let vm_res = run_file_vm(path);
            assert!(vm_res.is_ok(), "{} failed on VM: {:?}", path, vm_res.err());

            // AST Evaluator
            let ast_res = run_file(path);
            assert!(
                ast_res.is_ok(),
                "{} failed on AST: {:?}",
                path,
                ast_res.err()
            );
        }
    });
}

#[test]
fn test_llvm_compiler_ir_generation_arithmetic() {
    let source = r#"
fn compute_ops(a, b) {
    let sum = a + b
    let diff = a - b
    let prod = a * b
    let quot = a / b
    let rem = a % b
    let bit_and = a & b
    let bit_or = a | b
    let bit_xor = a ^ b
    let shl = a << 2
    let shr = a >> 1
    let neg = -a
    let inv = not false
    return sum + diff + prod + quot + rem + bit_and + bit_or + bit_xor + shl + shr + neg
}

let result = compute_ops(100, 7)
"#;

    let mut compiler = LlvmCompiler::new();
    let ir = compiler
        .compile_source(source)
        .expect("LLVM IR compilation failed");

    // Module header & triple
    assert!(ir.contains("source_filename = \"shae_module\""));
    assert!(ir.contains("target triple = \"x86_64-pc-linux-gnu\""));

    // Function definition
    assert!(ir.contains("define i64 @compute_ops(i64 %p0, i64 %p1)"));

    // SSA instructions
    assert!(ir.contains("add i64"));
    assert!(ir.contains("sub i64"));
    assert!(ir.contains("mul i64"));
    assert!(ir.contains("sdiv i64"));
    assert!(ir.contains("srem i64"));
    assert!(ir.contains("and i64"));
    assert!(ir.contains("or i64"));
    assert!(ir.contains("xor i64"));
    assert!(ir.contains("shl i64"));
    assert!(ir.contains("ashr i64"));

    // Global variable
    assert!(ir.contains("@result = internal global i64 0, align 8"));
}

#[test]
fn test_llvm_compiler_control_flow() {
    let source = r#"
fn abs_val(x) {
    if x < 0 {
        return -x
    } else {
        return x
    }
}

fn sum_to_n(n) {
    let total = 0
    let i = 1
    while i <= n {
        total = total + i
        i = i + 1
    }
    return total
}

let a = abs_val(-42)
let s = sum_to_n(10)
"#;

    let mut compiler = LlvmCompiler::new();
    let ir = compiler
        .compile_source(source)
        .expect("LLVM IR compilation failed");

    assert!(ir.contains("define i64 @abs_val(i64 %p0)"));
    assert!(ir.contains("icmp slt i64"));
    assert!(ir.contains("define i64 @sum_to_n(i64 %p0)"));
    assert!(ir.contains("icmp sle i64"));
    assert!(ir.contains("br i1"));
}

#[test]
fn test_llvm_native_compilation_and_execution() {
    if !has_clang() {
        eprintln!("clang not available, skipping native execution test");
        return;
    }

    let source = r#"
fn fib(n) {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

fn factorial(n) {
    let acc = 1
    let i = 1
    while i <= n {
        acc = acc * i
        i = i + 1
    }
    return acc
}

let f = factorial(6)
let fib_val = fib(10)
println("Factorial 6: ", f)
println("Fibonacci 10: ", fib_val)
"#;

    let temp_ll = "/tmp/test_llvm_run.ll";
    let temp_bin = "/tmp/test_llvm_run_bin";

    let mut compiler = LlvmCompiler::new();
    compiler
        .compile_and_link(source, temp_ll, temp_bin, 2)
        .expect("Compilation with clang failed");

    let output = Command::new(temp_bin)
        .output()
        .expect("Failed to execute native binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Factorial 6: 720\nFibonacci 10: 55");

    let _ = fs::remove_file(temp_ll);
    let _ = fs::remove_file(temp_bin);
}

#[test]
fn test_llvm_floats_and_string_literals() {
    if !has_clang() {
        eprintln!("clang not available, skipping native float test");
        return;
    }

    let source = r#"
let greeting = "Hello Native LLVM!"
println(greeting)
let pi = 3.14159
let radius = 2.0
let area = pi * radius * radius
println("Area: ", area)
"#;

    let temp_ll = "/tmp/test_llvm_floats.ll";
    let temp_bin = "/tmp/test_llvm_floats_bin";

    let mut compiler = LlvmCompiler::new();
    compiler
        .compile_and_link(source, temp_ll, temp_bin, 2)
        .expect("Compilation with clang failed");

    let output = Command::new(temp_bin)
        .output()
        .expect("Failed to execute native binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Hello Native LLVM!"));
    assert!(stdout.contains("Area: 12.566360") || stdout.contains("Area: 12.56636"));

    let _ = fs::remove_file(temp_ll);
    let _ = fs::remove_file(temp_bin);
}

#[test]
fn test_llvm_native_exit_code_and_user_main() {
    if !has_clang() {
        eprintln!("clang not available, skipping native exit code test");
        return;
    }

    // Test exit(42)
    let src_exit = "exit(42)";
    let temp_ll1 = "/tmp/test_llvm_exit.ll";
    let temp_bin1 = "/tmp/test_llvm_exit_bin";
    let mut compiler = LlvmCompiler::new();
    compiler
        .compile_and_link(src_exit, temp_ll1, temp_bin1, 2)
        .expect("Compilation with clang failed");
    let out1 = Command::new(temp_bin1)
        .output()
        .expect("Failed to execute native binary");
    assert_eq!(out1.status.code(), Some(42));
    let _ = fs::remove_file(temp_ll1);
    let _ = fs::remove_file(temp_bin1);

    // Test fn main() { return 77 }
    let src_main = "fn main() { return 77 }";
    let temp_ll2 = "/tmp/test_llvm_user_main.ll";
    let temp_bin2 = "/tmp/test_llvm_user_main_bin";
    compiler
        .compile_and_link(src_main, temp_ll2, temp_bin2, 2)
        .expect("Compilation with clang failed");
    let out2 = Command::new(temp_bin2)
        .output()
        .expect("Failed to execute native binary");
    assert_eq!(out2.status.code(), Some(77));
    let _ = fs::remove_file(temp_ll2);
    let _ = fs::remove_file(temp_bin2);
}

#[test]
fn test_llvm_string_operations() {
    if !has_clang() {
        eprintln!("clang not available, skipping string operations test");
        return;
    }

    let source = r#"
let s1 = "hello "
let s2 = "world!"
let combined = s1 + s2
println(combined)
let eq1 = (s1 == "hello ")
let eq2 = (s1 == s2)
println(eq1)
println(eq2)
"#;

    let temp_ll = "/tmp/test_llvm_str_ops.ll";
    let temp_bin = "/tmp/test_llvm_str_ops_bin";

    let mut compiler = LlvmCompiler::new();
    compiler
        .compile_and_link(source, temp_ll, temp_bin, 2)
        .expect("Compilation with clang failed");

    let output = Command::new(temp_bin)
        .output()
        .expect("Failed to execute native binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello world!"));
    assert!(stdout.contains("true"));
    assert!(stdout.contains("false"));

    let _ = fs::remove_file(temp_ll);
    let _ = fs::remove_file(temp_bin);
}

#[test]
fn test_llvm_cli_subcommand() {
    if !has_clang() {
        eprintln!("clang not available, skipping CLI test");
        return;
    }

    let sample_shae = "tests/shae/sample_target.shae";
    let out_ll = "/tmp/test_cli_sample.ll";
    let out_bin = "/tmp/test_cli_sample_bin";

    // 0. Test --help
    let status_help = Command::new(env!("CARGO_BIN_EXE_shae"))
        .args(["llvm", "--help"])
        .output()
        .expect("Failed to execute shae llvm --help");
    assert!(status_help.status.success());
    let help_txt = String::from_utf8_lossy(&status_help.stdout);
    assert!(help_txt.contains("Usage: shae llvm"));

    // 1. Generate LLVM IR
    let status_ir = Command::new(env!("CARGO_BIN_EXE_shae"))
        .args(["llvm", sample_shae, "-o", out_ll])
        .status()
        .expect("Failed to execute shae llvm command");
    assert!(status_ir.success());
    assert!(fs::metadata(out_ll).is_ok());

    // 2. Compile to binary
    let status_compile = Command::new(env!("CARGO_BIN_EXE_shae"))
        .args(["llvm", sample_shae, "-c", "-o", out_bin])
        .status()
        .expect("Failed to execute shae llvm --compile command");
    assert!(status_compile.success());
    assert!(fs::metadata(out_bin).is_ok());

    // 3. Run directly with --run
    let output_run = Command::new(env!("CARGO_BIN_EXE_shae"))
        .args(["llvm", sample_shae, "-r"])
        .output()
        .expect("Failed to execute shae llvm --run command");
    // sample_target returns 42
    assert_eq!(output_run.status.code(), Some(42));

    let _ = fs::remove_file(out_ll);
    let _ = fs::remove_file(out_bin);
    let _ = fs::remove_file("sample_target.ll");
    let _ = fs::remove_file("sample_target");
}

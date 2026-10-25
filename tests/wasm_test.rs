use shae::run_file;
use shae::run_file_vm;
use shae::wasm::{WASM_MAGIC, WASM_VERSION, WasmCompiler, WasmRunner};
use std::fs;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

#[test]
fn test_pure_shae_self_wasm_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_wasm_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_wasm_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_wasm_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

#[test]
fn test_wasm_compiler_and_runner_execution() {
    let source = r#"
fn add(a, b) {
    return a + b
}

fn multiply(x, y) {
    let p = x * y
    return p
}

let res = add(30, 12)
return res
"#;

    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler
        .compile_source(source)
        .expect("Wasm compilation failed");

    // Check header
    assert!(wasm_bytes.starts_with(WASM_MAGIC));
    assert_eq!(&wasm_bytes[4..8], WASM_VERSION);

    // Run exported function "add" with args [15, 27]
    let add_res = WasmRunner::execute_function(&wasm_bytes, "add", &[15, 27])
        .expect("Failed to run WASM 'add'");
    assert_eq!(add_res, 42);

    // Run exported function "multiply" with args [6, 7]
    let mul_res = WasmRunner::execute_function(&wasm_bytes, "multiply", &[6, 7])
        .expect("Failed to run WASM 'multiply'");
    assert_eq!(mul_res, 42);

    // Run "main"
    let main_res =
        WasmRunner::execute_function(&wasm_bytes, "main", &[]).expect("Failed to run WASM 'main'");
    assert_eq!(main_res, 42);
}

#[test]
fn test_wasm_file_output_and_reload() {
    let source = "fn square(x) {\n    return x * x\n}\nsquare(9)\n";
    let mut compiler = WasmCompiler::new();
    let wasm_bytes = compiler.compile_source(source).expect("Compilation failed");

    let out_file = "tests/test_square_temp.wasm";
    fs::write(out_file, &wasm_bytes).expect("Failed to write wasm file");

    let read_back = fs::read(out_file).expect("Failed to read wasm file");
    let sq_res =
        WasmRunner::execute_function(&read_back, "square", &[9]).expect("Failed to execute square");
    assert_eq!(sq_res, 81);

    let _ = fs::remove_file(out_file);
}

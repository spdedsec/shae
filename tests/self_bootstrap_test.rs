use shae::run_file;
use shae::run_file_vm;
use std::fs;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

#[test]
fn test_pure_shae_self_bootstrap_pipeline_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_bootstrap_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_bootstrap_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_bootstrap_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

#[test]
fn test_pure_shae_bootstrap_bytecode_execution() {
    run_with_large_stack(|| {
        let compile_driver = r#"
use { compile_to_bytecode } from "lib/compiler/main.shae"
use { write } from "std:fs"

let user_src = "fn calculate(x, y) \{\n    return (x * 2) + (y * 3)\n\}\ncalculate(15, 4)\n"
write("tests/test_bootstrap_exec_temp.shae", user_src)
compile_to_bytecode("tests/test_bootstrap_exec_temp.shae", "tests/test_bootstrap_exec_temp.shaec")
"#;

        let res = shae::run(compile_driver).expect("Failed to run compiler driver in Shae");
        assert!(res.to_display().parse::<usize>().unwrap() > 0);

        // Read and verify the generated .shaec bytecode chunk in the VM
        let shaec_bytes = fs::read("tests/test_bootstrap_exec_temp.shaec")
            .expect("Failed to read test_bootstrap_exec_temp.shaec");
        let chunk = shae::chunk::Chunk::from_bytes(&shaec_bytes)
            .expect("Failed to deserialize chunk produced by pure-Shae compiler driver");

        let mut vm = shae::vm::VM::new();
        match vm.interpret(chunk) {
            shae::vm::InterpretResult::Ok(val) => {
                // calculate(15, 4) = (15 * 2) + (4 * 3) = 30 + 12 = 42
                assert_eq!(val, shae::value::Value::Int(42));
            }
            other => panic!("VM execution of pure-Shae bytecode failed: {:?}", other),
        }

        let _ = fs::remove_file("tests/test_bootstrap_exec_temp.shae");
        let _ = fs::remove_file("tests/test_bootstrap_exec_temp.shaec");
    });
}

#[test]
fn test_pure_shae_bootstrap_wasm_execution() {
    run_with_large_stack(|| {
        let compile_driver = r#"
use { compile_to_wasm } from "lib/compiler/main.shae"
use { write } from "std:fs"

let user_src = "fn multiply(a, b) \{\n    return a * b\n\}\nfn compute(x, y) \{\n    let p = multiply(x, y)\n    return p + 2\n\}\ncompute(10, 4)\n"
write("tests/test_bootstrap_wasm_temp.shae", user_src)
compile_to_wasm("tests/test_bootstrap_wasm_temp.shae", "tests/test_bootstrap_wasm_temp.wasm")
"#;

        let res = shae::run(compile_driver).expect("Failed to run compiler driver for WASM");
        assert!(res.to_display().parse::<usize>().unwrap() > 0);

        let wasm_bytes = fs::read("tests/test_bootstrap_wasm_temp.wasm")
            .expect("Failed to read test_bootstrap_wasm_temp.wasm");

        // Run exported function "compute" with args [10, 4] -> (10 * 4) + 2 = 42
        let compute_res =
            shae::wasm::WasmRunner::execute_function(&wasm_bytes, "compute", &[10, 4])
                .expect("Failed to run WASM 'compute'");
        assert_eq!(compute_res, 42);

        // Run "main"
        let main_res = shae::wasm::WasmRunner::execute_function(&wasm_bytes, "main", &[])
            .expect("Failed to run WASM 'main'");
        assert_eq!(main_res, 42);

        let _ = fs::remove_file("tests/test_bootstrap_wasm_temp.shae");
        let _ = fs::remove_file("tests/test_bootstrap_wasm_temp.wasm");
    });
}

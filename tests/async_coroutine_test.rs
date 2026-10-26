//! Milestone 3: First-Class async / await Coroutines Integration Test Suite
//! tests/async_coroutine_test.rs

use shae::{run_file, run_file_vm};

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

#[test]
fn test_async_coroutine_dual_engine() {
    run_with_large_stack(|| {
        let test_path = "tests/shae/async_coroutine_test.shae";

        println!("Running async coroutine test on AST Evaluator...");
        let res_ast = run_file(test_path);
        assert!(
            res_ast.is_ok(),
            "Async coroutine test failed on AST Evaluator: {:?}",
            res_ast
        );

        println!("Running async coroutine test on Bytecode VM...");
        let res_vm = run_file_vm(test_path);
        assert!(
            res_vm.is_ok(),
            "Async coroutine test failed on Bytecode VM: {:?}",
            res_vm
        );
    });
}

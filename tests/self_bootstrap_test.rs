use shae::run_file;
use shae::run_file_vm;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

#[test]
fn test_pure_shae_self_bootstrap_dual_engine() {
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

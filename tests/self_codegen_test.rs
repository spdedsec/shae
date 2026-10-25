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
fn test_pure_shae_self_parser_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_parser_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_parser_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_parser_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

#[test]
fn test_pure_shae_self_codegen_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_codegen_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_codegen_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_codegen_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

#[test]
fn test_pure_shae_compiled_bytecode_execution() {
    run_with_large_stack(|| {
        let compile_driver = r#"
use { compile_source_to_chunk } from "lib/compiler/codegen.shae"
use { serialize_chunk } from "lib/compiler/chunk.shae"
use { write_bytes } from "std:fs"

let user_src = "fn compute(a, b) \{\n    let sum = a + b\n    return sum * 3\n\}\ncompute(10, 4)\n"
let chunk = compile_source_to_chunk(user_src)
let bin_bytes = serialize_chunk(chunk)
write_bytes("tests/test_compiled_from_shae.shaec", bin_bytes)
bin_bytes.len
"#;

        let res = shae::run(compile_driver).expect("Failed to run compiler driver in Shae");
        assert!(res.to_display().parse::<usize>().unwrap() > 0);

        // Now read the generated .shaec file and run it directly with VM!
        let shaec_bytes = fs::read("tests/test_compiled_from_shae.shaec")
            .expect("Failed to read test_compiled_from_shae.shaec");
        let chunk = shae::chunk::Chunk::from_bytes(&shaec_bytes)
            .expect("Failed to deserialize chunk produced by pure-Shae codegen");

        let mut vm = shae::vm::VM::new();
        match vm.interpret(chunk) {
            shae::vm::InterpretResult::Ok(val) => {
                // compute(10, 4) = (10 + 4) * 3 = 42
                assert_eq!(val, shae::value::Value::Int(42));
            }
            other => panic!("VM execution of pure-Shae bytecode failed: {:?}", other),
        }

        let _ = fs::remove_file("tests/test_compiled_from_shae.shaec");
    });
}

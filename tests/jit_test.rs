use shae::compiler::Compiler;
use shae::jit::{JitEngine, JIT_THRESHOLD};
use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};

#[test]
fn test_jit_can_compile_pure_math() {
    let script = r#"
fn add(a, b) {
    return a + b
}
add(10, 25)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    // Verify VM interpret
    let res = vm.interpret(chunk);
    assert_eq!(res, InterpretResult::Ok(Value::Int(35)));
}

#[test]
fn test_jit_engine_direct_compilation() {
    let script = r#"
fn compute(a, b, c) {
    let x = a * b + c
    let y = x - 5
    return y
}
compute(6, 7, 8)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();

    // Find the CompiledFunction constant
    let func = chunk
        .constants
        .iter()
        .find_map(|v| match v {
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("CompiledFunction constant not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Failed to initialize JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compilation failed");

    // Execute compiled native function directly
    let args = [6i64, 7i64, 8i64];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };

    assert_eq!(status, 0);
    // 6 * 7 + 8 = 50; 50 - 5 = 45
    assert_eq!(out, 45);
}

#[test]
fn test_jit_loop_and_accumulator() {
    let script = r#"
fn sum_range(n) {
    let sum = 0
    let i = 1
    while i <= n {
        sum = sum + i
        i = i + 1
    }
    return sum
}
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();

    let func = chunk
        .constants
        .iter()
        .find_map(|v| match v {
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("CompiledFunction constant not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Failed to initialize JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compilation failed");

    let args = [100i64];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };

    assert_eq!(status, 0);
    // sum(1..=100) = 5050
    assert_eq!(out, 5050);
}

#[test]
fn test_jit_iterative_fibonacci() {
    let script = r#"
fn fib(n) {
    if n <= 1 {
        return n
    }
    let a = 0
    let b = 1
    let i = 2
    while i <= n {
        let c = a + b
        a = b
        b = c
        i = i + 1
    }
    return b
}
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();

    let func = chunk
        .constants
        .iter()
        .find_map(|v| match v {
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("CompiledFunction constant not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Failed to initialize JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compilation failed");

    // fib(10) = 55
    let args = [10i64];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };
    assert_eq!(status, 0);
    assert_eq!(out, 55);

    // fib(20) = 6765
    let args20 = [20i64];
    let mut out20: i64 = 0;
    let status20 = unsafe { jit_fn(args20.as_ptr(), args20.len(), &mut out20) };
    assert_eq!(status20, 0);
    assert_eq!(out20, 6765);
}

#[test]
fn test_jit_bitwise_and_comparisons() {
    let script = r#"
fn bitwise_ops(a, b) {
    let and_val = a & b
    let or_val = a | b
    let xor_val = a ^ b
    let shl_val = a << 2
    let shr_val = b >> 1
    return and_val + or_val + xor_val + shl_val + shr_val
}
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();

    let func = chunk
        .constants
        .iter()
        .find_map(|v| match v {
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("CompiledFunction constant not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Failed to initialize JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compilation failed");

    // a = 12 (0b1100), b = 10 (0b1010)
    // and = 8 (0b1000)
    // or  = 14 (0b1110)
    // xor = 6 (0b0110)
    // shl = 12 << 2 = 48
    // shr = 10 >> 1 = 5
    // expected = 8 + 14 + 6 + 48 + 5 = 81
    let args = [12i64, 10i64];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };
    assert_eq!(status, 0);
    assert_eq!(out, 81);
}

#[test]
fn test_jit_zero_division_safety() {
    let script = r#"
fn div_op(a, b) {
    return a / b
}
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();

    let func = chunk
        .constants
        .iter()
        .find_map(|v| match v {
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("CompiledFunction constant not found");

    let mut engine = JitEngine::new().expect("Failed to initialize JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compilation failed");

    // Division by zero should return status code 2 (div by zero bailout)
    let args = [42i64, 0i64];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };
    assert_eq!(status, 2);
}

#[test]
fn test_jit_hot_invocation_tiered_trigger() {
    let script = r#"
fn square(x) {
    return x * x
}

let sum = 0
let i = 0
while i < 30 {
    sum = sum + square(i)
    i = i + 1
}
sum
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    assert!(matches!(res, InterpretResult::Ok(Value::Int(_))));

    // square was called 30 times, exceeding JIT_THRESHOLD (20)
    let square_func = vm
        .globals
        .get("square")
        .and_then(|v| match v {
            Value::Closure(c) => Some(c.function.clone()),
            Value::CompiledFunction(f) => Some(f.clone()),
            _ => None,
        })
        .expect("square function not found in globals");

    assert!(square_func.jit.invocation_count.load(std::sync::atomic::Ordering::Relaxed) >= JIT_THRESHOLD);
    assert!(square_func.get_jit_fn().is_some(), "Function should be tiered into JIT native code");
}

#[test]
fn test_jit_script_builtin_controls() {
    let script = r#"
let s = jit("status")
let d = jit("disable")
let dis_status = jit("status")
let e = jit("enable")
let en_status = jit("status")
[s, dis_status, en_status]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    match res {
        InterpretResult::Ok(Value::Array(arr)) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::String("enabled".into()));
            assert_eq!(guard[1], Value::String("disabled".into()));
            assert_eq!(guard[2], Value::String("enabled".into()));
        }
        _ => panic!("Expected array result, got {:?}", res),
    }
}

#[test]
fn test_jit_manual_compilation_builtin() {
    let script = r#"
fn double(x) {
    return x + x
}

let compiled = jit("compile", double)
let result = double(21)
[compiled, result]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    match res {
        InterpretResult::Ok(Value::Array(arr)) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Int(42));
        }
        _ => panic!("Expected array result, got {:?}", res),
    }
}

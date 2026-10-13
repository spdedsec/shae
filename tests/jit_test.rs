use shae::compiler::Compiler;
use shae::jit::{JIT_THRESHOLD, JitEngine, ShaeJitArray};
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

    assert!(
        square_func
            .jit
            .invocation_count
            .load(std::sync::atomic::Ordering::Relaxed)
            >= JIT_THRESHOLD
    );
    assert!(
        square_func.get_jit_fn().is_some(),
        "Function should be tiered into JIT native code"
    );
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

#[test]
fn test_jit_float_direct_compilation() {
    let script = r#"
fn circle_area(radius) {
    let pi = 3.141592653589793
    return pi * radius * radius
}
circle_area(2.0)
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

    let radius_val = (2.0f64).to_bits() as i64;
    let args = [radius_val];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };

    assert_eq!(status, 3);
    let res = f64::from_bits(out as u64);
    assert!((res - 12.566370614359172).abs() < 1e-9);
}

#[test]
fn test_jit_float_vm_integration() {
    let script = r#"
fn compute_f64(x, y) {
    let diff = x - y
    let prod = diff * 2.5
    return prod / 1.25
}

let compiled = jit("compile", compute_f64)
let res = compute_f64(10.5, 3.5)
[compiled, res]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    match res {
        InterpretResult::Ok(Value::Array(arr)) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            match &guard[1] {
                Value::Float(f) | Value::Number(f) => {
                    assert!((*f - 14.0).abs() < 1e-9);
                }
                other => panic!("Expected Float, got {:?}", other),
            }
        }
        _ => panic!("Expected array result, got {:?}", res),
    }
}

#[test]
fn test_jit_recursive_fibonacci_direct() {
    let script = r#"
fn rec_fib(n) {
    if n <= 1 {
        return n
    }
    return rec_fib(n - 1) + rec_fib(n - 2)
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
        .expect("CompiledFunction not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Init JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compile recursive fib");

    // fib(10) = 55
    let args10 = [10i64];
    let mut out10: i64 = 0;
    let status10 = unsafe { jit_fn(args10.as_ptr(), args10.len(), &mut out10) };
    assert_eq!(status10, 0);
    assert_eq!(out10, 55);

    // fib(15) = 610
    let args15 = [15i64];
    let mut out15: i64 = 0;
    let status15 = unsafe { jit_fn(args15.as_ptr(), args15.len(), &mut out15) };
    assert_eq!(status15, 0);
    assert_eq!(out15, 610);
}

#[test]
fn test_jit_recursive_factorial_direct() {
    let script = r#"
fn rec_fact(n) {
    if n <= 1 {
        return 1
    }
    return n * rec_fact(n - 1)
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
        .expect("CompiledFunction not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Init JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compile recursive factorial");

    // fact(6) = 720
    let args6 = [6i64];
    let mut out6: i64 = 0;
    let status6 = unsafe { jit_fn(args6.as_ptr(), args6.len(), &mut out6) };
    assert_eq!(status6, 0);
    assert_eq!(out6, 720);
}

#[test]
fn test_jit_recursive_vm_integration() {
    let script = r#"
fn rec_fib(n) {
    if n <= 1 {
        return n
    }
    return rec_fib(n - 1) + rec_fib(n - 2)
}

let compiled = jit("compile", rec_fib)
let res = rec_fib(12)

[compiled, res]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    match res {
        InterpretResult::Ok(Value::Array(arr)) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Int(144));
        }
        _ => panic!("Expected array result, got {:?}", res),
    }
}

#[test]
fn test_jit_array_indexing_direct() {
    let script = r#"
fn sum_arr(arr, len) {
    let mut sum = 0
    let mut i = 0
    while i < len {
        sum = sum + arr[i]
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
        .expect("CompiledFunction not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Init JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compile array sum");

    let elements = [10i64, 25i64, 40i64, 15i64];
    let buf = ShaeJitArray::create_buffer(&elements);
    let args = [buf.as_ptr() as i64, elements.len() as i64];
    let mut out: i64 = 0;
    let status = unsafe { jit_fn(args.as_ptr(), args.len(), &mut out) };

    assert_eq!(status, 0);
    // 10 + 25 + 40 + 15 = 90
    assert_eq!(out, 90);
}

#[test]
fn test_jit_array_bounds_check_bailout() {
    let script = r#"
fn get_elem(arr, idx) {
    return arr[idx]
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
        .expect("CompiledFunction not found");

    assert!(JitEngine::can_compile(&func));

    let mut engine = JitEngine::new().expect("Init JitEngine");
    let jit_fn = engine
        .compile_function(&func)
        .expect("JIT compile get_elem");

    let elements = [42i64, 99i64];
    let buf = ShaeJitArray::create_buffer(&elements);

    // Valid index 0
    let args0 = [buf.as_ptr() as i64, 0i64];
    let mut out0: i64 = 0;
    let status0 = unsafe { jit_fn(args0.as_ptr(), args0.len(), &mut out0) };
    assert_eq!(status0, 0);
    assert_eq!(out0, 42);

    // Out-of-bounds index 5 -> should bailout with status 1
    let args_oob = [buf.as_ptr() as i64, 5i64];
    let mut out_oob: i64 = 0;
    let status_oob = unsafe { jit_fn(args_oob.as_ptr(), args_oob.len(), &mut out_oob) };
    assert_eq!(status_oob, 1);

    // Negative index -1 -> unsigned comparison catches it, bailouts with status 1
    let args_neg = [buf.as_ptr() as i64, -1i64];
    let mut out_neg: i64 = 0;
    let status_neg = unsafe { jit_fn(args_neg.as_ptr(), args_neg.len(), &mut out_neg) };
    assert_eq!(status_neg, 1);
}

#[test]
fn test_jit_array_vm_integration() {
    let script = r#"
fn sum_arr(arr, len) {
    let mut sum = 0
    let mut i = 0
    while i < len {
        sum = sum + arr[i]
        i = i + 1
    }
    return sum
}

let compiled = jit("compile", sum_arr)
let total = sum_arr([5, 15, 30, 50], 4)

[compiled, total]
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    match res {
        InterpretResult::Ok(Value::Array(arr)) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Int(100));
        }
        _ => panic!("Expected array result, got {:?}", res),
    }
}

#[test]
fn test_jit_array_vm_deopt_fallback_on_oob() {
    let script = r#"
fn get_at(arr, i) {
    return arr[i]
}

jit("compile", get_at)
get_at([10, 20], 99)
"#;
    let program = parse_source(script).unwrap();
    let chunk = Compiler::new().compile_program(&program).unwrap();
    let mut vm = VM::new();

    let res = vm.interpret(chunk);
    // When JIT bails out on index 99, VM fallback executes and generates bounds error
    assert_eq!(res, InterpretResult::RuntimeError);
}

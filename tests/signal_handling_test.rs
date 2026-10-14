use shae::parse_source;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn run_shae(code: &str) -> Value {
    let program = parse_source(code).expect("Parse error in test script");
    let chunk = shae::compiler::Compiler::new()
        .compile_program(&program)
        .expect("Compile error");
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(e) => panic!("Runtime error: {:?}", e),
        InterpretResult::CompileError => panic!("Compile error"),
    }
}

#[test]
fn test_sys_signals_list() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let script = r#"
let sys = use "std:sys"
let sigs = sys.signals()
[
    len(sigs) >= 3,
    sigs[0] == "SIGINT",
    sigs[1] == "SIGTERM",
    sigs[2] == "SIGHUP"
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Failed condition at index {}", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

#[test]
fn test_sys_signal_registration_and_trigger() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let script = r#"
let sys = use "std:sys"
sys.clear_signals()

let log = []

// Register handler that accepts signal name
sys.on_signal("SIGINT", fn(sig) {
    log.push("handler_1:" + sig)
})

// Register second handler for same signal
sys.on_signal("SIGINT", fn(sig) {
    log.push("handler_2:" + sig)
})

// Trigger using normalized aliases
sys.trigger_signal("int")

// Trigger SIGTERM (none registered yet, should return false)
let term_res = sys.trigger_signal("SIGTERM")

[
    len(log) == 2,
    log[0] == "handler_1:SIGINT",
    log[1] == "handler_2:SIGINT",
    term_res == false
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Failed condition at index {}", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

#[test]
fn test_sys_signal_clear() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let script = r#"
let sys = use "std:sys"
sys.clear_signals()

let log = []
sys.on_signal("SIGINT", fn() {
    log.push("int")
})
sys.on_signal("SIGTERM", fn() {
    log.push("term")
})

// Clear only SIGINT
sys.clear_signals("SIGINT")

sys.trigger_signal("SIGINT")
sys.trigger_signal("SIGTERM")

// Clear all signals
sys.clear_signals()
sys.trigger_signal("SIGTERM")

[
    len(log) == 1,
    log[0] == "term"
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Failed condition at index {}", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

#[test]
fn test_sys_graceful_shutdown_pattern() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let script = r#"
let sys = use "std:sys"
sys.clear_signals()

let server = {
    "running": true,
    "active_connections": 5,
    "drained": false
}

sys.on_signal("SIGTERM", fn(sig) {
    server.running = false
    server.active_connections = 0
    server.drained = true
})

// Simulate receiving SIGTERM
sys.trigger_signal("term")

[
    server.running == false,
    server.active_connections == 0,
    server.drained == true
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Failed condition at index {}", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

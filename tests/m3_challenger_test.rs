use serde_json::json;
use shae::{run, run_vm};
use std::fs;

fn get_thread_count() -> usize {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("Threads:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(count) = parts[1].parse::<usize>() {
                        return count;
                    }
                }
            }
        }
    }
    1
}

// =========================================================================
// 1. Destructuring Import Parity: AST vs VM
// =========================================================================

#[test]
fn test_destructuring_import_let_syntax() {
    let script = r#"
let { spawn, ask, supervisor, send } = use "std:actor"
let p = spawn(0, fn(state, msg) {
    if msg == "inc" {
        return state + 1
    } else if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state
    }
})
send(p, "inc")
send(p, "inc")
let res = ask(p, "get")
p.stop()
res
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(2));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(2));
}

#[test]
fn test_destructuring_import_use_syntax() {
    let script = r#"
use { spawn, ask, supervisor, send } from "std:actor"
let p = spawn(10, fn(state, msg) {
    if msg == "double" {
        return state * 2
    } else if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state
    }
})
send(p, "double")
let res = ask(p, "get")
p.stop()
res
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(20));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(20));
}

// =========================================================================
// 2. High Message Throughput in AST and VM
// =========================================================================

#[test]
fn test_high_message_throughput_ast() {
    let script = r#"
let actor = use "std:actor"
let counter = actor.spawn(0, fn(state, msg) {
    if msg == "inc" {
        return state + 1
    } else if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state
    }
})

let i = 0
while i < 1000 {
    counter.send("inc")
    i = i + 1
}

let final_val = counter.ask("get", 5000)
counter.stop()
final_val
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(1000));
}

#[test]
fn test_high_message_throughput_vm() {
    let script = r#"
let actor = use "std:actor"
let counter = actor.spawn(0, fn(state, msg) {
    if msg == "inc" {
        return state + 1
    } else if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state
    }
})

let i = 0
while i < 1000 {
    counter.send("inc")
    i = i + 1
}

let final_val = counter.ask("get", 5000)
counter.stop()
final_val
"#;
    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(1000));
}

// =========================================================================
// 3. Bound Method Safety: Invalid Receivers & Unexpected Arguments
// =========================================================================

#[test]
fn test_bound_methods_unexpected_arguments_ast_and_vm() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })

let errors = []

// send with 0 args
try {
    p.send()
} catch (e) {
    errors.push("send_0_args")
}

// send with 2 args
try {
    p.send(1, 2)
} catch (e) {
    errors.push("send_2_args")
}

// stop with 1 arg
try {
    p.stop("extra")
} catch (e) {
    errors.push("stop_1_arg")
}

// is_alive with 1 arg
try {
    p.is_alive("extra")
} catch (e) {
    errors.push("is_alive_1_arg")
}

// ask with 0 args
try {
    p.ask()
} catch (e) {
    errors.push("ask_0_args")
}

// ask with negative timeout
try {
    p.ask("hello", -10)
} catch (e) {
    errors.push("ask_negative_timeout")
}

// ask with invalid timeout string
try {
    p.ask("hello", "not_a_number")
} catch (e) {
    errors.push("ask_string_timeout")
}

p.stop()
errors
"#;
    let expected = json!([
        "send_0_args",
        "send_2_args",
        "stop_1_arg",
        "is_alive_1_arg",
        "ask_0_args",
        "ask_negative_timeout",
        "ask_string_timeout"
    ]);

    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), expected);

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), expected);
}

#[test]
fn test_supervisor_bound_methods_unexpected_arguments_ast_and_vm() {
    let script = r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 3,
    within_seconds: 5.0,
    children: []
})

let errors = []

// get_child with 0 args
try {
    sup.get_child()
} catch (e) {
    errors.push("get_child_0_args")
}

// get_child with non-string
try {
    sup.get_child(12345)
} catch (e) {
    errors.push("get_child_int")
}

// stop_child with 0 args
try {
    sup.stop_child()
} catch (e) {
    errors.push("stop_child_0_args")
}

// restart_child with non-existent child
try {
    sup.restart_child("does_not_exist")
} catch (e) {
    errors.push("restart_non_existent")
}

// start_child with non-map
try {
    sup.start_child("not_a_map")
} catch (e) {
    errors.push("start_child_not_map")
}

// children with unexpected arg
try {
    sup.children("extra")
} catch (e) {
    errors.push("children_extra_arg")
}

sup.stop()
errors
"#;
    let expected = json!([
        "get_child_0_args",
        "get_child_int",
        "stop_child_0_args",
        "restart_non_existent",
        "start_child_not_map",
        "children_extra_arg"
    ]);

    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), expected);

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), expected);
}

// =========================================================================
// 4. Bound Method Extraction & First-Class Invocation
// =========================================================================

#[test]
fn test_extracted_bound_method_invocation() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(0, fn(state, msg) {
    if msg == "inc" {
        return state + 1
    } else if msg == "read" {
        return { state: state, reply: state }
    } else {
        return state
    }
})

let send_fn = p.send
let ask_fn = p.ask
let stop_fn = p.stop

send_fn("inc")
send_fn("inc")
let result = ask_fn("read")
let stopped = stop_fn()

[result, stopped]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([2, true]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([2, true]));
}

// =========================================================================
// 5. Deep Supervisor Trees
// =========================================================================

#[test]
fn test_deep_supervisor_tree_hierarchy() {
    let script = r#"
let actor = use "std:actor"

// Root supervisor manages Child A and a Sub-Supervisor (B)
let root_sup = actor.supervisor({
    id: "root_sup",
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "leaf_worker_1",
            state: 10,
            start: fn(st, msg) {
                if msg == "read" {
                    return { state: st, reply: st }
                } else {
                    return st + msg
                }
            }
        }
    ]
})

// Sub-supervisor manages leaf_worker_2
let sub_sup = actor.supervisor({
    id: "sub_sup",
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "leaf_worker_2",
            state: 100,
            start: fn(st, msg) {
                if msg == "read" {
                    return { state: st, reply: st }
                } else {
                    return st + msg
                }
            }
        }
    ]
})

let w1 = root_sup.get_child("leaf_worker_1")
let w2 = sub_sup.get_child("leaf_worker_2")

w1.send(5)
w2.send(50)

let v1 = w1.ask("read")
let v2 = w2.ask("read")

// Clean cascade shutdown
sub_sup.stop()
root_sup.stop()

let w1_alive = w1.is_alive()
let w2_alive = w2.is_alive()

[v1, v2, w1_alive, w2_alive]
"#;
    let expected = json!([15, 150, false, false]);

    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), expected);

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), expected);
}

// =========================================================================
// 6. Rapid Child Crashes: OneForOne vs OneForAll
// =========================================================================

#[test]
fn test_rapid_child_crashes_one_for_all_sibling_restart() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "all_for_one",
    max_restarts: 10,
    within_seconds: 10.0,
    children: [
        {
            id: "w1",
            state: 100,
            start: fn(st, msg) {
                if msg == "read" {
                    return { state: st, reply: st }
                } else if msg == "fail" {
                    return 1 / 0
                } else {
                    return st + msg
                }
            }
        },
        {
            id: "w2",
            state: 200,
            start: fn(st, msg) {
                if msg == "read" {
                    return { state: st, reply: st }
                } else {
                    return st + msg
                }
            }
        }
    ]
})

let w1 = sup.get_child("w1")
let w2 = sup.get_child("w2")

// Mutate w2 state
w2.send(50)
let w2_val_before = w2.ask("read")

// Crash w1 -> under one_for_all, w2 should also be restarted back to 200!
w1.send("fail")

// Wait briefly for supervisor recovery
let timer = channel(1)
timer.recv(200)

let w2_restarted = sup.get_child("w2")
let w2_val_after = w2_restarted.ask("read")

sup.stop()
[w2_val_before, w2_val_after]
"#;
    // Before crash: 200 + 50 = 250.
    // After one_for_all sibling restart: resets back to initial state 200.
    let expected = json!([250, 200]);

    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), expected);

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), expected);
}

// =========================================================================
// 7. OS Thread Leak Immunity on Rapid Spawn/Stop Cycles
// =========================================================================

#[test]
fn test_rapid_spawn_stop_no_thread_leak_shae() {
    let initial_threads = get_thread_count();

    let script = r#"
let actor = use "std:actor"
let i = 0
while i < 100 {
    let p = actor.spawn(fn(msg) { msg })
    p.stop()
    i = i + 1
}
i
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(100));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(100));

    // Allow thread cleanup
    std::thread::sleep(std::time::Duration::from_millis(150));
    let final_threads = get_thread_count();

    // Verify thread count difference is within acceptable tolerance (no 200 leaked threads)
    assert!(
        final_threads <= initial_threads + 3,
        "Leaked OS threads: initial={}, final={}",
        initial_threads,
        final_threads
    );
}

// =========================================================================
// 8. AST Evaluator Recursion Depth Leak Verification
// =========================================================================

#[test]
fn test_evaluator_depth_leak_on_repeated_errors() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.stop()

let count = 0
let i = 0
while i < 2500 {
    try {
        p.send(1)
    } catch (e) {
        count = count + 1
    }
    i = i + 1
}
count
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(2500));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(2500));
}

// =========================================================================
// 9. All 13 std:actor Module Exports Destructuring Parity
// =========================================================================

#[test]
fn test_destructuring_all_13_exports_ast_and_vm() {
    let script = r#"
let {
    spawn,
    spawn_named,
    send,
    ask,
    stop,
    is_alive,
    mailbox_len,
    self_pid,
    supervisor,
    register,
    whereis,
    unregister,
    registered
} = use "std:actor"

let is_fn_list = [
    type(spawn) == "builtin_function",
    type(spawn_named) == "builtin_function",
    type(send) == "builtin_function",
    type(ask) == "builtin_function",
    type(stop) == "builtin_function",
    type(is_alive) == "builtin_function",
    type(mailbox_len) == "builtin_function",
    type(self_pid) == "builtin_function",
    type(supervisor) == "builtin_function",
    type(register) == "builtin_function",
    type(whereis) == "builtin_function",
    type(unregister) == "builtin_function",
    type(registered) == "builtin_function"
]
is_fn_list
"#;
    let expected = json!([
        true, true, true, true, true, true, true, true, true, true, true, true, true
    ]);

    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), expected);

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), expected);
}

// =========================================================================
// 10. Synthetic Corrupted BoundMethod Handling
// =========================================================================

#[test]
fn test_synthetic_corrupted_bound_method_receiver_and_args() {
    use shae::ast::Span;
    use shae::eval::Evaluator;
    use shae::value::Value;

    let span = Span::new(1, 1);

    // Corrupted receiver: Int instead of Actor.
    // Previously in src/eval.rs:3532, the fallback arm `_ =>` executed `self.depth -= 1`
    // and then line 3542 also executed `self.depth -= 1` (double decrement).
    // Now fixed: double decrement is eliminated, returning clean RuntimeError without panicking.
    let bad_callee = Value::BoundMethod {
        object: Box::new(Value::Int(42)),
        method: "send".to_string(),
    };
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut ev_inner = Evaluator::new();
        let res = ev_inner.call_value(&bad_callee, vec![Value::Int(10)], span);
        assert!(
            res.is_err(),
            "Expected RuntimeError for invalid bound method receiver"
        );
    }));

    assert!(
        panic_result.is_ok(),
        "Must not panic with double decrement underflow at depth 0"
    );
}

// =========================================================================
// 11. Multi-Threaded Concurrent Send Hammer (5,000 Messages)
// =========================================================================

#[test]
fn test_multithreaded_concurrent_send_hammer() {
    let script = r#"
let actor = use "std:actor"
let counter = actor.spawn(0, fn(state, msg) {
    if msg == "inc" {
        return state + 1
    } else if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state
    }
})
counter
"#;
    let ast_val = run(script).expect("Failed to spawn counter");
    let actor_ref = match ast_val {
        shae::value::Value::Actor(act) => act,
        _ => panic!("Expected Value::Actor"),
    };

    let threads = 20;
    let msgs_per_thread = 250;
    let mut handles = Vec::new();

    for _ in 0..threads {
        let act_clone = actor_ref.clone();
        handles.push(std::thread::spawn(move || {
            for _ in 0..msgs_per_thread {
                act_clone
                    .send(shae::value::Value::String("inc".into()))
                    .unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let final_val = actor_ref
        .ask(
            shae::value::Value::String("get".into()),
            Some(std::time::Duration::from_secs(5)),
        )
        .expect("Failed to ask counter");

    actor_ref.stop();
    assert_eq!(final_val.to_json(), json!(5000));
}

// =========================================================================
// 12. Multi-Threaded Concurrent Ask Hammer (500 Requests)
// =========================================================================

#[test]
fn test_multithreaded_concurrent_ask_hammer() {
    let script = r#"
let actor = use "std:actor"
let echo = actor.spawn(fn(msg) { msg * 2 })
echo
"#;
    let ast_val = run(script).expect("Failed to spawn echo");
    let actor_ref = match ast_val {
        shae::value::Value::Actor(act) => act,
        _ => panic!("Expected Value::Actor"),
    };

    let threads = 10;
    let requests_per_thread = 50;
    let mut handles = Vec::new();

    for t in 0..threads {
        let act_clone = actor_ref.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..requests_per_thread {
                let req_val = (t * 1000 + i) as i64;
                let reply = act_clone
                    .ask(
                        shae::value::Value::Int(req_val),
                        Some(std::time::Duration::from_secs(5)),
                    )
                    .expect("Concurrent ask failed");
                assert_eq!(reply.to_json(), json!(req_val * 2));
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    actor_ref.stop();
}

#[test]
fn test_shae_script_bound_method_unmatched_panic() {
    let script = r#"
let m = { a: 1 }
m.union({})
"#;
    let res = std::panic::catch_unwind(|| run(script));
    println!("AST run result: {:?}", res);

    let vm_res = std::panic::catch_unwind(|| run_vm(script));
    println!("VM run result: {:?}", vm_res);
}

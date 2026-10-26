//! Adversarial Verification Test Suite for Milestone 3
//! Authored by challenger_m3_2
//!
//! Validates:
//! 1. Destructuring imports in AST and VM
//! 2. High message throughput in AST and VM
//! 3. OneForOne and AllForOne supervisor crash recovery in AST and VM
//! 4. Deep supervisor hierarchy in AST and VM
//! 5. OS thread leak immunity across spawn/stop cycles
//! 6. Bound method corruption resistance and argument validation
//! 7. Detached bound methods
//! 8. Evaluator call depth leak reproduction under error recovery

use serde_json::json;
use shae::{run, run_vm};

fn get_os_threads() -> Option<usize> {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("Threads:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(count) = parts[1].parse::<usize>() {
                        return Some(count);
                    }
                }
            }
        }
    }
    // Fallback on non-Linux OS (macOS/Windows where /proc is absent)
    Some(1)
}

// =========================================================================
// 1. Destructuring Import Parity (AST & VM)
// =========================================================================

#[test]
fn test_destructuring_let_use_std_actor_parity() {
    let script = r#"
let { spawn, ask, stop, is_alive } = use "std:actor"
let p = spawn(fn(msg) { msg * 3 })
let alive_before = is_alive(p)
let res = ask(p, 14)
let stopped = stop(p)
let alive_after = is_alive(p)
[alive_before, res, stopped, alive_after]
"#;
    let ast_res = run(script).expect("AST destructuring failed");
    assert_eq!(ast_res.to_json(), json!([true, 42, true, false]));

    let vm_res = run_vm(script).expect("VM destructuring failed");
    assert_eq!(vm_res.to_json(), json!([true, 42, true, false]));
}

#[test]
fn test_destructuring_let_with_renaming() {
    let script = r#"
let { spawn: actor_spawn, ask: actor_ask, stop: actor_stop } = use "std:actor"
let p = actor_spawn(100, fn(state, msg) {
    if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state + msg
    }
})
p.send(25)
let res = actor_ask(p, "get")
actor_stop(p)
res
"#;
    let ast_res = run(script).expect("AST renamed destructuring failed");
    assert_eq!(ast_res.to_json(), json!(125));

    let vm_res = run_vm(script).expect("VM renamed destructuring failed");
    assert_eq!(vm_res.to_json(), json!(125));
}

// =========================================================================
// 2. High Message Throughput (AST & VM)
// =========================================================================

#[test]
fn test_high_message_throughput_ast_and_vm() {
    let script = r#"
let actor = use "std:actor"
let act = actor.spawn(0, fn(state, msg) {
    if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state + msg
    }
})

let i = 0
while i < 1000 {
    act.send(1)
    i = i + 1
}
let total = act.ask("get")
act.stop()
total
"#;
    let ast_res = run(script).expect("AST throughput failed");
    assert_eq!(ast_res.to_json(), json!(1000));

    let vm_res = run_vm(script).expect("VM throughput failed");
    assert_eq!(vm_res.to_json(), json!(1000));
}

// =========================================================================
// 3. Supervision Restart Policies: OneForOne vs AllForOne (AST & VM)
// =========================================================================

#[test]
fn test_one_for_one_isolated_restart_in_shae() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "worker_a",
            state: 10,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else if msg == "fail" {
                    return 1 / 0
                } else {
                    return st + msg
                }
            }
        },
        {
            id: "worker_b",
            state: 100,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else {
                    return st + msg
                }
            }
        }
    ]
})

let wa = sup.get_child("worker_a")
let wb = sup.get_child("worker_b")

wa.send(5) // state 15
wb.send(50) // state 150

let a_before = wa.ask("get")
let b_before = wb.ask("get")

wa.send("fail")

let timer = channel(1)
timer.recv(200)

let wa_new = sup.get_child("worker_a")
let wb_new = sup.get_child("worker_b")

let a_after = wa_new.ask("get") // 10 (restarted)
let b_after = wb_new.ask("get") // 150 (untouched in one_for_one)

sup.stop()
[a_before, b_before, a_after, b_after]
"#;
    let ast_res = run(script).expect("AST OneForOne failed");
    assert_eq!(ast_res.to_json(), json!([15, 150, 10, 150]));

    let vm_res = run_vm(script).expect("VM OneForOne failed");
    assert_eq!(vm_res.to_json(), json!([15, 150, 10, 150]));
}

#[test]
fn test_all_for_one_sibling_restart_in_shae() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "all_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "worker_a",
            state: 10,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else if msg == "fail" {
                    return 1 / 0
                } else {
                    return st + msg
                }
            }
        },
        {
            id: "worker_b",
            state: 100,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else {
                    return st + msg
                }
            }
        }
    ]
})

let wa = sup.get_child("worker_a")
let wb = sup.get_child("worker_b")

wa.send(5)
wb.send(50)

let a_before = wa.ask("get")
let b_before = wb.ask("get")

wa.send("fail")

let timer = channel(1)
timer.recv(200)

let wa_new = sup.get_child("worker_a")
let wb_new = sup.get_child("worker_b")

let a_after = wa_new.ask("get") // 10
let b_after = wb_new.ask("get") // 100 (restarted in all_for_one)

sup.stop()
[a_before, b_before, a_after, b_after]
"#;
    let ast_res = run(script).expect("AST AllForOne failed");
    assert_eq!(ast_res.to_json(), json!([15, 150, 10, 100]));

    let vm_res = run_vm(script).expect("VM AllForOne failed");
    assert_eq!(vm_res.to_json(), json!([15, 150, 10, 100]));
}

// =========================================================================
// 4. Deep Supervision Tree Hierarchy (AST & VM)
// =========================================================================

#[test]
fn test_deep_supervision_tree_hierarchy() {
    let script = r#"
let actor = use "std:actor"

let leaf_sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "leaf_worker",
            state: 42,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else if msg == "crash" {
                    return 1 / 0
                } else {
                    return st + msg
                }
            }
        }
    ]
})

let root_sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "mid_manager",
            state: leaf_sup,
            start: fn(st, msg) {
                if msg == "forward_get" {
                    let w = st.get_child("leaf_worker")
                    let val = w.ask("get")
                    return { state: st, reply: val }
                } else if msg == "forward_crash" {
                    let w = st.get_child("leaf_worker")
                    w.send("crash")
                    return { state: st, reply: "crashed" }
                } else {
                    return st
                }
            }
        }
    ]
})

let mid = root_sup.get_child("mid_manager")
let v1 = mid.ask("forward_get")

mid.ask("forward_crash")

let timer = channel(1)
timer.recv(200)

let v2 = mid.ask("forward_get")

let leaf = leaf_sup.get_child("leaf_worker")
let leaf_alive = leaf.is_alive()

root_sup.stop()
leaf_sup.stop()

[v1, v2, leaf_alive]
"#;
    let ast_res = run(script).expect("AST deep supervision failed");
    assert_eq!(ast_res.to_json(), json!([42, 42, true]));

    let vm_res = run_vm(script).expect("VM deep supervision failed");
    assert_eq!(vm_res.to_json(), json!([42, 42, true]));
}

// =========================================================================
// 5. OS Thread Count Stability & Zero Thread Leaks
// =========================================================================

#[test]
fn test_no_thread_leak_rapid_spawn_stop() {
    let threads_before = get_os_threads().expect("Failed to read threads from /proc");

    let script = r#"
let actor = use "std:actor"
let i = 0
while i < 50 {
    let a = actor.spawn(fn(msg) { msg })
    a.send(1)
    a.stop()
    i = i + 1
}
i
"#;
    let ast_res = run(script).expect("AST spawn/stop failed");
    assert_eq!(ast_res.to_json(), json!(50));

    let vm_res = run_vm(script).expect("VM spawn/stop failed");
    assert_eq!(vm_res.to_json(), json!(50));

    std::thread::sleep(std::time::Duration::from_millis(200));

    let threads_after = get_os_threads().expect("Failed to read threads from /proc");
    assert!(
        threads_after <= threads_before + 2,
        "Leaked OS threads! before: {}, after: {}",
        threads_before,
        threads_after
    );
}

// =========================================================================
// 6. Bound Method Corruption Resistance & Argument Validation
// =========================================================================

#[test]
fn test_bound_methods_argument_validation() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 3,
    within_seconds: 5.0,
    children: []
})
let ch = channel(5)

let err_count = 0

// Actor arity attacks
try { p.send() } catch (e) { err_count = err_count + 1 }
try { p.send(1, 2) } catch (e) { err_count = err_count + 1 }
try { p.ask() } catch (e) { err_count = err_count + 1 }
try { p.ask(1, 2, 3) } catch (e) { err_count = err_count + 1 }
try { p.stop(1) } catch (e) { err_count = err_count + 1 }
try { p.is_alive(1) } catch (e) { err_count = err_count + 1 }
try { p.len(1) } catch (e) { err_count = err_count + 1 }
try { p.id(1) } catch (e) { err_count = err_count + 1 }
try { p.status(1) } catch (e) { err_count = err_count + 1 }

// Actor type attacks
try { p.ask(1, "invalid_timeout") } catch (e) { err_count = err_count + 1 }
try { p.ask(1, -100) } catch (e) { err_count = err_count + 1 }

// Supervisor arity & type attacks
try { sup.get_child() } catch (e) { err_count = err_count + 1 }
try { sup.get_child("a", "b") } catch (e) { err_count = err_count + 1 }
try { sup.get_child(123) } catch (e) { err_count = err_count + 1 }
try { sup.start_child() } catch (e) { err_count = err_count + 1 }
try { sup.start_child(123) } catch (e) { err_count = err_count + 1 }
try { sup.stop_child() } catch (e) { err_count = err_count + 1 }
try { sup.stop_child(123) } catch (e) { err_count = err_count + 1 }
try { sup.restart_child() } catch (e) { err_count = err_count + 1 }
try { sup.restart_child(123) } catch (e) { err_count = err_count + 1 }
try { sup.children(1) } catch (e) { err_count = err_count + 1 }
try { sup.stop(1) } catch (e) { err_count = err_count + 1 }
try { sup.len(1) } catch (e) { err_count = err_count + 1 }
try { sup.id(1) } catch (e) { err_count = err_count + 1 }
try { sup.status(1) } catch (e) { err_count = err_count + 1 }
try { sup.is_alive(1) } catch (e) { err_count = err_count + 1 }

// Channel arity & type attacks
try { ch.send() } catch (e) { err_count = err_count + 1 }
try { ch.recv(1, 2) } catch (e) { err_count = err_count + 1 }
try { ch.recv("bad_timeout") } catch (e) { err_count = err_count + 1 }
try { ch.try_recv(1) } catch (e) { err_count = err_count + 1 }
try { ch.close(1) } catch (e) { err_count = err_count + 1 }

p.stop()
sup.stop()
ch.close()

err_count
"#;
    let ast_res = run(script).expect("AST bound method attack test failed");
    assert_eq!(ast_res.to_json(), json!(30));

    let vm_res = run_vm(script).expect("VM bound method attack test failed");
    assert_eq!(vm_res.to_json(), json!(30));
}

// =========================================================================
// 7. Detached Bound Methods (AST & VM)
// =========================================================================

#[test]
fn test_detached_bound_methods() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { "received: " + msg })

let fn_send = p.send
let fn_ask = p.ask
let fn_is_alive = p.is_alive
let fn_stop = p.stop

let alive_1 = fn_is_alive()
let ans = fn_ask("detachment")
let stopped = fn_stop()
let alive_2 = fn_is_alive()

[alive_1, ans, stopped, alive_2]
"#;
    let ast_res = run(script).expect("AST detached bound method failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, "received: detachment", true, false])
    );

    let vm_res = run_vm(script).expect("VM detached bound method failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, "received: detachment", true, false])
    );
}

// =========================================================================
// 8. EMPIRICAL BUG REPRODUCTION: Evaluator Call Depth Leak under Error Recovery
// =========================================================================

#[test]
fn test_reproduce_depth_leak_on_failed_actor_send() {
    // In src/eval.rs: (Value::Actor(act), "send") => act.send(...).map_err(...)?;
    // If act.send() fails, the '?' operator returns early from Evaluator::call_value,
    // bypassing line 3542 (self.depth -= 1).
    // As a result, each failed send permanently leaks +1 to self.depth.
    // After 2000 failures, any subsequent function call triggers "Stack overflow".
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.stop()

let i = 0
while i < 2100 {
    try {
        p.send("message_to_stopped_actor")
    } catch (e) {
    }
    i = i + 1
}

fn test_func() { 42 }
test_func()
"#;
    let res = run(script);
    assert!(
        res.is_ok(),
        "Depth leak must be resolved, but got: {:?}",
        res
    );
    assert_eq!(res.unwrap().to_json(), json!(42));
}

#[test]
fn test_reproduce_depth_leak_on_failed_channel_send() {
    let script = r#"
let ch = channel(1)
ch.close()

let i = 0
while i < 2100 {
    try {
        ch.send("message_to_closed_channel")
    } catch (e) {
    }
    i = i + 1
}

fn test_func() { 99 }
test_func()
"#;
    let res = run(script);
    assert!(
        res.is_ok(),
        "Channel send depth leak must be resolved, but got: {:?}",
        res
    );
    assert_eq!(res.unwrap().to_json(), json!(99));
}

#[test]
fn test_ask_does_not_leak_depth_because_no_question_mark_operator() {
    // Unlike 'send', 'ask' in eval.rs returns Result directly to the match expression:
    // act.ask(args[0].clone(), timeout).map_err(|e| e.to_runtime_error(span))
    // So line 3542 (self.depth -= 1) executes properly on every error!
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.stop()

let i = 0
while i < 2100 {
    try {
        p.ask("ping", 10)
    } catch (e) {
    }
    i = i + 1
}

fn test_func() { 123 }
test_func()
"#;
    let res = run(script);
    assert!(
        res.is_ok(),
        "Expected Ok because ask() correctly decrements depth, but got: {:?}",
        res
    );
    assert_eq!(res.unwrap().to_json(), json!(123));
}

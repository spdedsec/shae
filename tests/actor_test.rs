use serde_json::json;
use shae::{run, run_vm};

// =========================================================================
// 1. Module Import Tests
// =========================================================================

#[test]
fn test_import_std_actor_module() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg * 2 })
let res = actor.ask(p, 21)
p.stop()
res
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(42));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(42));
}

#[test]
fn test_use_destructuring_import() {
    let script = r#"
use { spawn, send, ask, stop, is_alive } from "std:actor"
let p = spawn(fn(msg) { "hello " + msg })
let alive_before = is_alive(p)
let res = ask(p, "world")
let stopped = stop(p)
let alive_after = is_alive(p)
[alive_before, res, stopped, alive_after]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([true, "hello world", true, false]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, "hello world", true, false]));
}

// =========================================================================
// 2. Stateless & Stateful Actor Lifecycle
// =========================================================================

#[test]
fn test_stateless_actor_spawn_and_ask() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg + 10 })
let a = p.ask(5)
let b = p.ask(20)
p.stop()
[a, b]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([15, 30]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([15, 30]));
}

#[test]
fn test_stateful_actor_counter() {
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

counter.send("inc")
counter.send("inc")
counter.send("inc")
let count = counter.ask("get")
counter.stop()
count
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(3));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(3));
}

// =========================================================================
// 3. Method Syntax on Actor Instances
// =========================================================================

#[test]
fn test_actor_instance_methods() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(100, fn(state, msg) {
    if msg == "read" {
        return { state: state, reply: state }
    } else {
        return state + msg
    }
})

let alive1 = p.is_alive()
let id_str = p.id()
let status1 = p.status()

p.send(50)
let val = p.ask("read")

let stopped = p.stop()
let alive2 = p.is_alive()

[alive1, val, stopped, alive2, status1]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!([true, 150, true, false, "running"])
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, 150, true, false, "running"]));
}

#[test]
fn test_actor_len_and_mailbox_len_methods() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
let l1 = p.len()
let l2 = p.mailbox_len()
p.stop()
[l1, l2]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([0, 0]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([0, 0]));
}

// =========================================================================
// 4. Polymorphic Prelude send() & Interoperability
// =========================================================================

#[test]
fn test_polymorphic_prelude_send_channel_and_actor() {
    let script = r#"
let actor = use "std:actor"

// 1. Channel send
let ch = channel()
send(ch, "from_channel")
let ch_msg = recv(ch)

// 2. Actor send
let act = actor.spawn(0, fn(state, msg) {
    if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state + msg
    }
})
send(act, 10)
send(act, 25)
let act_val = act.ask("get")
act.stop()

[ch_msg, act_val]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(["from_channel", 35]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(["from_channel", 35]));
}

#[test]
fn test_polymorphic_send_to_registered_name() {
    let script = r#"
let actor = use "std:actor"
actor.unregister("named_poly_box")

let act = actor.spawn_named("named_poly_box", 0, fn(state, msg) {
    if msg == "get" {
        return { state: state, reply: state }
    } else {
        return state + msg
    }
})

// Send via registered string name through prelude send()
send("named_poly_box", 100)
send("named_poly_box", 200)

let total = act.ask("get")
actor.unregister("named_poly_box")
act.stop()

total
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(300));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(300));
}

// =========================================================================
// 5. Global Registry Operations
// =========================================================================

#[test]
fn test_actor_registry_crud_operations() {
    let script = r#"
let actor = use "std:actor"
actor.unregister("service_crud")

let p = actor.spawn(fn(msg) { "echo:" + msg })
actor.register("service_crud", p)

let resolved = actor.whereis("service_crud")
let ans = resolved.ask("test")

let reg_list = actor.registered()
let has_service_crud = false
for name in reg_list {
    if name == "service_crud" {
        has_service_crud = true
    }
}

actor.unregister("service_crud")
let after_unreg = actor.whereis("service_crud")
p.stop()

[ans, has_service_crud, after_unreg]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(["echo:test", true, null]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(["echo:test", true, null]));
}

// =========================================================================
// 6. Supervision Tree Creation & Instance Methods
// =========================================================================

#[test]
fn test_supervisor_basic_creation_and_methods() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 3,
    within_seconds: 5.0,
    children: [
        {
            id: "worker_1",
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

let sup_alive = sup.is_alive()
let sup_len = sup.len()

let w1 = sup.get_child("worker_1")
w1.send(5)
let val = w1.ask("read")

let kids = sup.children()
let k_id = kids[0]["id"]
let k_status = kids[0]["status"]

sup.stop()
let sup_alive_after = sup.is_alive()

[sup_alive, sup_len, val, k_id, k_status, sup_alive_after]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!([true, 1, 15, "worker_1", "running", false])
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        json!([true, 1, 15, "worker_1", "running", false])
    );
}

#[test]
fn test_supervisor_dynamic_child_lifecycle() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: []
})

let len0 = sup.len()

// Dynamically start a child
let dyn_child = sup.start_child({
    id: "dyn_worker",
    state: 100,
    start: fn(st, msg) {
        if msg == "peek" {
            return { state: st, reply: st }
        } else {
            return st + msg
        }
    }
})

let len1 = sup.len()
dyn_child.send(50)
let v1 = dyn_child.ask("peek")

// Restart child
let restarted = sup.restart_child("dyn_worker")
let v2 = restarted.ask("peek") // reset to initial state 100

// Stop child
sup.stop_child("dyn_worker")
let len2 = sup.len()
let missing = sup.get_child("dyn_worker")

sup.stop()
[len0, len1, v1, v2, len2, missing]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([0, 1, 150, 100, 0, null]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([0, 1, 150, 100, 0, null]));
}

#[test]
fn test_supervisor_one_for_one_crash_recovery() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "safe_worker",
            state: 0,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else if msg == "fail" {
                    return 1 / 0
                } else {
                    return st + msg
                }
            }
        }
    ]
})

let w = sup.get_child("safe_worker")
w.send(10)
let before_crash = w.ask("get")

// Crash the worker by sending "fail"
w.send("fail")

// Sleep briefly using channel timeout on an empty channel to allow supervisor restart
let timer = channel(1)
timer.recv(150)

// Retrieve the restarted child
let w_restarted = sup.get_child("safe_worker")
let after_crash_alive = w_restarted.is_alive()
let after_crash_state = w_restarted.ask("get")

sup.stop()
[before_crash, after_crash_alive, after_crash_state]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([10, true, 0]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([10, true, 0]));
}

// =========================================================================
// 7. Deadlock Prevention & Error Handling
// =========================================================================

#[test]
fn test_self_ask_deadlock_detection() {
    let script = r#"
let actor = use "std:actor"

let act = actor.spawn(fn(msg) {
    if msg == "ask_self" {
        let me = actor.self_pid()
        me.ask("ping")
    } else {
        "pong"
    }
})

let deadlock_caught = false
try {
    act.ask("ask_self", 1000)
} catch (e) {
    deadlock_caught = true
}

act.stop()
deadlock_caught
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(true));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(true));
}

// =========================================================================
// 8. Channel Bound Methods in VM
// =========================================================================

#[test]
fn test_channel_bound_methods_in_vm() {
    let script = r#"
let ch = channel(5)
ch.send(10)
ch.send(20)
let l = ch.len()
let cap = ch.capacity()
let r1 = ch.recv()
let r2 = ch.recv()
ch.close()
[l, cap, r1, r2]
"#;
    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([2, 5, 10, 20]));
}

// =========================================================================
// 9. Call Depth Leak Prevention on Caught Errors
// =========================================================================

#[test]
fn test_actor_supervisor_no_call_depth_leak_on_caught_errors() {
    let script = r#"
let actor = use "std:actor"

let p = actor.spawn(fn(msg) { msg })
p.stop()

let sup = actor.supervisor({
    strategy: "one_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: []
})

let caught_actor_errs = 0
let i = 0
while i < 1025 {
    try {
        p.send(1)
    } catch (e) {
        if e != "Stack overflow: maximum call depth exceeded" {
            caught_actor_errs = caught_actor_errs + 1
        }
    }
    i = i + 1
}

let caught_sup_errs = 0
let j = 0
while j < 1025 {
    try {
        sup.restart_child("non_existent_child")
    } catch (e) {
        if e != "Stack overflow: maximum call depth exceeded" {
            caught_sup_errs = caught_sup_errs + 1
        }
    }
    j = j + 1
}

sup.stop()

// Crucial check: calling a user function must succeed and not trigger false stack overflow
let probe = fn(x) { x + 10 }
let final_call = probe(32)

[caught_actor_errs, caught_sup_errs, final_call]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([1025, 1025, 42]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([1025, 1025, 42]));
}

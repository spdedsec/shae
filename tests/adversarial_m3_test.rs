//! Adversarial Verification Test Suite for Milestone 3:
//! Dual-Engine Runtime & Stdlib Integration (AST Evaluator & Bytecode VM)

use serde_json::json;
use shae::actor::{ActorRef, ActorRegistry};
use shae::{run, run_vm};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

// Helper to assert that a script fails on both AST and VM engines with an expected message fragment
fn assert_both_fail_with(script: &str, expected_substring: &str) {
    let ast_res = run(script);
    assert!(
        ast_res.is_err(),
        "AST engine unexpectedly succeeded on script:\n{}\nResult: {:?}",
        script,
        ast_res.ok()
    );
    let ast_err = ast_res.unwrap_err().to_string();
    assert!(
        ast_err.contains(expected_substring),
        "AST error '{}' does not contain expected substring '{}'",
        ast_err,
        expected_substring
    );

    let vm_res = run_vm(script);
    assert!(
        vm_res.is_err(),
        "VM engine unexpectedly succeeded on script:\n{}\nResult: {:?}",
        script,
        vm_res.ok()
    );
    let vm_err = vm_res.unwrap_err().to_string();
    assert!(
        vm_err.contains(expected_substring),
        "VM error '{}' does not contain expected substring '{}'",
        vm_err,
        expected_substring
    );
}

// Helper to assert that a script succeeds on both AST and VM with identical JSON result
fn assert_both_succeed_eq(script: &str, expected: serde_json::Value) {
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        expected,
        "AST result mismatch for script:\n{}",
        script
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        expected,
        "VM result mismatch for script:\n{}",
        script
    );
}

// =========================================================================
// 1. Dual-Engine Actor Method Arity and Type Enforcement
// =========================================================================

#[test]
fn test_adversarial_actor_method_invalid_args() {
    // send() expects exactly 1 argument
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.send()
"#,
        "send() expects 1 argument",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.send(1, 2)
"#,
        "send() expects 1 argument",
    );

    // ask() expects 1 or 2 arguments
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask()
"#,
        "ask() expects 1 or 2 arguments",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask(1, 2, 3)
"#,
        "ask() expects 1 or 2 arguments",
    );

    // ask() timeout negative or invalid type
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask("test", -50)
"#,
        "ask() timeout must be a non-negative number",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask("test", "five_seconds")
"#,
        "ask() timeout must be a non-negative number",
    );

    // stop() expects 0 arguments
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.stop("extra")
"#,
        "stop() expects 0 arguments",
    );

    // is_alive() expects 0 arguments
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.is_alive(123)
"#,
        "is_alive() expects 0 arguments",
    );

    // mailbox_len() / len() expects 0 arguments
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.mailbox_len(123)
"#,
        "mailbox_len() expects 0 arguments",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.len(123)
"#,
        "len() expects 0 arguments",
    );

    // Non-existent actor method access
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.non_existent_method()
"#,
        "Actor has no property or method 'non_existent_method'",
    );
}

// =========================================================================
// 2. Stdlib Module Function Invalid Arguments
// =========================================================================

#[test]
fn test_adversarial_stdlib_actor_functions_invalid_args() {
    // spawn invalid args
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.spawn()
"#,
        "spawn() expects 1 or 2 arguments",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.spawn(1, 2, 3)
"#,
        "spawn() expects 1 or 2 arguments",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.spawn("not_a_function")
"#,
        "spawn() handler must be a function",
    );

    // spawn_named invalid args
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.spawn_named(123, fn(m) { m })
"#,
        "spawn_named() first argument must be a string name",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.spawn_named("worker", "not_a_function")
"#,
        "spawn_named() handler must be a function",
    );

    // send invalid target
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.send(12345, "hello")
"#,
        "send expects an actor, channel, or registered actor name",
    );

    // ask invalid target
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.ask(12345, "hello")
"#,
        "ask expects an actor or registered actor name",
    );

    // register invalid args
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.register(123, 456)
"#,
        "register() name must be a string",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.register("valid_name", 456)
"#,
        "register() second argument must be an actor",
    );
}

// =========================================================================
// 3. Send to Unregistered String Name
// =========================================================================

#[test]
fn test_adversarial_send_to_unregistered_name() {
    // Via std:actor send()
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.send("ghost_actor_unknown_999", "ping")
"#,
        "not found",
    );

    // Via std:actor ask()
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.ask("ghost_actor_unknown_888", "ping")
"#,
        "not found",
    );

    // Via polymorphic prelude send()
    assert_both_fail_with(
        r#"
send("ghost_actor_prelude_777", "ping")
"#,
        "Actor name not found: ghost_actor_prelude_777",
    );
}

// =========================================================================
// 4. Ask Timeout Behavior
// =========================================================================

#[test]
fn test_adversarial_ask_timeout_behavior() {
    let script = r#"
let actor = use "std:actor"
let time = use "std:time"

let slow_worker = actor.spawn(fn(msg) {
    if msg == "sleep" {
        time.sleep(250)
        return "woke_up"
    } else if msg == "ping" {
        return "pong"
    }
    return "ok"
})

// Ask with 50ms timeout when worker will take 250ms
let timed_out = false
try {
    slow_worker.ask("sleep", 50)
} catch err {
    if err.contains("timed out") {
        timed_out = true
    }
}

// Wait until worker finishes sleeping
time.sleep(300)

// Verify worker is still alive and responsive after timeout!
let post_ping = slow_worker.ask("ping", 1000)
let alive = slow_worker.is_alive()
slow_worker.stop()

[timed_out, post_ping, alive]
"#;

    assert_both_succeed_eq(script, json!([true, "pong", true]));
}

// =========================================================================
// 5. Self-Ask Deadlock Prevention
// =========================================================================

#[test]
fn test_adversarial_self_ask_deadlock_prevention() {
    let script = r#"
let actor = use "std:actor"

let deadlocker = actor.spawn(fn(msg) {
    if msg == "trigger_deadlock" {
        let me = actor.self_pid()
        // Synchronously asking oneself would hang forever without detection
        let res = me.ask("recursive", 2000)
        return "got: " + res
    }
    return "reply"
})

let caught_error = false
let err_msg = ""
try {
    deadlocker.ask("trigger_deadlock", 2000)
} catch err {
    caught_error = true
    err_msg = err
}

deadlocker.stop()
[caught_error, err_msg.contains("Deadlock detected")]
"#;

    assert_both_succeed_eq(script, json!([true, true]));
}

// =========================================================================
// 6. Concurrent Calls to register / whereis / unregister
// =========================================================================

#[test]
fn test_adversarial_concurrent_registry_operations() {
    let num_threads = 12;
    let iterations_per_thread = 200;
    let stop_signal = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(AtomicUsize::new(0));

    // Spawn 10 persistent dummy actors
    let actors: Vec<Arc<ActorRef>> = (0..10).map(|_| ActorRef::spawn_fn(|msg| Ok(msg))).collect();
    let actors = Arc::new(actors);

    let mut handles = Vec::new();
    for t_id in 0..num_threads {
        let stop = Arc::clone(&stop_signal);
        let errs = Arc::clone(&errors);
        let actor_pool = Arc::clone(&actors);

        handles.push(thread::spawn(move || {
            for i in 0..iterations_per_thread {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let name = format!("adversarial_actor_{}", (t_id * 10 + i) % 30);
                let actor_idx = (t_id + i) % actor_pool.len();
                let actor = Arc::clone(&actor_pool[actor_idx]);

                // Register
                match ActorRegistry::register(&name, Arc::clone(&actor)) {
                    Ok(()) => {}
                    Err(shae::actor::ActorError::NameAlreadyRegistered(_)) => {
                        // Expected when racing
                    }
                    Err(e) => {
                        eprintln!("Unexpected register error: {:?}", e);
                        errs.fetch_add(1, Ordering::SeqCst);
                    }
                }

                // Whereis
                if let Some(resolved) = ActorRegistry::whereis(&name) {
                    assert!(resolved.is_alive());
                }

                // Registered list
                let list = ActorRegistry::registered();
                assert!(!list.is_empty() || true);

                // Unregister
                ActorRegistry::unregister(&name);
            }
        }));
    }

    for h in handles {
        h.join().expect("Worker thread panicked");
    }

    assert_eq!(
        errors.load(Ordering::SeqCst),
        0,
        "Concurrent registry operations had unexpected errors"
    );

    // Clean up dummy actors
    for a in actors.iter() {
        a.stop();
        let _ = a.join();
    }
}

// =========================================================================
// 7. Supervisor Bound Methods Edge Cases
// =========================================================================

#[test]
fn test_adversarial_supervisor_bound_methods_edge_cases() {
    // 7.1 get_child with non-existent id returns null
    let script_non_existent = r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    children: [
        { id: "child_a", handler: fn(m) { m } }
    ]
})

let missing = sup.get_child("does_not_exist_at_all")
let exists = sup.get_child("child_a")
let exists_is_alive = exists != null && exists.is_alive()
sup.stop()

[missing == null, exists_is_alive]
"#;
    assert_both_succeed_eq(script_non_existent, json!([true, true]));

    // 7.2 get_child invalid argument counts and types
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.get_child()
"#,
        "get_child() expects 1 argument",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.get_child(12345)
"#,
        "get_child() expects string child_id, got int",
    );

    // 7.3 stop_child with non-existent id
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.stop_child("ghost_child")
"#,
        "Child not found with id: ghost_child",
    );

    // stop_child invalid arguments
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.stop_child()
"#,
        "stop_child() expects 1 argument",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.stop_child(999)
"#,
        "stop_child() expects string child_id, got int",
    );

    // 7.4 restart_child with non-existent id
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.restart_child("ghost_child")
"#,
        "Child not found with id: ghost_child",
    );

    // restart_child invalid arguments
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.restart_child()
"#,
        "restart_child() expects 1 argument",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.restart_child(888)
"#,
        "restart_child() expects string child_id, got int",
    );

    // 7.5 start_child edge cases: invalid specs, duplicate id
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.start_child("not_a_map")
"#,
        "Child spec must be a map",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.start_child({ handler: fn(m) { m } })
"#,
        "Child spec must contain a string 'id'",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.start_child({ id: "worker_missing_handler" })
"#,
        "must contain a callable 'handler' or 'start'",
    );

    // Duplicate child id rejection
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    children: [
        { id: "dup_worker", handler: fn(m) { m } }
    ]
})
sup.start_child({ id: "dup_worker", handler: fn(m) { m } })
"#,
        "Child already exists with id: dup_worker",
    );

    // 7.6 Dynamic child start, stop, restart cycle validation
    let dynamic_lifecycle_script = r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    children: []
})

let len_0 = sup.len()
let child1 = sup.start_child({
    id: "dyn_1",
    state: 10,
    handler: fn(state, msg) {
        if msg == "inc" { return state + 1 }
        if msg == "get" { return { state: state, reply: state } }
        return state
    }
})

let len_1 = sup.len()
let c1_ask1 = child1.ask("get")
child1.send("inc")
child1.send("inc")
let c1_ask2 = child1.ask("get")

// Restart child1 - state should reset to initial_state 10
let child1_restarted = sup.restart_child("dyn_1")
let c1_ask3 = child1_restarted.ask("get")

// Stop child1
sup.stop_child("dyn_1")
let len_after_stop = sup.len()
let child1_dead = child1_restarted.is_alive() == false

sup.stop()
[len_0, len_1, c1_ask1, c1_ask2, c1_ask3, len_after_stop, child1_dead]
"#;

    assert_both_succeed_eq(dynamic_lifecycle_script, json!([0, 1, 10, 12, 10, 0, true]));
}

// =========================================================================
// 8. Stopped Actor and Cascading Supervision Edge Cases
// =========================================================================

#[test]
fn test_adversarial_stopped_actor_send_and_ask() {
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(m) { m })
p.stop()
p.send("hello")
"#,
        "Actor has stopped",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let p = actor.spawn(fn(m) { m })
p.stop()
p.ask("hello")
"#,
        "Actor has stopped",
    );
}

#[test]
fn test_adversarial_supervisor_invalid_config() {
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.supervisor({ strategy: "bogus_strategy" })
"#,
        "Unknown restart strategy 'bogus_strategy'",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.supervisor({ strategy: "one_for_one", max_restarts: -5 })
"#,
        "max_restarts must be a non-negative integer",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
actor.supervisor({ strategy: "one_for_one", within_seconds: -10 })
"#,
        "within_seconds must be a positive number",
    );
}

#[test]
fn test_adversarial_supervisor_cascading_shutdown() {
    let script = r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    children: [
        { id: "c1", handler: fn(m) { m } },
        { id: "c2", handler: fn(m) { m } }
    ]
})

let c1 = sup.get_child("c1")
let c2 = sup.get_child("c2")
let before_c1 = c1.is_alive()
let before_c2 = c2.is_alive()
let before_sup = sup.is_alive()

// Stop the supervisor
let sup_stopped = sup.stop()
let after_sup = sup.is_alive()
let after_c1 = c1.is_alive()
let after_c2 = c2.is_alive()

[before_c1, before_c2, before_sup, sup_stopped, after_sup, after_c1, after_c2]
"#;

    assert_both_succeed_eq(script, json!([true, true, true, true, false, false, false]));

    // Operations on stopped supervisor must fail
    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", children: [] })
sup.stop()
sup.start_child({ id: "late_child", handler: fn(m) { m } })
"#,
        "Supervisor has stopped",
    );

    assert_both_fail_with(
        r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    children: [
        { id: "c1", handler: fn(m) { m } }
    ]
})
sup.stop()
sup.restart_child("c1")
"#,
        "Supervisor is not alive",
    );
}

// =========================================================================
// 9. Direct Low-Level Bytecode Compilation & VM Interpretation Parity
// =========================================================================

#[test]
fn test_adversarial_low_level_bytecode_compiler_and_vm_parity() {
    use shae::compiler::Compiler;
    use shae::parse_source;
    use shae::vm::{InterpretResult, VM};

    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(m) { "echo: " + m })
let r1 = p.ask("hello")
let id = p.id()
let status = p.status()
let stopped = p.stop()
let alive = p.is_alive()
[r1, id.starts_with("actor-"), status, stopped, alive]
"#;

    // AST result
    let ast_val = run(script).expect("AST execution failed");

    // Bytecode compilation directly
    let program = parse_source(script).expect("Parsing failed");
    let chunk = Compiler::new()
        .compile_program(&program)
        .expect("Bytecode compilation failed");

    let mut vm = VM::new();
    let vm_res = vm.interpret(chunk);
    let vm_val = match vm_res {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(err) => panic!("VM runtime error: {}", err),
        InterpretResult::CompileError => panic!("VM compile error"),
    };

    assert_eq!(ast_val.to_json(), vm_val.to_json());
    assert_eq!(
        ast_val.to_json(),
        json!(["echo: hello", true, "running", true, false])
    );
}

// =========================================================================
// 10. Empirical Bug Reproduction: Evaluator Call Depth Leak in Bound Methods
// =========================================================================

#[test]
fn test_reproduce_evaluator_call_depth_leak_on_actor_send_error() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.stop()

let i = 0
while i < 2100 {
    try {
        p.send("msg_to_stopped_actor")
    } catch (e) {
    }
    i = i + 1
}

fn test_func() { 42 }
test_func()
"#;

    // Bytecode VM correctly handles call frames and does not leak depth
    let vm_res = run_vm(script);
    assert!(
        vm_res.is_ok(),
        "VM should succeed without call depth leak, but got: {:?}",
        vm_res
    );
    assert_eq!(vm_res.unwrap().to_json(), json!(42));

    // AST Evaluator currently has a depth leak bug because '?' in call_value returns early
    // bypassing line 3542 (self.depth -= 1).
    let ast_res = run(script);
    // When depth leaks, ast_res returns Err("Stack overflow: maximum call depth exceeded")
    if let Err(ref err) = ast_res {
        let msg = err.to_string();
        if msg.contains("Stack overflow: maximum call depth exceeded") {
            println!(
                "CONFIRMED BUG: AST Evaluator leaked call depth on bound method send() error!"
            );
        }
    }
    assert!(
        ast_res.is_ok(),
        "CRITICAL BUG: AST Evaluator call depth leaked! Failed with: {:?}",
        ast_res
    );
}

#[test]
fn test_reproduce_evaluator_call_depth_leak_on_supervisor_stop_child_error() {
    let script = r#"
let actor = use "std:actor"
let sup = actor.supervisor({
    strategy: "one_for_one",
    children: []
})

let i = 0
while i < 2100 {
    try {
        sup.stop_child("nonexistent")
    } catch (e) {
    }
    i = i + 1
}

fn test_func() { 100 }
test_func()
"#;

    let vm_res = run_vm(script);
    assert!(
        vm_res.is_ok(),
        "VM should succeed without call depth leak, but got: {:?}",
        vm_res
    );
    assert_eq!(vm_res.unwrap().to_json(), json!(100));

    let ast_res = run(script);
    if let Err(ref err) = ast_res {
        let msg = err.to_string();
        if msg.contains("Stack overflow: maximum call depth exceeded") {
            println!(
                "CONFIRMED BUG: AST Evaluator leaked call depth on bound method stop_child() error!"
            );
        }
    }
    assert!(
        ast_res.is_ok(),
        "CRITICAL BUG: AST Evaluator call depth leaked on stop_child! Failed with: {:?}",
        ast_res
    );
}

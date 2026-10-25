use serde_json::json;
use shae::actor::{ActorRef, ActorRegistry};
use shae::{run, run_vm};
use std::sync::Arc;
use std::thread;

// =========================================================================
// 1. Invalid Argument Counts & Types on Actor Bound Methods
// =========================================================================

#[test]
fn test_actor_method_invalid_arg_counts_and_types() {
    let script_send_no_args = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.send()
"#;
    assert!(
        run(script_send_no_args).is_err(),
        "AST send() no args should fail"
    );
    assert!(
        run_vm(script_send_no_args).is_err(),
        "VM send() no args should fail"
    );

    let script_send_too_many = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.send(1, 2)
"#;
    assert!(
        run(script_send_too_many).is_err(),
        "AST send(1, 2) should fail"
    );
    assert!(
        run_vm(script_send_too_many).is_err(),
        "VM send(1, 2) should fail"
    );

    let script_ask_no_args = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask()
"#;
    assert!(
        run(script_ask_no_args).is_err(),
        "AST ask() no args should fail"
    );
    assert!(
        run_vm(script_ask_no_args).is_err(),
        "VM ask() no args should fail"
    );

    let script_ask_too_many = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask(1, 2, 3)
"#;
    assert!(
        run(script_ask_too_many).is_err(),
        "AST ask(1, 2, 3) should fail"
    );
    assert!(
        run_vm(script_ask_too_many).is_err(),
        "VM ask(1, 2, 3) should fail"
    );

    let script_ask_negative_timeout = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask("ping", -50)
"#;
    assert!(
        run(script_ask_negative_timeout).is_err(),
        "AST ask negative timeout should fail"
    );
    assert!(
        run_vm(script_ask_negative_timeout).is_err(),
        "VM ask negative timeout should fail"
    );

    let script_ask_string_timeout = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.ask("ping", "invalid_timeout")
"#;
    assert!(
        run(script_ask_string_timeout).is_err(),
        "AST ask string timeout should fail"
    );
    assert!(
        run_vm(script_ask_string_timeout).is_err(),
        "VM ask string timeout should fail"
    );

    let script_stop_with_args = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.stop("extra")
"#;
    assert!(
        run(script_stop_with_args).is_err(),
        "AST stop with args should fail"
    );
    assert!(
        run_vm(script_stop_with_args).is_err(),
        "VM stop with args should fail"
    );

    let script_is_alive_with_args = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.is_alive(123)
"#;
    assert!(
        run(script_is_alive_with_args).is_err(),
        "AST is_alive with args should fail"
    );
    assert!(
        run_vm(script_is_alive_with_args).is_err(),
        "VM is_alive with args should fail"
    );

    let script_mailbox_len_with_args = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.mailbox_len(123)
"#;
    assert!(
        run(script_mailbox_len_with_args).is_err(),
        "AST mailbox_len with args should fail"
    );
    assert!(
        run_vm(script_mailbox_len_with_args).is_err(),
        "VM mailbox_len with args should fail"
    );

    let script_len_with_args = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.len(123)
"#;
    assert!(
        run(script_len_with_args).is_err(),
        "AST len with args should fail"
    );
    assert!(
        run_vm(script_len_with_args).is_err(),
        "VM len with args should fail"
    );

    let script_nonexistent_method = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { msg })
p.nonexistent_method()
"#;
    assert!(
        run(script_nonexistent_method).is_err(),
        "AST nonexistent method should fail"
    );
    assert!(
        run_vm(script_nonexistent_method).is_err(),
        "VM nonexistent method should fail"
    );
}

// =========================================================================
// 2. Invalid Argument Counts & Types on Std:Actor Module Functions
// =========================================================================

#[test]
fn test_actor_module_functions_invalid_args() {
    let script_spawn_no_args = r#"
let actor = use "std:actor"
actor.spawn()
"#;
    assert!(
        run(script_spawn_no_args).is_err(),
        "spawn() no args should fail"
    );
    assert!(
        run_vm(script_spawn_no_args).is_err(),
        "spawn() no args should fail in VM"
    );

    let script_spawn_non_fn = r#"
let actor = use "std:actor"
actor.spawn("not_a_func")
"#;
    assert!(
        run(script_spawn_non_fn).is_err(),
        "spawn non-fn should fail"
    );
    assert!(
        run_vm(script_spawn_non_fn).is_err(),
        "spawn non-fn should fail in VM"
    );

    let script_spawn_named_non_string = r#"
let actor = use "std:actor"
actor.spawn_named(12345, fn(m) { m })
"#;
    assert!(
        run(script_spawn_named_non_string).is_err(),
        "spawn_named non-string name should fail"
    );
    assert!(
        run_vm(script_spawn_named_non_string).is_err(),
        "spawn_named non-string name should fail in VM"
    );

    let script_send_invalid_target = r#"
let actor = use "std:actor"
actor.send(12345, "msg")
"#;
    assert!(
        run(script_send_invalid_target).is_err(),
        "actor.send invalid target should fail"
    );
    assert!(
        run_vm(script_send_invalid_target).is_err(),
        "actor.send invalid target should fail in VM"
    );

    let script_ask_invalid_target = r#"
let actor = use "std:actor"
actor.ask(12345, "msg")
"#;
    assert!(
        run(script_ask_invalid_target).is_err(),
        "actor.ask invalid target should fail"
    );
    assert!(
        run_vm(script_ask_invalid_target).is_err(),
        "actor.ask invalid target should fail in VM"
    );

    let script_stop_invalid_target = r#"
let actor = use "std:actor"
actor.stop(12345)
"#;
    assert!(
        run(script_stop_invalid_target).is_err(),
        "actor.stop invalid target should fail"
    );
    assert!(
        run_vm(script_stop_invalid_target).is_err(),
        "actor.stop invalid target should fail in VM"
    );

    let script_is_alive_invalid_target = r#"
let actor = use "std:actor"
actor.is_alive(12345)
"#;
    assert!(
        run(script_is_alive_invalid_target).is_err(),
        "actor.is_alive invalid target should fail"
    );
    assert!(
        run_vm(script_is_alive_invalid_target).is_err(),
        "actor.is_alive invalid target should fail in VM"
    );

    let script_mailbox_len_invalid_target = r#"
let actor = use "std:actor"
actor.mailbox_len(12345)
"#;
    assert!(
        run(script_mailbox_len_invalid_target).is_err(),
        "actor.mailbox_len invalid target should fail"
    );
    assert!(
        run_vm(script_mailbox_len_invalid_target).is_err(),
        "actor.mailbox_len invalid target should fail in VM"
    );
}

// =========================================================================
// 3. Send to Unregistered String Name (Clear Runtime Error)
// =========================================================================

#[test]
fn test_send_to_unregistered_name_errors() {
    let script_module_send = r#"
let actor = use "std:actor"
actor.send("ghost_actor_never_registered_42", "ping")
"#;
    let ast_err = run(script_module_send).unwrap_err().to_string();
    assert!(
        ast_err.contains("ghost_actor_never_registered_42") || ast_err.contains("not found"),
        "AST error should clearly identify unregistered actor name: {}",
        ast_err
    );
    let vm_err = run_vm(script_module_send).unwrap_err().to_string();
    assert!(
        vm_err.contains("ghost_actor_never_registered_42") || vm_err.contains("not found"),
        "VM error should clearly identify unregistered actor name: {}",
        vm_err
    );

    let script_prelude_send = r#"
send("ghost_actor_prelude_99", "ping")
"#;
    let ast_prelude_err = run(script_prelude_send).unwrap_err().to_string();
    assert!(
        ast_prelude_err.contains("ghost_actor_prelude_99") || ast_prelude_err.contains("not found"),
        "AST prelude error should clearly identify unregistered actor name: {}",
        ast_prelude_err
    );
    let vm_prelude_err = run_vm(script_prelude_send).unwrap_err().to_string();
    assert!(
        vm_prelude_err.contains("ghost_actor_prelude_99") || vm_prelude_err.contains("not found"),
        "VM prelude error should clearly identify unregistered actor name: {}",
        vm_prelude_err
    );
}

// =========================================================================
// 4. Ask Timeout Behavior When Actor Takes Longer Than Timeout
// =========================================================================

#[test]
fn test_ask_timeout_behavior() {
    let script_ast = r#"
let actor = use "std:actor"
let time = use "std:time"

let slow_actor = actor.spawn(fn(msg) {
    if msg == "slow" {
        time.sleep(150)
        return "finished slow"
    } else {
        return "quick: " + msg
    }
})

// Ask with 30ms timeout: should time out
let timeout_occurred = false
try {
    slow_actor.ask("slow", 30)
} catch e {
    timeout_occurred = true
}

// Actor should still be alive and process subsequent messages after waking up
time.sleep(200)
let alive = slow_actor.is_alive()
let subsequent = slow_actor.ask("fast", 1000)
slow_actor.stop()

[timeout_occurred, alive, subsequent]
"#;

    let ast_res = run(script_ast).expect("AST ask timeout test failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, true, "quick: fast"]),
        "AST timeout handling and subsequent recovery failed"
    );

    let script_vm = r#"
let actor = use "std:actor"
let time = use "std:time"

let slow_actor = actor.spawn(fn(msg) {
    if msg == "slow" {
        time.sleep(150)
        return "finished slow"
    } else {
        return "quick: " + msg
    }
})

let timeout_occurred = false
try {
    slow_actor.ask("slow", 30)
} catch e {
    timeout_occurred = true
}

time.sleep(200)
let alive = slow_actor.is_alive()
let subsequent = slow_actor.ask("fast", 1000)
slow_actor.stop()

[timeout_occurred, alive, subsequent]
"#;

    let vm_res = run_vm(script_vm).expect("VM ask timeout test failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, true, "quick: fast"]),
        "VM timeout handling and subsequent recovery failed"
    );
}

// =========================================================================
// 5. Self-Ask Deadlock Prevention
// =========================================================================

#[test]
fn test_self_ask_deadlock_prevention() {
    let script_ast = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) {
    let self_pid = actor.self_pid()
    if msg == "deadlock_test" {
        // Calling ask on self must fail immediately with deadlock error, NOT hang
        return self_pid.ask("nested")
    }
    return "ok"
})

let error_caught = false
let error_msg = ""
try {
    p.ask("deadlock_test", 1000)
} catch e {
    error_caught = true
    error_msg = e
}

p.stop()
[error_caught, error_msg]
"#;

    let ast_res = run(script_ast).expect("AST deadlock test failed");
    let json_val = ast_res.to_json();
    let caught = json_val[0].as_bool().unwrap();
    let msg = json_val[1].as_str().unwrap();
    assert!(caught, "Deadlock test must catch error");
    assert!(
        msg.contains("Deadlock detected") || msg.contains("cannot ask() itself"),
        "Error message should mention deadlock: {}",
        msg
    );

    let script_vm = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) {
    let self_pid = actor.self_pid()
    if msg == "deadlock_test" {
        return self_pid.ask("nested")
    }
    return "ok"
})

let error_caught = false
let error_msg = ""
try {
    p.ask("deadlock_test", 1000)
} catch e {
    error_caught = true
    error_msg = e
}

p.stop()
[error_caught, error_msg]
"#;

    let vm_res = run_vm(script_vm).expect("VM deadlock test failed");
    let json_val_vm = vm_res.to_json();
    let caught_vm = json_val_vm[0].as_bool().unwrap();
    let msg_vm = json_val_vm[1].as_str().unwrap();
    assert!(caught_vm, "VM deadlock test must catch error");
    assert!(
        msg_vm.contains("Deadlock detected") || msg_vm.contains("cannot ask() itself"),
        "VM error message should mention deadlock: {}",
        msg_vm
    );
}

// =========================================================================
// 6. Concurrent Calls to register / whereis / unregister
// =========================================================================

#[test]
fn test_concurrent_registry_operations() {
    let concurrency = 8;
    let iterations_per_thread = 50;
    let mut handles = Vec::new();

    for thread_idx in 0..concurrency {
        let handle = thread::spawn(move || {
            let mut local_registered = Vec::new();
            for i in 0..iterations_per_thread {
                let name = format!("adv_reg_th_{}_{}", thread_idx, i);
                let actor = ActorRef::spawn_fn(|msg| Ok(msg));

                // Test register
                let reg_res = ActorRegistry::register(&name, Arc::clone(&actor));
                assert!(
                    reg_res.is_ok(),
                    "Registration of unique name should succeed"
                );

                // Test whereis
                let resolved = ActorRegistry::whereis(&name);
                assert!(
                    resolved.is_some(),
                    "Whereis should resolve newly registered actor"
                );

                local_registered.push((name.clone(), actor));

                // Query random other thread names
                let other_name = format!("adv_reg_th_{}_{}", (thread_idx + 1) % concurrency, i);
                let _ = ActorRegistry::whereis(&other_name);
                let _ = ActorRegistry::registered();
            }

            // Cleanup
            for (name, actor) in local_registered {
                let unreg = ActorRegistry::unregister(&name);
                assert!(unreg.is_some(), "Unregister should return removed actor");
                actor.stop();
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Concurrent registry thread panicked");
    }
}

// =========================================================================
// 7. Supervisor Bound Methods Edge Cases
// =========================================================================

#[test]
fn test_supervisor_bound_methods_edge_cases() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({ strategy: "one_for_one", max_restarts: 5, within_seconds: 10 }, [
    {
        id: "w1",
        handler: fn(msg) { "w1: " + msg },
        restart: "permanent"
    }
])

// 1. get_child with non-existent id returns null
let non_existent = sup.get_child("does_not_exist_999")

// 2. get_child invalid argument errors
let get_child_err = false
try {
    sup.get_child()
} catch e {
    get_child_err = true
}

let get_child_type_err = false
try {
    sup.get_child(12345)
} catch e {
    get_child_type_err = true
}

// 3. stop_child with non-existent id throws error
let stop_non_existent_err = false
try {
    sup.stop_child("does_not_exist_999")
} catch e {
    stop_non_existent_err = true
}

// 4. stop_child invalid arguments
let stop_child_no_args_err = false
try {
    sup.stop_child()
} catch e {
    stop_child_no_args_err = true
}

// 5. restart_child with non-existent id throws error
let restart_non_existent_err = false
try {
    sup.restart_child("does_not_exist_999")
} catch e {
    restart_non_existent_err = true
}

// 6. start_child with duplicate id throws error
let dup_start_err = false
try {
    sup.start_child({
        id: "w1",
        handler: fn(msg) { msg }
    })
} catch e {
    dup_start_err = true
}

// 7. Dynamic start_child succeeds for new id
let dynamic_child = sup.start_child({
    id: "w2",
    handler: fn(msg) { "w2: " + msg }
})
let w2_reply = dynamic_child.ask("hello")

// 8. restart_child for active child works
let restarted_w2 = sup.restart_child("w2")
let w2_reply_after_restart = restarted_w2.ask("world")

// 9. stop_child works
sup.stop_child("w2")
let w2_after_stop = sup.get_child("w2")

// Clean up supervisor
sup.stop()

[
    non_existent == null,
    get_child_err,
    get_child_type_err,
    stop_non_existent_err,
    stop_child_no_args_err,
    restart_non_existent_err,
    dup_start_err,
    w2_reply == "w2: hello",
    w2_reply_after_restart == "w2: world",
    w2_after_stop == null
]
"#;

    let ast_res = run(script).expect("AST supervisor edge cases test failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, true, true, true, true, true, true, true, true, true]),
        "AST supervisor edge cases failed"
    );

    let vm_res = run_vm(script).expect("VM supervisor edge cases test failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, true, true, true, true, true, true, true, true, true]),
        "VM supervisor edge cases failed"
    );
}

// =========================================================================
// 8. First-Class Bound Methods in AST & Bytecode VM
// =========================================================================

#[test]
fn test_actor_first_class_bound_methods_ast_and_vm() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn(fn(msg) { "echo:" + msg })

let my_ask = p.ask
let my_is_alive = p.is_alive
let my_len = p.mailbox_len
let my_stop = p.stop

let alive_before = my_is_alive()
let res = my_ask("bound")
let len_val = my_len()
let stop_res = my_stop()
let alive_after = my_is_alive()

[alive_before, res, len_val == 0, stop_res, alive_after]
"#;

    let ast_res = run(script).expect("AST bound method as value failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, "echo:bound", true, true, false]),
        "AST bound method as value failed"
    );

    let vm_res = run_vm(script).expect("VM bound method as value failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, "echo:bound", true, true, false]),
        "VM bound method as value failed"
    );
}

// =========================================================================
// 9. Actor Crash During Ask in AST & Bytecode VM
// =========================================================================

#[test]
fn test_actor_crash_during_ask_ast_and_vm() {
    let script = r#"
let actor = use "std:actor"
let crasher = actor.spawn(fn(msg) {
    if msg == "crash" {
        return 1 / 0
    }
    return "ok"
})

let error_caught = false
let error_msg = ""
try {
    crasher.ask("crash")
} catch (e) {
    error_caught = true
    error_msg = e
}

let alive_after_crash = crasher.is_alive()
crasher.stop()

[error_caught, error_msg, alive_after_crash]
"#;

    let ast_res = run(script).expect("AST crash during ask failed");
    let ast_json = ast_res.to_json();
    assert!(ast_json[0].as_bool().unwrap(), "AST must catch crash error");
    assert!(
        ast_json[1].as_str().unwrap().contains("Division by zero")
            || ast_json[1].as_str().unwrap().contains("zero"),
        "AST error should contain division by zero message: {}",
        ast_json[1]
    );
    assert!(
        !ast_json[2].as_bool().unwrap(),
        "Crashed actor should no longer be alive"
    );

    let vm_res = run_vm(script).expect("VM crash during ask failed");
    let vm_json = vm_res.to_json();
    assert!(vm_json[0].as_bool().unwrap(), "VM must catch crash error");
    assert!(
        vm_json[1].as_str().unwrap().contains("Division by zero")
            || vm_json[1].as_str().unwrap().contains("zero"),
        "VM error should contain division by zero message: {}",
        vm_json[1]
    );
    assert!(
        !vm_json[2].as_bool().unwrap(),
        "Crashed actor should no longer be alive in VM"
    );
}

// =========================================================================
// 10. Operations on Stopped Supervisor
// =========================================================================

#[test]
fn test_operations_on_stopped_supervisor() {
    let script = r#"
let actor = use "std:actor"
let sup = actor.supervisor({ strategy: "one_for_one", max_restarts: 2, within_seconds: 5 }, [
    {
        id: "child1",
        handler: fn(msg) { msg }
    }
])

let stopped = sup.stop()
let is_alive = sup.is_alive()

let start_err = false
try {
    sup.start_child({ id: "child2", handler: fn(m) { m } })
} catch e {
    start_err = true
}

let restart_err = false
try {
    sup.restart_child("child1")
} catch e {
    restart_err = true
}

[stopped, is_alive, start_err, restart_err]
"#;

    let ast_res = run(script).expect("AST stopped supervisor operations failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, false, true, true]),
        "AST stopped supervisor operations failed"
    );

    let vm_res = run_vm(script).expect("VM stopped supervisor operations failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, false, true, true]),
        "VM stopped supervisor operations failed"
    );
}

// =========================================================================
// 11. Send and Whereis on Stopped Actor
// =========================================================================

#[test]
fn test_send_and_whereis_on_stopped_actor() {
    let script = r#"
let actor = use "std:actor"
let p = actor.spawn_named("temp_actor", fn(msg) { msg })
let whereis_alive = actor.whereis("temp_actor")

p.stop()

let whereis_after_stop = actor.whereis("temp_actor")

let send_err = false
try {
    p.send("message to stopped actor")
} catch e {
    send_err = true
}

let ask_err = false
try {
    p.ask("message to stopped actor")
} catch e {
    ask_err = true
}

[whereis_alive != null, whereis_after_stop == null, send_err, ask_err]
"#;

    let ast_res = run(script).expect("AST stopped actor test failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, true, true, true]),
        "AST stopped actor test failed"
    );

    let vm_res = run_vm(script).expect("VM stopped actor test failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, true, true, true]),
        "VM stopped actor test failed"
    );
}

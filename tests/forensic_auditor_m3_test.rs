//! Forensic Audit Integration Test Suite for Milestone 3:
//! Dual-Engine Runtime & Stdlib Integration (AST & VM)

use serde_json::json;
use shae::{run, run_vm};

// =========================================================================
// 1. Dual-Engine Error Handling & Argument Validation
// =========================================================================

#[test]
fn test_forensic_send_invalid_arguments() {
    // 1. send() with invalid target (number)
    let script_invalid_target = r#"
send(12345, "payload")
"#;
    assert!(run(script_invalid_target).is_err());
    assert!(run_vm(script_invalid_target).is_err());

    // 2. send() to unregistered name
    let script_unregistered = r#"
send("definitely_non_existent_actor_xyz", "hello")
"#;
    let ast_err = run(script_unregistered).unwrap_err();
    assert!(
        ast_err
            .to_string()
            .contains("Actor name not found: definitely_non_existent_actor_xyz")
    );

    let vm_err = run_vm(script_unregistered).unwrap_err();
    assert!(
        vm_err
            .to_string()
            .contains("Actor name not found: definitely_non_existent_actor_xyz")
    );
}

#[test]
fn test_forensic_ask_timeout_and_stopped_actor() {
    let script = r#"
let actor = use "std:actor"

// Actor that sleeps without replying
let p = actor.spawn(fn(msg) {
    let ch = channel(1)
    ch.recv(200) // delay 200ms
    msg
})

let timeout_caught = false
try {
    actor.ask(p, "ping", 30) // 30ms timeout
} catch (e) {
    timeout_caught = true
}

p.stop()

let stopped_caught = false
try {
    actor.ask(p, "ping", 50)
} catch (e) {
    stopped_caught = true
}

[timeout_caught, stopped_caught]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([true, true]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, true]));
}

// =========================================================================
// 2. Complex Forwarding & Dual-Engine Parity
// =========================================================================

#[test]
fn test_forensic_actor_pipeline_forwarding() {
    let script = r#"
let actor = use "std:actor"

// Worker actor adds 10
let worker = actor.spawn(fn(msg) { msg + 10 })

// Router actor asks worker and multiplies by 2
let router = actor.spawn(fn(msg) {
    let w_res = worker.ask(msg)
    w_res * 2
})

let res1 = router.ask(5)  // (5 + 10) * 2 = 30
let res2 = router.ask(15) // (15 + 10) * 2 = 50

worker.stop()
router.stop()

[res1, res2]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([30, 50]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([30, 50]));
}

// =========================================================================
// 3. Supervisor AllForOne Strategy in Shae Script (AST & VM)
// =========================================================================

#[test]
fn test_forensic_supervisor_all_for_one_dual_engine() {
    let script = r#"
let actor = use "std:actor"

let sup = actor.supervisor({
    strategy: "all_for_one",
    max_restarts: 5,
    within_seconds: 5.0,
    children: [
        {
            id: "child_a",
            state: 0,
            start: fn(st, msg) {
                if msg == "get" {
                    return { state: st, reply: st }
                } else if msg == "crash" {
                    return 1 / 0
                } else {
                    return st + msg
                }
            }
        },
        {
            id: "child_b",
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

let a = sup.get_child("child_a")
let b = sup.get_child("child_b")

a.send(10)
b.send(20)

let a_val1 = a.ask("get") // 10
let b_val1 = b.ask("get") // 120

// Crash child_a; with all_for_one, BOTH child_a and child_b must be restarted
a.send("crash")

let timer = channel(1)
timer.recv(150) // wait for restart cascade

let a_restarted = sup.get_child("child_a")
let b_restarted = sup.get_child("child_b")

let a_val2 = a_restarted.ask("get") // reset to initial state 0
let b_val2 = b_restarted.ask("get") // reset to initial state 100

sup.stop()

[a_val1, b_val1, a_val2, b_val2]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([10, 120, 0, 100]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([10, 120, 0, 100]));
}

// =========================================================================
// 4. Duplicate Name Registration & Cleanup
// =========================================================================

#[test]
fn test_forensic_duplicate_name_registration() {
    let script = r#"
let actor = use "std:actor"
actor.unregister("forensic_dup_test")

let p1 = actor.spawn_named("forensic_dup_test", fn(msg) { msg })

let duplicate_caught = false
try {
    let p2 = actor.spawn_named("forensic_dup_test", fn(msg) { msg })
} catch (e) {
    duplicate_caught = true
}

let unreg = actor.unregister("forensic_dup_test")
let can_re_register = false
try {
    let p3 = actor.spawn_named("forensic_dup_test", fn(msg) { msg })
    can_re_register = true
    p3.stop()
} catch (e) {}

p1.stop()
actor.unregister("forensic_dup_test")

[duplicate_caught, can_re_register]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([true, true]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, true]));
}

// =========================================================================
// 5. Child Spec Parsing Error Handling
// =========================================================================

#[test]
fn test_forensic_invalid_child_spec_rejection() {
    // Missing 'id'
    let script_missing_id = r#"
let actor = use "std:actor"
actor.supervisor([
    { start: fn(msg) { msg } }
])
"#;
    assert!(run(script_missing_id).is_err());
    assert!(run_vm(script_missing_id).is_err());

    // Non-callable handler
    let script_bad_handler = r#"
let actor = use "std:actor"
actor.supervisor([
    { id: "bad_child", start: 12345 }
])
"#;
    assert!(run(script_bad_handler).is_err());
    assert!(run_vm(script_bad_handler).is_err());

    // Invalid restart policy string
    let script_bad_policy = r#"
let actor = use "std:actor"
actor.supervisor([
    { id: "bad_child", start: fn(msg) { msg }, restart: "never_gonna_give_you_up" }
])
"#;
    assert!(run(script_bad_policy).is_err());
    assert!(run_vm(script_bad_policy).is_err());
}

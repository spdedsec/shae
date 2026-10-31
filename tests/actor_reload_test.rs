//! Integration tests for Chunk 30: Hot-Code Reloading (`tests/actor_reload_test.rs`)
//! Tests local actor hot-reloading with state preservation, migration functions,
//! mailbox continuity, and distributed cluster actor reloading across network mesh.

use serde_json::json;
use shae::actor_cluster::ClusterNode;
use shae::value::Value;
use shae::{run, run_vm};
use std::time::Duration;

// =========================================================================
// 1. Local Actor Hot-Reloading with State Preservation
// =========================================================================

#[test]
fn test_local_actor_hot_reload_preserves_state() {
    let script = r#"
let actor = use "std:actor"

// V1 handler: adds msg to state
let p = actor.spawn(10, fn(state, msg) {
    if msg["op"] == "add" {
        let next_val = state + msg["val"]
        return { "state": next_val, "reply": next_val }
    } else if msg["op"] == "get" {
        return { "state": state, "reply": state }
    }
    return state
})

let r1 = p.ask({ "op": "add", "val": 5 })
let r2 = p.ask({ "op": "get" })

// Hot-reload to V2 handler: multiplies instead of adding, preserving state 15
let reload_ok = p.reload(fn(state, msg) {
    if msg["op"] == "mult" {
        let next_val = state * msg["val"]
        return { "state": next_val, "reply": next_val }
    } else if msg["op"] == "get" {
        return { "state": state, "reply": state }
    }
    return state
})

let r3 = p.ask({ "op": "mult", "val": 4 })
let r4 = p.ask({ "op": "get" })
p.stop()

[r1, r2, reload_ok, r3, r4]
"#;

    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!([15, 15, true, 60, 60]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!([15, 15, true, 60, 60]));
}

// =========================================================================
// 2. Local Actor Hot-Reloading with State Migration Function
// =========================================================================

#[test]
fn test_local_actor_hot_reload_with_state_migration() {
    let script = r#"
let actor = use "std:actor"

// V1 handler: state is a simple integer
let p = actor.spawn(42, fn(state, msg) {
    if msg == "get" {
        return { "state": state, "reply": state }
    } else if msg == "inc" {
        return state + 1
    }
    return state
})

let v1_val = p.ask("get")

// Migration function: converts integer state to structured map { count: state, version: 2 }
let migration = fn(old_state) {
    return { "count": old_state, "version": 2, "history": ["migrated"] }
}

// V2 handler: operates on structured map state
let v2_handler = fn(state, msg) {
    if msg == "get" {
        return { "state": state, "reply": state["count"] }
    } else if msg == "info" {
        return { "state": state, "reply": state }
    } else if msg == "inc" {
        let next_count = state["count"] + 1
        let updated = { "count": next_count, "version": state["version"], "history": state["history"] }
        return { "state": updated, "reply": next_count }
    }
    return state
}

let reload_ok = p.reload(v2_handler, migration)
let v2_val = p.ask("get")
let v2_info = p.ask("info")
let v2_inc = p.ask("inc")
p.stop()

[v1_val, reload_ok, v2_val, v2_info["version"], v2_inc]
"#;

    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!([42, true, 42, 2, 43]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!([42, true, 42, 2, 43]));
}

// =========================================================================
// 3. Mailbox Continuity Across Hot-Reload
// =========================================================================

#[test]
fn test_mailbox_continuity_across_hot_reload() {
    let script = r#"
let actor = use "std:actor"

// V1 handler logs message to history list
let p = actor.spawn([], fn(state, msg) {
    if msg["op"] == "log_v1" {
        state.push("v1:" + msg["text"])
        return state
    } else if msg["op"] == "get" {
        return { "state": state, "reply": state }
    }
    return state
})

// Queue messages before reload
p.send({ "op": "log_v1", "text": "alpha" })
p.send({ "op": "log_v1", "text": "beta" })

// Reload to V2 handler
let reload_ok = p.reload(fn(state, msg) {
    if msg["op"] == "log_v2" {
        state.push("v2:" + msg["text"])
        return state
    } else if msg["op"] == "get" {
        return { "state": state, "reply": state }
    }
    return state
})

// Queue messages after reload
p.send({ "op": "log_v2", "text": "gamma" })
p.send({ "op": "log_v2", "text": "delta" })

let final_history = p.ask({ "op": "get" })
p.stop()

[reload_ok, final_history]
"#;

    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(
        ast_res.to_json(),
        json!([true, ["v1:alpha", "v1:beta", "v2:gamma", "v2:delta"]])
    );

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_res.to_json(),
        json!([true, ["v1:alpha", "v1:beta", "v2:gamma", "v2:delta"]])
    );
}

// =========================================================================
// 4. Distributed Cluster Hot-Reload Across Network Mesh
// =========================================================================

#[test]
fn test_distributed_cluster_hot_reload_across_mesh() {
    let node1 = ClusterNode::start(
        "cluster_worker_node_1",
        "127.0.0.1:0",
        Some("hot_reload_token".into()),
    )
    .expect("Failed to start cluster worker node 1");

    let node2 = ClusterNode::start(
        "cluster_controller_node_2",
        "127.0.0.1:0",
        Some("hot_reload_token".into()),
    )
    .expect("Failed to start cluster controller node 2");

    // Spawn stateful daemon actor on node 1
    let daemon_handler = run("let h = fn(state, msg) {
        if msg == \"status\" {
            return { \"state\": state, \"reply\": \"v1_online:\" + str(state) }
        } else if msg == \"inc\" {
            return state + 1
        }
        return state
    }; h")
    .unwrap();

    let daemon = node1
        .spawn_actor("telemetry_daemon", Some(Value::Int(100)), daemon_handler)
        .expect("Failed to spawn daemon actor on node 1");

    // Connect node 2 to node 1 mesh
    let peer = node2
        .connect(&node1.address())
        .expect("Failed to connect node 2 to node 1");
    assert_eq!(peer.node_name, "cluster_worker_node_1");

    // Resolve remote actor from node 2
    let remote_daemon = node2.actor("telemetry_daemon", "cluster_worker_node_1");
    assert!(remote_daemon.is_alive());

    // Send ask to remote actor
    let r1 = remote_daemon
        .ask(Value::String("status".into()), Some(Duration::from_millis(5000)))
        .expect("Failed ask v1");
    assert_eq!(r1.to_json(), json!("v1_online:100"));

    // Increment state remotely
    remote_daemon
        .send(Value::String("inc".into()))
        .expect("Failed send inc");

    // Hot-reload remote actor code over cluster network mesh
    let v2_code = r#"fn(state, msg) {
        if msg == "status" {
            return { "state": state, "reply": "v2_upgraded:" + str(state) }
        } else if msg == "double" {
            return state * 2
        }
        return state
    }"#;

    let reload_ok = remote_daemon
        .reload(
            Value::String(v2_code.into()),
            None,
            Some(Duration::from_millis(5000)),
        )
        .expect("Failed to hot-reload remote actor");
    assert!(reload_ok);

    // Verify node connections remained open (zero downtime)
    assert!(node1.is_alive());
    assert!(node2.is_alive());
    assert!(node2.peers().contains(&"cluster_worker_node_1".to_string()));

    // Verify V2 behavior preserving accumulated state (101)
    let r2 = remote_daemon
        .ask(Value::String("status".into()), Some(Duration::from_millis(5000)))
        .expect("Failed ask v2 status");
    assert_eq!(r2.to_json(), json!("v2_upgraded:101"));

    // Verify new V2 operation
    remote_daemon
        .send(Value::String("double".into()))
        .expect("Failed send double");

    let r3 = remote_daemon
        .ask(Value::String("status".into()), Some(Duration::from_millis(5000)))
        .expect("Failed ask v2 doubled");
    assert_eq!(r3.to_json(), json!("v2_upgraded:202"));

    // Shutdown
    daemon.stop();
    node2.stop();
    node1.stop();
}

// =========================================================================
// 5. Cluster Node reload_actor API
// =========================================================================

#[test]
fn test_cluster_node_reload_actor_api() {
    let node_a = ClusterNode::start("node_alpha", "127.0.0.1:0", None)
        .expect("Failed to start node alpha");
    let node_b = ClusterNode::start("node_beta", "127.0.0.1:0", None)
        .expect("Failed to start node beta");

    let handler_v1 = run("let h = fn(state, msg) {
        return { \"state\": state, \"reply\": \"alpha_v1:\" + str(state) }
    }; h")
    .unwrap();

    let act = node_a
        .spawn_actor("service_a", Some(Value::Int(5)), handler_v1)
        .expect("Failed to spawn service_a");

    node_b.connect(&node_a.address()).expect("Failed to connect");

    // Hot-reload service_a from node_b via reload_actor with migration
    let new_handler = "fn(state, msg) { return { \"state\": state, \"reply\": \"alpha_v2:\" + str(state) } }";
    let migration = "fn(old_state) { return old_state + 100 }";

    let ok = node_b
        .reload_actor(
            "service_a@node_alpha",
            Value::String(new_handler.into()),
            Some(Value::String(migration.into())),
            Some(Duration::from_millis(5000)),
        )
        .expect("node_b.reload_actor failed");
    assert!(ok);

    let reply = act
        .ask(Value::String("test".into()), Some(Duration::from_millis(5000)))
        .expect("Ask failed");
    assert_eq!(reply.to_json(), json!("alpha_v2:105"));

    act.stop();
    node_b.stop();
    node_a.stop();
}

// =========================================================================
// 6. Migration Failure Handling & Actor Resilience
// =========================================================================

#[test]
fn test_reload_migration_error_does_not_crash_actor() {
    let script = r#"
let actor = use "std:actor"

let p = actor.spawn(10, fn(state, msg) {
    return { "state": state, "reply": state }
})

// Intentionally faulty migration function that divides by zero
let bad_migration = fn(st) {
    return 10 / 0
}

let v2_handler = fn(state, msg) {
    return { "state": state, "reply": state * 100 }
}

// Attempt reload with faulty migration
let res = "initial"
try {
    p.reload(v2_handler, bad_migration)
} catch err {
    res = "caught_migration_err"
}

// Actor should still be running with original state 10 and original handler
let alive = p.is_alive()
let current = p.ask("get")
p.stop()

[res, alive, current]
"#;

    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!(["caught_migration_err", true, 10]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!(["caught_migration_err", true, 10]));
}

use shae::actor_cluster::ClusterNode;
use shae::run_file;
use shae::run_file_vm;
use shae::value::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

#[test]
fn test_rust_cluster_node_handshake_and_messaging() {
    let node1 = ClusterNode::start(
        "test_node_1",
        "127.0.0.1:0",
        Some("test_auth_cookie".into()),
    )
    .expect("Failed to start node 1");

    let node2 = ClusterNode::start(
        "test_node_2",
        "127.0.0.1:0",
        Some("test_auth_cookie".into()),
    )
    .expect("Failed to start node 2");

    // Spawn an actor on node 1
    let counter_actor = shae::actor::spawn_native(Arc::new(|msg| {
        println!("test_node_1 received msg: {:?}", msg);
    }));
    node1.register("native_counter", counter_actor);

    // Connect node 2 to node 1
    let peer = node2
        .connect(&node1.address())
        .expect("Failed to connect node 2 to node 1");

    assert_eq!(peer.node_name, "test_node_1");
    assert!(node2.peers().contains(&"test_node_1".to_string()));

    // Send a message from node 2 to actor on node 1
    let send_res = node2.send_to_remote(
        "test_node_1",
        "native_counter",
        Value::String("hello_remote".into()),
    );
    assert!(send_res.is_ok());

    // Stop nodes
    node2.stop();
    node1.stop();
}

#[test]
fn test_cluster_authentication_cookie_rejection() {
    let node_auth = ClusterNode::start("secure_node", "127.0.0.1:0", Some("secret_valid".into()))
        .expect("Failed to start secure node");

    let node_rogue = ClusterNode::start("rogue_node", "127.0.0.1:0", Some("invalid_cookie".into()))
        .expect("Failed to start rogue node");

    let conn_res = node_rogue.connect(&node_auth.address());
    assert!(
        conn_res.is_err(),
        "Expected rogue node connection to be rejected due to cookie mismatch"
    );

    node_rogue.stop();
    node_auth.stop();
}

static RECEIVED_BROADCAST: Mutex<Option<String>> = Mutex::new(None);

fn test_broadcast_handler(
    _ev: &mut shae::eval::Evaluator,
    args: Vec<Value>,
    _sp: shae::ast::Span,
) -> Result<Value, shae::eval::RuntimeError> {
    if args.len() == 2 {
        if let Value::String(ref val) = args[1] {
            *RECEIVED_BROADCAST.lock().unwrap() = Some(val.clone());
        }
    }
    Ok(Value::Null)
}

#[test]
fn test_cluster_broadcast_mesh() {
    let node_a =
        ClusterNode::start("node_broad_a", "127.0.0.1:0", None).expect("Failed to start node A");
    let node_b =
        ClusterNode::start("node_broad_b", "127.0.0.1:0", None).expect("Failed to start node B");

    node_b
        .connect(&node_a.address())
        .expect("Failed to connect node B to node A");

    // Ensure node A sees node B in its peers
    let wait_start = std::time::Instant::now();
    while node_a.peers().is_empty() && wait_start.elapsed() < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(10));
    }

    *RECEIVED_BROADCAST.lock().unwrap() = None;

    // Register broadcast listener callback
    let cb = Value::Builtin {
        name: "test_listener".into(),
        func: test_broadcast_handler,
    };
    node_b.on_broadcast(cb);

    // Node A broadcasts message
    node_a.broadcast("alerts", Value::String("system_online".into()));

    // Wait briefly for delivery
    let start = std::time::Instant::now();
    let mut got_msg = false;
    while start.elapsed() < Duration::from_millis(1500) {
        if let Some(msg) = RECEIVED_BROADCAST.lock().unwrap().as_ref() {
            if msg == "system_online" {
                got_msg = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(got_msg, "Node B failed to receive broadcast from Node A");

    node_b.stop();
    node_a.stop();
}

#[test]
fn test_pure_shae_self_cluster_dual_engine() {
    run_with_large_stack(|| {
        let path = "tests/shae/self_cluster_test.shae";

        // Bytecode VM
        let vm_res = run_file_vm(path);
        assert!(
            vm_res.is_ok(),
            "self_cluster_test failed on VM: {:?}",
            vm_res.err()
        );

        // AST Evaluator
        let ast_res = run_file(path);
        assert!(
            ast_res.is_ok(),
            "self_cluster_test failed on AST: {:?}",
            ast_res.err()
        );
    });
}

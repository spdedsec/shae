//! Integration Tests for Cluster Transport Security: TLS & Mutual-TLS (mTLS)
//! tests/actor_cluster_tls_test.rs

use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use shae::actor_cluster::{ClusterNode, ClusterTlsConfig};
use shae::run_file;
use shae::run_file_vm;
use shae::value::Value;
use std::sync::Arc;
use std::time::Duration;

fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(f)
        .expect("Failed to spawn thread");
    handle.join().expect("Thread panicked");
}

/// Helper to generate a self-signed cert and key for TLS testing.
fn generate_tls_credentials() -> (String, String) {
    let cert_gen =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("Failed to generate self-signed TLS cert");
    (cert_gen.cert.pem(), cert_gen.key_pair.serialize_pem())
}

/// Helper to generate a CA and two signed certificates for mTLS testing.
fn generate_mtls_pki() -> (String, (String, String), (String, String), (String, String)) {
    // 1. Root CA
    let ca_key = KeyPair::generate().expect("Failed to generate CA key");
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .expect("Failed to self-sign CA");
    let ca_pem = ca_cert.pem();

    // 2. Node 1 Cert signed by CA
    let node1_key = KeyPair::generate().expect("Failed to generate Node 1 key");
    let node1_params =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("Failed to create Node 1 cert params");
    let node1_cert = node1_params
        .signed_by(&node1_key, &ca_cert, &ca_key)
        .expect("Failed to sign Node 1 cert with CA");

    // 3. Node 2 Cert signed by CA
    let node2_key = KeyPair::generate().expect("Failed to generate Node 2 key");
    let node2_params =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("Failed to create Node 2 cert params");
    let node2_cert = node2_params
        .signed_by(&node2_key, &ca_cert, &ca_key)
        .expect("Failed to sign Node 2 cert with CA");

    // 4. Untrusted Rogue Cert signed by different independent CA
    let rogue_ca_key = KeyPair::generate().expect("Failed to generate rogue CA key");
    let mut rogue_ca_params = CertificateParams::default();
    rogue_ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let rogue_ca = rogue_ca_params
        .self_signed(&rogue_ca_key)
        .expect("Failed to sign rogue CA");
    let rogue_key = KeyPair::generate().expect("Failed to generate rogue key");
    let rogue_params =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    let rogue_cert = rogue_params
        .signed_by(&rogue_key, &rogue_ca, &rogue_ca_key)
        .unwrap();

    (
        ca_pem,
        (node1_cert.pem(), node1_key.serialize_pem()),
        (node2_cert.pem(), node2_key.serialize_pem()),
        (rogue_cert.pem(), rogue_key.serialize_pem()),
    )
}

#[test]
fn test_cluster_tls_encrypted_messaging() {
    let (cert_pem, key_pem) = generate_tls_credentials();

    let tls_config_1 = ClusterTlsConfig {
        cert: cert_pem.clone(),
        key: key_pem.clone(),
        ca: None,
        verify_peer: Some(false),
        server_name: Some("localhost".into()),
    };

    let tls_config_2 = ClusterTlsConfig {
        cert: cert_pem,
        key: key_pem,
        ca: None,
        verify_peer: Some(false),
        server_name: Some("localhost".into()),
    };

    let node1 = ClusterNode::start_with_tls("tls_node_1", "127.0.0.1:0", None, Some(tls_config_1))
        .expect("Failed to start TLS node 1");
    let node2 = ClusterNode::start_with_tls("tls_node_2", "127.0.0.1:0", None, Some(tls_config_2))
        .expect("Failed to start TLS node 2");

    assert!(node1.is_tls(), "Node 1 must report is_tls() = true");
    assert!(node2.is_tls(), "Node 2 must report is_tls() = true");

    // Register a counter actor on node 1
    let received_msgs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let msgs_clone = Arc::clone(&received_msgs);
    let counter_actor = shae::actor::spawn_native(Arc::new(move |msg| {
        msgs_clone.lock().unwrap().push(msg);
    }));
    node1.register("tls_echo_actor", counter_actor);

    // Connect node 2 to node 1 over TLS
    let peer = node2
        .connect(&node1.address())
        .expect("Failed to connect node 2 to node 1 via TLS");

    assert_eq!(peer.node_name, "tls_node_1");
    assert!(peer.is_tls, "Peer connection must report is_tls = true");
    assert!(node2.peers().contains(&"tls_node_1".to_string()));

    // Send encrypted message from node 2 to actor on node 1
    let send_res = node2.send_to_remote(
        "tls_node_1",
        "tls_echo_actor",
        Value::String("encrypted_payload_shae_tls".into()),
    );
    assert!(send_res.is_ok(), "Failed to send message over TLS");

    // Allow delivery
    std::thread::sleep(Duration::from_millis(50));
    {
        let guard = received_msgs.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0], Value::String("encrypted_payload_shae_tls".into()));
    }

    node2.stop();
    node1.stop();
}

#[test]
fn test_cluster_mtls_mutual_authentication_and_ask() {
    let (ca_pem, (n1_cert, n1_key), (n2_cert, n2_key), _) = generate_mtls_pki();

    // Node 1: Server and Client mTLS configuration requiring CA verification
    let tls_config_1 = ClusterTlsConfig {
        cert: n1_cert,
        key: n1_key,
        ca: Some(ca_pem.clone()),
        verify_peer: Some(true),
        server_name: Some("localhost".into()),
    };

    // Node 2: Server and Client mTLS configuration with CA verification
    let tls_config_2 = ClusterTlsConfig {
        cert: n2_cert,
        key: n2_key,
        ca: Some(ca_pem),
        verify_peer: Some(true),
        server_name: Some("localhost".into()),
    };

    let node1 = ClusterNode::start_with_tls("mtls_node_1", "127.0.0.1:0", None, Some(tls_config_1))
        .expect("Failed to start mTLS node 1");
    let node2 = ClusterNode::start_with_tls("mtls_node_2", "127.0.0.1:0", None, Some(tls_config_2))
        .expect("Failed to start mTLS node 2");

    assert!(node1.is_tls());
    assert!(node2.is_tls());

    // Register a math compute actor on node 1 using spawn_fn
    let compute_actor = shae::actor::ActorRef::spawn_fn(|msg| {
        if let Value::Int(n) = msg {
            Ok(Value::Int(n * n))
        } else {
            Ok(Value::Int(0))
        }
    });
    node1.register("math_service", compute_actor);

    // Connect node 2 to node 1 over mTLS
    let peer = node2
        .connect(&node1.address())
        .expect("Failed to connect node 2 to node 1 via mTLS");
    assert_eq!(peer.node_name, "mtls_node_1");

    // Perform synchronous Ask over mutual-TLS
    let reply = node2
        .ask_remote(
            "mtls_node_1",
            "math_service",
            Value::Int(9),
            Some(Duration::from_millis(3000)),
        )
        .expect("mTLS ask_remote failed");

    assert_eq!(reply, Value::Int(81), "9 * 9 must equal 81 over mTLS link");

    node2.stop();
    node1.stop();
}

#[test]
fn test_cluster_mtls_rejects_untrusted_peer() {
    let (ca_pem, (n1_cert, n1_key), _, (rogue_cert, rogue_key)) = generate_mtls_pki();

    // Node 1: Strict CA verification required
    let tls_config_1 = ClusterTlsConfig {
        cert: n1_cert,
        key: n1_key,
        ca: Some(ca_pem.clone()),
        verify_peer: Some(true),
        server_name: Some("localhost".into()),
    };

    // Rogue Node: Client presents certificate not signed by Node 1's trusted CA
    let rogue_tls_config = ClusterTlsConfig {
        cert: rogue_cert,
        key: rogue_key,
        ca: Some(ca_pem),
        verify_peer: Some(false),
        server_name: Some("localhost".into()),
    };

    let node1 = ClusterNode::start_with_tls("secure_hub", "127.0.0.1:0", None, Some(tls_config_1))
        .expect("Failed to start secure hub");
    let rogue_node =
        ClusterNode::start_with_tls("rogue_client", "127.0.0.1:0", None, Some(rogue_tls_config))
            .expect("Failed to start rogue node");

    // Attempt connection from untrusted client to strict mTLS server: MUST FAIL
    let connect_res = rogue_node.connect(&node1.address());
    assert!(
        connect_res.is_err(),
        "mTLS server must reject peer whose certificate is not trusted by CA"
    );

    rogue_node.stop();
    node1.stop();
}

#[test]
fn test_pure_shae_tls_cluster_dual_engine() {
    run_with_large_stack(|| {
        let (cert_pem, key_pem) = generate_tls_credentials();
        let cert_file = "tests/shae/temp_tls_cert.pem";
        let key_file = "tests/shae/temp_tls_key.pem";

        std::fs::write(cert_file, cert_pem).unwrap();
        std::fs::write(key_file, key_pem).unwrap();

        let sample_script = "tests/shae/actor_cluster_tls_test.shae";

        // AST Evaluator
        let res_ast = run_file(sample_script);
        assert!(
            res_ast.is_ok(),
            "TLS cluster script failed on AST Evaluator: {:?}",
            res_ast
        );

        // Bytecode Stack VM
        let res_vm = run_file_vm(sample_script);
        assert!(
            res_vm.is_ok(),
            "TLS cluster script failed on Bytecode VM: {:?}",
            res_vm
        );

        let _ = std::fs::remove_file(cert_file);
        let _ = std::fs::remove_file(key_file);
    });
}

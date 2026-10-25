//! Empirical Challenger Stress Tests for Milestone 1:
//! 1. Mailbox FIFO Stress: Multi-threaded concurrent producers and high-volume strict FIFO ordering.
//! 2. State Isolation: Concurrent mutation of arrays and maps during and after send operations.
//! 3. Deep-Clone Edge Cases: Direct, mutual, and heterogeneous cycles, DAG sharing, and deeply nested structures.

use indexmap::IndexMap;
use shae::actor::spawn_native;
use shae::ast::Span;
use shae::channel::Channel;
use shae::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

// =============================================================================
// PART 1: Mailbox FIFO Stress Tests
// =============================================================================

#[test]
fn test_fifo_multithreaded_producers_stress() {
    const NUM_PRODUCERS: usize = 10;
    const MSGS_PER_PRODUCER: usize = 200;
    const TOTAL_MSGS: usize = NUM_PRODUCERS * MSGS_PER_PRODUCER;

    let received_records: Arc<Mutex<Vec<(usize, usize)>>> =
        Arc::new(Mutex::new(Vec::with_capacity(TOTAL_MSGS)));
    let records_clone = Arc::clone(&received_records);
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&count);

    // Actor records (producer_id, seq) in arrival order
    let actor = spawn_native(Arc::new(move |msg: Value| {
        if let Value::Array(ref arr) = msg {
            let guard = arr.read().unwrap();
            if guard.len() == 2 {
                if let (Value::Int(producer_id), Value::Int(seq)) = (&guard[0], &guard[1]) {
                    records_clone
                        .lock()
                        .unwrap()
                        .push((*producer_id as usize, *seq as usize));
                    count_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    }));

    let start_gate = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();

    // Spawn 10 concurrent producer threads
    for producer_id in 0..NUM_PRODUCERS {
        let actor_ref = Arc::clone(&actor);
        let gate = Arc::clone(&start_gate);

        handles.push(thread::spawn(move || {
            // Spin until all threads are ready to fire simultaneously
            while !gate.load(Ordering::Acquire) {
                thread::yield_now();
            }

            for seq in 0..MSGS_PER_PRODUCER {
                let msg = Value::Array(Arc::new(RwLock::new(vec![
                    Value::Int(producer_id as i64),
                    Value::Int(seq as i64),
                ])));
                actor_ref.send(msg).expect("send failed");
            }
        }));
    }

    // Release gate to maximize concurrency
    start_gate.store(true, Ordering::Release);

    // Join all producers
    for h in handles {
        h.join().unwrap();
    }

    // Await processing with a 10s timeout
    let deadline = Instant::now() + Duration::from_secs(10);
    while count.load(Ordering::SeqCst) < TOTAL_MSGS && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        count.load(Ordering::SeqCst),
        TOTAL_MSGS,
        "Actor did not process all messages within timeout!"
    );

    let records = received_records.lock().unwrap().clone();
    assert_eq!(records.len(), TOTAL_MSGS);

    // Verify per-producer strict monotonic FIFO order: 0, 1, 2, ..., MSGS_PER_PRODUCER - 1
    let mut producer_sequences: HashMap<usize, Vec<usize>> = HashMap::new();
    for (prod_id, seq) in records {
        producer_sequences.entry(prod_id).or_default().push(seq);
    }

    assert_eq!(
        producer_sequences.len(),
        NUM_PRODUCERS,
        "Missing messages from some producers"
    );

    for prod_id in 0..NUM_PRODUCERS {
        let seqs = producer_sequences.get(&prod_id).unwrap();
        assert_eq!(
            seqs.len(),
            MSGS_PER_PRODUCER,
            "Producer {} has incorrect message count",
            prod_id
        );

        for (expected_seq, &actual_seq) in seqs.iter().enumerate() {
            assert_eq!(
                actual_seq, expected_seq,
                "FIFO violation for producer {}! Expected seq {}, got {}",
                prod_id, expected_seq, actual_seq
            );
        }
    }

    actor.stop();
}

#[test]
fn test_fifo_single_producer_high_volume_stress() {
    const TOTAL_MSGS: usize = 5000;

    let received_seqs = Arc::new(Mutex::new(Vec::with_capacity(TOTAL_MSGS)));
    let seqs_clone = Arc::clone(&received_seqs);
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&count);

    let actor = spawn_native(Arc::new(move |msg: Value| {
        if let Value::Int(seq) = msg {
            seqs_clone.lock().unwrap().push(seq as usize);
            count_clone.fetch_add(1, Ordering::SeqCst);
        }
    }));

    for i in 0..TOTAL_MSGS {
        actor.send(Value::Int(i as i64)).expect("Send failed");
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    while count.load(Ordering::SeqCst) < TOTAL_MSGS && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(count.load(Ordering::SeqCst), TOTAL_MSGS);

    let seqs = received_seqs.lock().unwrap().clone();
    assert_eq!(seqs.len(), TOTAL_MSGS);

    for (expected, actual) in seqs.iter().enumerate() {
        assert_eq!(
            *actual, expected,
            "FIFO ordering violated at index {}: got {}",
            expected, actual
        );
    }

    actor.stop();
}

// =============================================================================
// PART 2: State Isolation Under Aggressive Concurrent Mutation
// =============================================================================

#[test]
fn test_state_isolation_concurrent_array_mutation_during_send() {
    const NUM_MUTATOR_THREADS: usize = 4;
    const ITERATIONS_PER_THREAD: usize = 50;

    let violations = Arc::new(AtomicUsize::new(0));
    let violations_actor = Arc::clone(&violations);
    let processed_count = Arc::new(AtomicUsize::new(0));
    let processed_clone = Arc::clone(&processed_count);

    // Actor inspects array, verifies no post-send mutations, and mutates its local copy aggressively
    let actor = spawn_native(Arc::new(move |msg: Value| {
        if let Value::Array(arr) = msg {
            {
                let g = arr.read().unwrap();
                assert!(!g.is_empty());
            }

            // Actor aggressively mutates its own received array
            {
                let mut w = arr.write().unwrap();
                w.clear();
                w.push(Value::String("actor_secret_token_12345".into()));
            }

            // Verify its own mutation took place
            {
                let r = arr.read().unwrap();
                if r.len() != 1 || r[0] != Value::String("actor_secret_token_12345".into()) {
                    violations_actor.fetch_add(1, Ordering::SeqCst);
                }
            }

            processed_clone.fetch_add(1, Ordering::SeqCst);
        } else {
            violations_actor.fetch_add(1, Ordering::SeqCst);
        }
    }));

    let mut handles = Vec::new();

    for _ in 0..NUM_MUTATOR_THREADS {
        let actor_ref = Arc::clone(&actor);
        let violations_sender = Arc::clone(&violations);

        handles.push(thread::spawn(move || {
            // Each thread maintains its own shared array
            let inner = Arc::new(RwLock::new(vec![Value::Int(1), Value::Int(2)]));
            let sender_array = Value::Array(Arc::clone(&inner));

            for iter in 0..ITERATIONS_PER_THREAD {
                // Mutate local array before send
                {
                    let mut g = inner.write().unwrap();
                    g.push(Value::Int(iter as i64));
                }

                // Send to actor
                actor_ref
                    .send(sender_array.clone())
                    .expect("send must succeed");

                // Immediately mutate local array after send
                {
                    let mut g = inner.write().unwrap();
                    g.push(Value::Int(9999));
                    if g.len() > 10 {
                        g.truncate(5);
                    }
                }

                // Assert that the sender array NEVER contains actor's secret token
                {
                    let g = inner.read().unwrap();
                    for item in g.iter() {
                        if item == &Value::String("actor_secret_token_12345".into()) {
                            violations_sender.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let expected_total = NUM_MUTATOR_THREADS * ITERATIONS_PER_THREAD;
    let deadline = Instant::now() + Duration::from_secs(10);
    while processed_count.load(Ordering::SeqCst) < expected_total && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        processed_count.load(Ordering::SeqCst),
        expected_total,
        "Not all messages were processed"
    );
    assert_eq!(
        violations.load(Ordering::SeqCst),
        0,
        "State isolation violations detected during concurrent array mutation!"
    );

    actor.stop();
}

#[test]
fn test_state_isolation_concurrent_map_mutation_during_send() {
    const NUM_THREADS: usize = 4;
    const ITERS: usize = 50;

    let violations = Arc::new(AtomicUsize::new(0));
    let violations_actor = Arc::clone(&violations);
    let processed = Arc::new(AtomicUsize::new(0));
    let processed_clone = Arc::clone(&processed);

    let actor = spawn_native(Arc::new(move |msg: Value| {
        if let Value::Map(m) = msg {
            // Actor aggressively mutates received map
            {
                let mut w = m.write().unwrap();
                w.insert("actor_private".into(), Value::Bool(true));
                w.shift_remove("sender_key_0");
            }
            processed_clone.fetch_add(1, Ordering::SeqCst);
        } else {
            violations_actor.fetch_add(1, Ordering::SeqCst);
        }
    }));

    let mut handles = Vec::new();

    for t_id in 0..NUM_THREADS {
        let actor_ref = Arc::clone(&actor);
        let violations_sender = Arc::clone(&violations);

        handles.push(thread::spawn(move || {
            let mut init_map = IndexMap::new();
            init_map.insert("sender_key_0".into(), Value::Int(t_id as i64));
            let inner = Arc::new(RwLock::new(init_map));
            let map_val = Value::Map(Arc::clone(&inner));

            for iter in 0..ITERS {
                // Mutate before send
                {
                    let mut w = inner.write().unwrap();
                    w.insert(format!("iter_{}", iter), Value::Int(iter as i64));
                }

                actor_ref.send(map_val.clone()).expect("send failed");

                // Mutate after send
                {
                    let mut w = inner.write().unwrap();
                    w.insert("after_send".into(), Value::String("mutated".into()));
                }

                // Check sender map never sees actor's mutation
                {
                    let r = inner.read().unwrap();
                    if r.contains_key("actor_private") {
                        violations_sender.fetch_add(1, Ordering::SeqCst);
                    }
                    if !r.contains_key("sender_key_0") {
                        violations_sender.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let expected = NUM_THREADS * ITERS;
    let deadline = Instant::now() + Duration::from_secs(10);
    while processed.load(Ordering::SeqCst) < expected && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(processed.load(Ordering::SeqCst), expected);
    assert_eq!(
        violations.load(Ordering::SeqCst),
        0,
        "State isolation violations detected during concurrent map mutation!"
    );

    actor.stop();
}

#[test]
fn test_state_isolation_nested_struct_map_array_mutation() {
    let reply_chan = Channel::new(None);
    let rx_reply = Arc::clone(&reply_chan);

    let actor = spawn_native(Arc::new(move |msg: Value| {
        // Actor mutates deeply nested structures inside received StructInstance
        if let Value::StructInstance { ref fields, .. } = msg {
            let fields_guard = fields.read().unwrap();
            if let Some(Value::Map(m)) = fields_guard.get("config") {
                if let Some(Value::Array(a)) = m.read().unwrap().get("tags") {
                    a.write()
                        .unwrap()
                        .push(Value::String("ACTOR_INJECTED".into()));
                }
                m.write()
                    .unwrap()
                    .insert("actor_status".into(), Value::String("OK".into()));
            }
        }
        rx_reply.send(Value::Bool(true), Span::new(1, 1)).unwrap();
    }));

    // Construct deeply nested: StructInstance -> Map -> Array
    let tags_inner = Arc::new(RwLock::new(vec![
        Value::String("alpha".into()),
        Value::String("beta".into()),
    ]));
    let mut config_map = IndexMap::new();
    config_map.insert("tags".into(), Value::Array(Arc::clone(&tags_inner)));
    let config_inner = Arc::new(RwLock::new(config_map));

    let mut struct_fields = IndexMap::new();
    struct_fields.insert("config".into(), Value::Map(Arc::clone(&config_inner)));
    let fields_inner = Arc::new(RwLock::new(struct_fields));

    let instance = Value::StructInstance {
        name: "ServiceConfig".into(),
        fields: Arc::clone(&fields_inner),
    };

    actor.send(instance.clone()).expect("send failed");

    // Sender mutates its own tags array after send
    tags_inner
        .write()
        .unwrap()
        .push(Value::String("SENDER_LOCAL".into()));

    // Wait for actor to complete its mutation
    reply_chan
        .recv(Some(5000), Span::new(1, 1))
        .expect("actor timed out");

    // 1. Sender tags array must contain "SENDER_LOCAL", but NOT "ACTOR_INJECTED"
    {
        let tags = tags_inner.read().unwrap();
        assert!(tags.contains(&Value::String("SENDER_LOCAL".into())));
        assert!(
            !tags.contains(&Value::String("ACTOR_INJECTED".into())),
            "Actor mutation leaked into sender tags array!"
        );
    }

    // 2. Sender config map must NOT contain "actor_status"
    {
        let config = config_inner.read().unwrap();
        assert!(
            !config.contains_key("actor_status"),
            "Actor mutation leaked into sender config map!"
        );
    }

    actor.stop();
}

// =============================================================================
// PART 3: Deep-Clone Edge Cases: Cycles & Deep Nesting
// =============================================================================

#[test]
fn test_deep_clone_mutual_cyclic_array() {
    let arr1_inner = Arc::new(RwLock::new(Vec::new()));
    let arr2_inner = Arc::new(RwLock::new(Vec::new()));

    let val1 = Value::Array(Arc::clone(&arr1_inner));
    let val2 = Value::Array(Arc::clone(&arr2_inner));

    // arr1 -> arr2, and arr2 -> arr1
    arr1_inner.write().unwrap().push(Value::Int(10));
    arr1_inner.write().unwrap().push(val2.clone());

    arr2_inner.write().unwrap().push(Value::Int(20));
    arr2_inner.write().unwrap().push(val1.clone());

    // Deep clone must terminate safely
    let cloned1 = val1.deep_clone();

    if let Value::Array(ref c1_inner) = cloned1 {
        // Assert independent memory allocation
        assert_ne!(
            Arc::as_ptr(c1_inner),
            Arc::as_ptr(&arr1_inner),
            "Cloned array must have fresh allocation"
        );

        let g1 = c1_inner.read().unwrap();
        assert_eq!(g1.len(), 2);
        assert_eq!(g1[0], Value::Int(10));

        // c1[1] points to cloned arr2
        if let Value::Array(c2_inner) = &g1[1] {
            assert_ne!(
                Arc::as_ptr(c2_inner),
                Arc::as_ptr(&arr2_inner),
                "Cloned arr2 must have fresh allocation"
            );

            let g2 = c2_inner.read().unwrap();
            assert_eq!(g2.len(), 2);
            assert_eq!(g2[0], Value::Int(20));

            // c2[1] points back to cloned arr1!
            if let Value::Array(loopback) = &g2[1] {
                assert_eq!(
                    Arc::as_ptr(c1_inner),
                    Arc::as_ptr(loopback),
                    "Mutual cycle topology must be preserved in clone!"
                );
            } else {
                panic!("Expected Value::Array in c2[1]");
            }
        } else {
            panic!("Expected Value::Array in c1[1]");
        }
    } else {
        panic!("Expected Value::Array");
    }
}

#[test]
fn test_deep_clone_direct_cyclic_map() {
    let map_inner = Arc::new(RwLock::new(IndexMap::new()));
    let map_val = Value::Map(Arc::clone(&map_inner));

    map_inner
        .write()
        .unwrap()
        .insert("data".into(), Value::Int(42));
    map_inner
        .write()
        .unwrap()
        .insert("self_ref".into(), map_val.clone());

    let cloned = map_val.deep_clone();

    if let Value::Map(ref c_inner) = cloned {
        assert_ne!(
            Arc::as_ptr(c_inner),
            Arc::as_ptr(&map_inner),
            "Cloned map must have independent allocation"
        );

        let g = c_inner.read().unwrap();
        assert_eq!(g.get("data"), Some(&Value::Int(42)));

        if let Some(Value::Map(loopback)) = g.get("self_ref") {
            assert_eq!(
                Arc::as_ptr(c_inner),
                Arc::as_ptr(loopback),
                "Direct map cycle topology must point to cloned self!"
            );
        } else {
            panic!("Expected cyclic Value::Map");
        }
    } else {
        panic!("Expected Value::Map");
    }
}

#[test]
fn test_deep_clone_mutual_cyclic_map() {
    let m1_inner = Arc::new(RwLock::new(IndexMap::new()));
    let m2_inner = Arc::new(RwLock::new(IndexMap::new()));

    let m1 = Value::Map(Arc::clone(&m1_inner));
    let m2 = Value::Map(Arc::clone(&m2_inner));

    m1_inner
        .write()
        .unwrap()
        .insert("name".into(), Value::String("m1".into()));
    m1_inner.write().unwrap().insert("link".into(), m2.clone());

    m2_inner
        .write()
        .unwrap()
        .insert("name".into(), Value::String("m2".into()));
    m2_inner.write().unwrap().insert("link".into(), m1.clone());

    let cloned1 = m1.deep_clone();

    if let Value::Map(ref c1_inner) = cloned1 {
        assert_ne!(Arc::as_ptr(c1_inner), Arc::as_ptr(&m1_inner));
        let g1 = c1_inner.read().unwrap();
        if let Some(Value::Map(c2_inner)) = g1.get("link") {
            assert_ne!(Arc::as_ptr(c2_inner), Arc::as_ptr(&m2_inner));
            let g2 = c2_inner.read().unwrap();
            if let Some(Value::Map(loopback)) = g2.get("link") {
                assert_eq!(
                    Arc::as_ptr(c1_inner),
                    Arc::as_ptr(loopback),
                    "Mutual map cycle must link back to cloned m1"
                );
            } else {
                panic!("Expected loopback Value::Map in m2.link");
            }
        } else {
            panic!("Expected Value::Map in m1.link");
        }
    } else {
        panic!("Expected Value::Map");
    }
}

#[test]
fn test_deep_clone_heterogeneous_cycle_array_map_struct() {
    // Array contains Map, Map contains StructInstance, StructInstance contains original Array!
    let arr_inner = Arc::new(RwLock::new(Vec::new()));
    let arr_val = Value::Array(Arc::clone(&arr_inner));

    let map_inner = Arc::new(RwLock::new(IndexMap::new()));
    let map_val = Value::Map(Arc::clone(&map_inner));

    let struct_fields = Arc::new(RwLock::new(IndexMap::new()));
    let struct_val = Value::StructInstance {
        name: "CycleNode".into(),
        fields: Arc::clone(&struct_fields),
    };

    // Link: arr -> map
    arr_inner.write().unwrap().push(map_val.clone());
    // Link: map -> struct
    map_inner
        .write()
        .unwrap()
        .insert("struct_node".into(), struct_val.clone());
    // Link: struct -> arr
    struct_fields
        .write()
        .unwrap()
        .insert("root_array".into(), arr_val.clone());

    let cloned_root = arr_val.deep_clone();

    // Verify termination, fresh allocations, and cyclic topology
    if let Value::Array(ref c_arr) = cloned_root {
        assert_ne!(Arc::as_ptr(c_arr), Arc::as_ptr(&arr_inner));
        let arr_guard = c_arr.read().unwrap();
        if let Value::Map(ref c_map) = arr_guard[0] {
            assert_ne!(Arc::as_ptr(c_map), Arc::as_ptr(&map_inner));
            let map_guard = c_map.read().unwrap();
            if let Some(Value::StructInstance {
                fields: c_struct, ..
            }) = map_guard.get("struct_node")
            {
                assert_ne!(Arc::as_ptr(c_struct), Arc::as_ptr(&struct_fields));
                let struct_guard = c_struct.read().unwrap();
                if let Some(Value::Array(loop_arr)) = struct_guard.get("root_array") {
                    assert_eq!(
                        Arc::as_ptr(c_arr),
                        Arc::as_ptr(loop_arr),
                        "Heterogeneous cycle must resolve back to cloned root array!"
                    );
                } else {
                    panic!("Expected root_array in cloned struct");
                }
            } else {
                panic!("Expected struct_node in cloned map");
            }
        } else {
            panic!("Expected map in cloned array");
        }
    } else {
        panic!("Expected Value::Array");
    }
}

#[test]
fn test_deep_clone_dag_diamond_sharing_preserved() {
    // Array root contains two references to the same child array
    let child_inner = Arc::new(RwLock::new(vec![Value::Int(100), Value::Int(200)]));
    let child_val = Value::Array(Arc::clone(&child_inner));

    let root_val = Value::Array(Arc::new(RwLock::new(vec![
        child_val.clone(),
        child_val.clone(),
    ])));

    let cloned_root = root_val.deep_clone();

    if let Value::Array(ref r_inner) = cloned_root {
        let guard = r_inner.read().unwrap();
        assert_eq!(guard.len(), 2);
        if let (Value::Array(c0), Value::Array(c1)) = (&guard[0], &guard[1]) {
            // Must not point to original child
            assert_ne!(Arc::as_ptr(c0), Arc::as_ptr(&child_inner));
            // Both cloned elements must share the SAME new child allocation (DAG preservation)
            assert_eq!(
                Arc::as_ptr(c0),
                Arc::as_ptr(c1),
                "DAG diamond sharing should be preserved in deep_clone"
            );
        } else {
            panic!("Expected Value::Array elements");
        }
    }
}

#[test]
fn test_deep_clone_nested_100() {
    println!("nested 100: building");
    let mut current = Value::Int(999);
    for _ in 0..100 {
        current = Value::Array(Arc::new(RwLock::new(vec![current])));
    }
    println!("nested 100: cloning");
    let cloned_100 = current.deep_clone();
    println!("nested 100: verifying");
    let mut leaf = cloned_100;
    for _ in 0..100 {
        if let Value::Array(arr) = leaf {
            leaf = arr.read().unwrap()[0].clone();
        } else {
            panic!("Expected array at depth");
        }
    }
    assert_eq!(leaf, Value::Int(999));
    println!("nested 100: done");
}

#[test]
fn test_deep_clone_nested_300() {
    println!("nested 300: building");
    let mut current = Value::Int(300);
    for _ in 0..300 {
        current = Value::Array(Arc::new(RwLock::new(vec![current])));
    }
    println!("nested 300: cloning");
    let cloned = current.deep_clone();
    println!("nested 300: verifying");
    let mut leaf = cloned;
    for _ in 0..300 {
        if let Value::Array(arr) = leaf {
            leaf = arr.read().unwrap()[0].clone();
        }
    }
    assert_eq!(leaf, Value::Int(300));
    println!("nested 300: done");
}

#[test]
fn test_deep_clone_nested_500() {
    println!("nested 500: building");
    let mut current = Value::Int(500);
    for _ in 0..500 {
        current = Value::Array(Arc::new(RwLock::new(vec![current])));
    }
    println!("nested 500: cloning");
    let cloned = current.deep_clone();
    println!("nested 500: verifying");
    let mut leaf = cloned;
    for _ in 0..500 {
        if let Value::Array(arr) = leaf {
            leaf = arr.read().unwrap()[0].clone();
        }
    }
    assert_eq!(leaf, Value::Int(500));
    println!("nested 500: done");
}

#[test]
fn test_actor_stop_queued_messages() {
    let processed = Arc::new(AtomicUsize::new(0));
    let proc_clone = Arc::clone(&processed);

    // Slow actor
    let actor = spawn_native(Arc::new(move |_msg: Value| {
        thread::sleep(Duration::from_millis(10));
        proc_clone.fetch_add(1, Ordering::SeqCst);
    }));

    // Send 10 messages
    for i in 0..10 {
        actor.send(Value::Int(i)).unwrap();
    }

    // Call stop immediately
    actor.stop();

    // Join
    let _ = actor.join();

    // Graceful stop contract: messages enqueued prior to stop() MUST be processed!
    assert_eq!(
        processed.load(Ordering::SeqCst),
        10,
        "Actor prematurely abandoned queued messages upon stop()!"
    );
}

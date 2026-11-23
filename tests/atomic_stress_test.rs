use serde_json::json;
use shae::{run, run_vm};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

// =========================================================================
// 1. 1,000,000-op Concurrent fetch_add Stress Test (10 Threads x 100,000)
// =========================================================================

#[test]
fn test_concurrent_fetch_add_1m_rust_threads() {
    // Direct multi-threaded saturation test on Value::AtomicInt
    let script = "let counter = AtomicInt(0); counter";
    let val = run(script).expect("Failed to initialize AtomicInt");
    let atom = match val {
        shae::value::Value::AtomicInt(ref a) => Arc::clone(a),
        _ => panic!("Expected Value::AtomicInt"),
    };

    let n_threads = 10;
    let ops_per_thread = 100_000;
    let mut handles = Vec::with_capacity(n_threads);

    for _ in 0..n_threads {
        let a = Arc::clone(&atom);
        handles.push(thread::spawn(move || {
            for _ in 0..ops_per_thread {
                a.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join().expect("Worker thread panicked");
    }

    assert_eq!(
        atom.load(Ordering::SeqCst),
        1_000_000,
        "All 1,000,000 atomic increments must be accounted for without lost updates"
    );
}

#[test]
fn test_concurrent_fetch_add_shae_spawn_stress() {
    // Concurrency stress test executed entirely within Shae runtime via spawn() & join()
    let script = r#"
let counter = AtomicInt(0)
let N_THREADS = 10
let OPS = 1000

let tasks = []
for t in range(N_THREADS) {
    let task = spawn(fn() {
        for i in range(OPS) {
            counter.fetch_add(1)
        }
    })
    push(tasks, task)
}

for t in tasks {
    join(t)
}

counter.load()
"#;
    let res = run(script).expect("Shae spawn stress test failed");
    assert_eq!(
        res.to_json(),
        json!(10_000),
        "Shae spawned tasks must accurately increment the atomic counter"
    );
}

// =========================================================================
// 2. CAS Spinlock Mutual Exclusion Test
// =========================================================================

#[test]
fn test_cas_spinlock_mutual_exclusion() {
    let script = r#"
let lock = AtomicBool(false)
let shared_data = [0]
let N_THREADS = 4
let N_ITERS = 100

let tasks = []
for t in range(N_THREADS) {
    let task = spawn(fn() {
        for i in range(N_ITERS) {
            // Spin until acquire lock via CAS
            while !lock.compare_and_swap(false, true, "acquire") {}

            // Critical section: deliberate non-atomic read-modify-write
            let cur = shared_data[0]
            shared_data[0] = cur + 1

            // Release lock
            lock.store(false, "release")
        }
    })
    push(tasks, task)
}

for t in tasks {
    join(t)
}

shared_data[0]
"#;
    let res = run(script).expect("CAS spinlock test failed");
    assert_eq!(
        res.to_json(),
        json!(400),
        "CAS spinlock must guarantee strict mutual exclusion across concurrent threads"
    );
}

// =========================================================================
// 3. Cross-Actor Atomic Sharing Test (deep_clone Arc Preservation)
// =========================================================================

#[test]
fn test_cross_actor_atomic_sharing() {
    let script = r#"
let actor = use "std:actor"

// Shared atomic counter
let shared_counter = AtomicInt(100)

// Spawn stateful actor that receives the atomic counter in its message
let worker = actor.spawn(fn(atom) {
    atom.fetch_add(250)
    "done"
})

// Send the AtomicInt directly across the actor boundary (deep_clone)
let reply = actor.ask(worker, shared_counter)
worker.stop()

// Read the shared counter from the main thread
[reply, shared_counter.load()]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!(["done", 350]),
        "Cross-actor message passing must preserve Arc pointer sharing for AtomicInt"
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        json!(["done", 350]),
        "VM Cross-actor message passing must preserve Arc pointer sharing for AtomicInt"
    );
}

#[test]
fn test_cross_actor_atomic_bool_coordination() {
    let script = r#"
let actor = use "std:actor"

let flag = AtomicBool(false)

let worker = actor.spawn(fn(b) {
    b.store(true, "release")
    "acknowledged"
})

let reply = actor.ask(worker, flag)
worker.stop()

[reply, flag.load("acquire")]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!(["acknowledged", true]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!(["acknowledged", true]));
}

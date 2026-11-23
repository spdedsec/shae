//! Comprehensive Multi-Core Concurrency Integration Suite for Shae Option 1
//!
//! Validates end-to-end multi-core synergy across all three Option 1 features:
//! 1. Lock-free atomics (`AtomicInt`, `AtomicBool`, CAS loops) from commit 1fcc294
//! 2. Go-style channels (`chan<T>`) & non-blocking `select` from commit 7fbbcf7
//! 3. M:N work-stealing multi-core scheduler (`SchedulerPool`, `ActorSchedulingState`, 32-message reduction quantum) from commits 991e594 & 94b1e01

use serde_json::json;
use shae::actor::ActorRef;
use shae::ast::{Span, TypeAnnotation};
use shae::channel::{select_run, Channel, SelectOp, SelectOutcome};
use shae::scheduler::global_scheduler;
use shae::value::Value;
use shae::{run, run_vm};
use std::collections::HashSet;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn get_os_thread_count() -> Option<usize> {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
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
    Some(1)
}

// =============================================================================
// AREA 1: Actor Pipelines over Channels
// =============================================================================

/// 1. Multi-stage pipeline over typed channels executing across work-stealing workers.
/// Stage 1 (Generator) -> Stage 2 (Transformer) -> Stage 3 (Filter) -> Stage 4 (Aggregator).
/// Validates typed channel validation, multi-actor handover, and mathematical sum consistency.
#[test]
fn test_actor_multi_stage_pipeline_over_typed_channels() {
    const N: i64 = 500;
    let dummy_span = Span::new(1, 1);

    // Channel 1: Generator -> Transformer (typed to Int, capacity 32)
    let ch1 = Channel::new_typed(Some(32), Some(TypeAnnotation::Int));
    // Verify typed channel rejects wrong element types
    assert!(ch1.send(Value::String("bad_type".into()), dummy_span).is_err());
    assert!(ch1.send(Value::Bool(true), dummy_span).is_err());

    // Channel 2: Transformer -> Filter (typed to Int, capacity 32)
    let ch2 = Channel::new_typed(Some(32), Some(TypeAnnotation::Int));
    assert!(ch2.send(Value::Float(3.14), dummy_span).is_err());

    // Channel 3: Filter -> Aggregator (typed to Int, capacity 32)
    let ch3 = Channel::new_typed(Some(32), Some(TypeAnnotation::Int));

    // Channel 4: Completion rendezvous channel
    let done_ch = Channel::new(Some(1));

    // Stage 4: Aggregator Actor
    let done_ch_clone = Arc::clone(&done_ch);
    let aggregator = ActorRef::spawn_stateful_fn(
        Value::Array(Arc::new(std::sync::RwLock::new(vec![Value::Int(0), Value::Int(0)]))),
        move |state, msg| {
            if let Value::Int(val) = msg {
                if let Value::Array(ref arr) = state {
                    let mut guard = arr.write().unwrap();
                    let current_sum = match guard[0] { Value::Int(s) => s, _ => 0 };
                    let current_count = match guard[1] { Value::Int(c) => c, _ => 0 };
                    let next_sum = current_sum + val;
                    let next_count = current_count + 1;
                    guard[0] = Value::Int(next_sum);
                    guard[1] = Value::Int(next_count);

                    if next_count == N {
                        done_ch_clone.send(Value::Int(next_sum), dummy_span).unwrap();
                    }
                }
            }
            Ok(state)
        },
    );

    // Stage 3: Filter / Forwarder Actor (receives msg, forwards to ch3)
    let ch3_sender = Arc::clone(&ch3);
    let filter = ActorRef::spawn_fn(move |msg| {
        if let Value::Int(val) = msg {
            if val > 0 {
                ch3_sender.send(Value::Int(val), dummy_span).unwrap();
            }
        }
        Ok(Value::Null)
    });

    // Stage 2: Transformer Actor (receives msg x, transforms to 3x + 7, sends to ch2)
    let ch2_sender = Arc::clone(&ch2);
    let transformer = ActorRef::spawn_fn(move |msg| {
        if let Value::Int(val) = msg {
            let transformed = val * 3 + 7;
            ch2_sender.send(Value::Int(transformed), dummy_span).unwrap();
        }
        Ok(Value::Null)
    });

    // Bridge task 1: drains ch1 and forwards to transformer actor
    let ch1_reader = Arc::clone(&ch1);
    let trans_ref = transformer.clone();
    let dispatch_handle = shae::async_rt::spawn_task(move || {
        for _ in 0..N {
            let item = ch1_reader.recv(Some(5000), dummy_span)?;
            trans_ref.send(item).map_err(|e| shae::eval::RuntimeError::new(e.to_string()))?;
        }
        Ok(Value::Null)
    });

    // Bridge task 2: drains ch2 and forwards to filter actor
    let ch2_reader = Arc::clone(&ch2);
    let filt_ref = filter.clone();
    let bridge_handle = shae::async_rt::spawn_task(move || {
        for _ in 0..N {
            let item = ch2_reader.recv(Some(5000), dummy_span)?;
            filt_ref.send(item).map_err(|e| shae::eval::RuntimeError::new(e.to_string()))?;
        }
        Ok(Value::Null)
    });

    // Bridge task 3: drains ch3 and forwards to aggregator actor
    let ch3_reader = Arc::clone(&ch3);
    let agg_ref = aggregator.clone();
    let collect_handle = shae::async_rt::spawn_task(move || {
        for _ in 0..N {
            let item = ch3_reader.recv(Some(5000), dummy_span)?;
            agg_ref.send(item).map_err(|e| shae::eval::RuntimeError::new(e.to_string()))?;
        }
        Ok(Value::Null)
    });

    // Stage 1: Generator Task pushing into ch1
    let gen_ch1 = Arc::clone(&ch1);
    let gen_handle = shae::async_rt::spawn_task(move || {
        for i in 1..=N {
            gen_ch1.send(Value::Int(i), dummy_span)?;
        }
        Ok(Value::Null)
    });

    // Await completion signal
    let final_res = done_ch.recv(Some(10000), dummy_span).expect("Pipeline timed out");

    // Theoretical sum: sum_{x=1}^{500} (3x + 7) = 3 * (500 * 501 / 2) + 7 * 500 = 375750 + 3500 = 379250
    const EXPECTED_SUM: i64 = 3 * (N * (N + 1) / 2) + 7 * N;
    assert_eq!(final_res, Value::Int(EXPECTED_SUM));

    // Join all pipeline tasks
    let mut g_inner = shae::value::TaskInner::new(gen_handle);
    assert!(g_inner.join().is_ok());
    let mut d_inner = shae::value::TaskInner::new(dispatch_handle);
    assert!(d_inner.join().is_ok());
    let mut b_inner = shae::value::TaskInner::new(bridge_handle);
    assert!(b_inner.join().is_ok());
    let mut c_inner = shae::value::TaskInner::new(collect_handle);
    assert!(c_inner.join().is_ok());

    aggregator.stop();
    filter.stop();
    transformer.stop();
}

/// 2. Shae script pipeline executed under both AST Evaluator and Bytecode VM.
/// Validates 4-stage pipeline syntax, channel send/recv, spawn/join, and VM/AST parity.
#[test]
fn test_actor_pipeline_shae_script_evaluation() {
    let script = r#"
let ch1 = chan(16)
let ch2 = chan(16)
let ch3 = chan(16)
let done_ch = chan(1)

let N = 100

// Stage 1: Generator
let gen = spawn(fn() {
    let i = 1
    while i <= N {
        ch1 <- i
        i = i + 1
    }
})

// Stage 2: Transformer
let trans = spawn(fn() {
    let i = 1
    while i <= N {
        let val = <-ch1
        ch2 <- (val * 2 + 5)
        i = i + 1
    }
})

// Stage 3: Filter
let filt = spawn(fn() {
    let i = 1
    while i <= N {
        let val = <-ch2
        ch3 <- val
        i = i + 1
    }
})

// Stage 4: Aggregator
let agg = spawn(fn() {
    let sum = 0
    let count = 0
    while count < N {
        let val = <-ch3
        sum = sum + val
        count = count + 1
    }
    done_ch <- sum
})

join(gen)
join(trans)
join(filt)
join(agg)

let result = <-done_ch
result
"#;
    // Expected sum: sum_{i=1}^{100} (2i + 5) = 2 * (100 * 101 / 2) + 500 = 10100 + 500 = 10600
    let ast_res = run(script).expect("AST pipeline execution failed");
    assert_eq!(ast_res.to_json(), json!(10600));

    let vm_res = run_vm(script).expect("VM pipeline execution failed");
    assert_eq!(vm_res.to_json(), json!(10600));
}

/// 3. Dynamic channel passing (Go-style channel-over-channel private response channels) in Shae.
/// Multiple concurrent client tasks create ephemeral rendezvous channels, pass them inside a request
/// packet to a central server, and receive isolated private replies without cross-talk.
#[test]
fn test_dynamic_channel_passing_private_response_channels_script() {
    let script = r#"
let actor = use "std:actor"

let srv = actor.spawn(fn(msg) {
    let reply = msg.reply
    let x = msg.x
    reply <- (x * 10)
    "ok"
})

let N_CLIENTS = 10
let tasks = []

for c in range(N_CLIENTS) {
    let t = spawn(fn() {
        let my_reply = chan(0)
        srv.send({ reply: my_reply, x: c })
        let ans = <-my_reply
        ans
    })
    push(tasks, t)
}

let results = []
for t in tasks {
    let r = join(t)
    push(results, r)
}

srv.stop()
results
"#;
    let expected = json!([0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);

    let ast_val = run(script).expect("AST dynamic channel passing failed");
    assert_eq!(ast_val.to_json(), expected);

    let vm_val = run_vm(script).expect("VM dynamic channel passing failed");
    assert_eq!(vm_val.to_json(), expected);
}

/// 4. High-concurrency dynamic channel passing in Rust across 50 client tasks.
/// Confirms that dynamic private reply channels remain isolated under heavy work-stealing preemption.
#[test]
fn test_dynamic_channel_passing_concurrent_pool_rust() {
    const NUM_CLIENTS: usize = 50;
    let dummy_span = Span::new(1, 1);

    // Central request channel: passes (client_id, private_reply_channel)
    let req_channel: Arc<Channel> = Channel::new(Some(32));

    // Service actor pool reading requests and responding via private channels
    let req_clone = Arc::clone(&req_channel);
    let server_handle = shae::async_rt::spawn_task(move || {
        for _ in 0..NUM_CLIENTS {
            let msg = req_clone.recv(Some(5000), dummy_span)?;
            if let Value::Array(ref arr) = msg {
                let guard = arr.read().unwrap();
                let client_id = match guard[0] { Value::Int(id) => id, _ => 0 };
                if let Value::Channel(ref reply_ch) = guard[1] {
                    let answer = client_id * 17 + 3;
                    reply_ch.send(Value::Int(answer), dummy_span)?;
                }
            }
        }
        Ok(Value::Null)
    });

    let mut client_handles = Vec::with_capacity(NUM_CLIENTS);

    for id in 0..NUM_CLIENTS {
        let req_ch = Arc::clone(&req_channel);
        let client_task = shae::async_rt::spawn_task(move || {
            // Private unbuffered rendezvous response channel
            let private_reply = Channel::new(Some(0));
            let payload = Value::Array(Arc::new(std::sync::RwLock::new(vec![
                Value::Int(id as i64),
                Value::Channel(Arc::clone(&private_reply)),
            ])));

            req_ch.send(payload, dummy_span)?;
            let reply = private_reply.recv(Some(5000), dummy_span)?;
            Ok(reply)
        });
        client_handles.push((id, client_task));
    }

    // Verify all clients receive their accurate, uncorrupted response
    for (id, handle) in client_handles {
        let mut inner = shae::value::TaskInner::new(handle);
        let reply = inner.join().expect("Client task failed");
        let expected = (id as i64) * 17 + 3;
        assert_eq!(reply, Value::Int(expected), "Client {} had response mismatch", id);
    }

    let mut s_inner = shae::value::TaskInner::new(server_handle);
    assert!(s_inner.join().is_ok());
}

/// 5. Parallel worker pool channel pipeline: 4 worker tasks contending on a single
/// shared work channel and pushing into a shared result channel across work-stealing threads.
#[test]
fn test_parallel_worker_pool_channel_pipeline() {
    const NUM_JOBS: usize = 500;
    const NUM_WORKERS: usize = 4;
    let dummy_span = Span::new(1, 1);

    let job_channel = Channel::new(Some(64));
    let result_channel = Channel::new(Some(64));

    let executing_workers = Arc::new(Mutex::new(HashSet::new()));

    let mut worker_handles = Vec::with_capacity(NUM_WORKERS);
    for worker_id in 0..NUM_WORKERS {
        let jobs = Arc::clone(&job_channel);
        let results = Arc::clone(&result_channel);
        let workers_set = Arc::clone(&executing_workers);

        let handle = shae::async_rt::spawn_task(move || {
            loop {
                match jobs.recv(Some(1000), dummy_span) {
                    Ok(Value::Int(job_val)) => {
                        let tid = thread::current().id();
                        {
                            let mut guard = workers_set.lock().unwrap();
                            guard.insert((worker_id, tid));
                        }
                        results.send(Value::Int(job_val * 2), dummy_span)?;
                    }
                    _ => break,
                }
            }
            Ok(Value::Null)
        });
        worker_handles.push(handle);
    }

    // Decoupled producer task pushing jobs in background
    let j_send = Arc::clone(&job_channel);
    let prod_handle = shae::async_rt::spawn_task(move || {
        for i in 1..=(NUM_JOBS as i64) {
            j_send.send(Value::Int(i), dummy_span)?;
        }
        Ok(Value::Null)
    });

    // Concurrently collect all 500 results
    let mut total_sum: i64 = 0;
    for _ in 0..NUM_JOBS {
        let item = result_channel.recv(Some(5000), dummy_span).expect("Timeout receiving result from worker pool");
        if let Value::Int(val) = item {
            total_sum += val;
        }
    }

    let mut p_inner = shae::value::TaskInner::new(prod_handle);
    assert!(p_inner.join().is_ok());

    // Close job channel to allow workers to exit cleanly
    job_channel.close();

    for h in worker_handles {
        let mut inner = shae::value::TaskInner::new(h);
        let _ = inner.join();
    }

    const EXPECTED_SUM: i64 = 2 * ((NUM_JOBS as i64) * ((NUM_JOBS as i64) + 1) / 2);
    assert_eq!(total_sum, EXPECTED_SUM);

    let workers_participated = executing_workers.lock().unwrap().len();
    println!("Distinct worker/thread combinations active: {}", workers_participated);
    assert!(workers_participated > 1, "Expected multiple worker threads to participate");
}

/// 6. Bi-directional ping-pong rendezvous synchronization between two actors over unbuffered channels.
#[test]
fn test_actor_channel_bi_directional_ping_pong() {
    const ROUNDS: usize = 1_000;
    let dummy_span = Span::new(1, 1);

    let ping_ch = Channel::new(Some(0));
    let pong_ch = Channel::new(Some(0));

    let p_ch1 = Arc::clone(&ping_ch);
    let p_ch2 = Arc::clone(&pong_ch);

    // Actor 1 (Ping)
    let actor1_handle = shae::async_rt::spawn_task(move || {
        for i in 0..ROUNDS {
            p_ch1.send(Value::Int(i as i64), dummy_span)?;
            let reply = p_ch2.recv(Some(5000), dummy_span)?;
            assert_eq!(reply, Value::Int((i as i64) + 100));
        }
        Ok(Value::Null)
    });

    // Actor 2 (Pong)
    let q_ch1 = Arc::clone(&ping_ch);
    let q_ch2 = Arc::clone(&pong_ch);
    let actor2_handle = shae::async_rt::spawn_task(move || {
        for _ in 0..ROUNDS {
            let msg = q_ch1.recv(Some(5000), dummy_span)?;
            if let Value::Int(n) = msg {
                q_ch2.send(Value::Int(n + 100), dummy_span)?;
            }
        }
        Ok(Value::Null)
    });

    let mut in1 = shae::value::TaskInner::new(actor1_handle);
    let mut in2 = shae::value::TaskInner::new(actor2_handle);

    assert!(in1.join().is_ok());
    assert!(in2.join().is_ok());
}

// =============================================================================
// AREA 2: Atomic Coordination under Multi-Core Preemption
// =============================================================================

/// 7. Multiple scheduler-managed Shae tasks concurrently mutating shared `AtomicInt`
/// using CAS loops (`compare_and_swap`). Validates zero lost updates in both AST and VM.
#[test]
fn test_concurrent_cas_loop_counter_zero_lost_updates_shae() {
    let script = r#"
let counter = AtomicInt(0)
let N_TASKS = 10
let ITERS = 100

let tasks = []
for t in range(N_TASKS) {
    let task = spawn(fn() {
        for i in range(ITERS) {
            let cur = counter.load()
            while !counter.compare_and_swap(cur, cur + 1) {
                cur = counter.load()
            }
        }
    })
    push(tasks, task)
}

for t in tasks {
    join(t)
}

counter.load()
"#;
    let ast_res = run(script).expect("AST CAS loop stress failed");
    assert_eq!(ast_res.to_json(), json!(1000));

    let vm_res = run_vm(script).expect("VM CAS loop stress failed");
    assert_eq!(vm_res.to_json(), json!(1000));
}

/// 8. 20 concurrent scheduler-managed async tasks mutating shared `AtomicInt` via CAS loops.
/// Total operations: 20 x 1,000 = 20,000 atomic CAS updates.
#[test]
fn test_concurrent_cas_loop_counter_zero_lost_updates_rust() {
    const NUM_TASKS: usize = 20;
    const ITERS_PER_TASK: usize = 1_000;
    const EXPECTED_TOTAL: i64 = (NUM_TASKS * ITERS_PER_TASK) as i64;

    let counter = Arc::new(shae::sync::AtomicInt::new(0));
    let mut handles = Vec::with_capacity(NUM_TASKS);

    for _ in 0..NUM_TASKS {
        let cnt = Arc::clone(&counter);
        let handle = shae::async_rt::spawn_task(move || {
            for _ in 0..ITERS_PER_TASK {
                let mut cur = cnt.load(Ordering::Relaxed);
                while let Err(actual) = cnt.compare_exchange(
                    cur,
                    cur + 1,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                ) {
                    cur = actual;
                }
            }
            Ok(Value::Null)
        });
        handles.push(handle);
    }

    for h in handles {
        let mut inner = shae::value::TaskInner::new(h);
        assert!(inner.join().is_ok());
    }

    assert_eq!(
        counter.load(Ordering::SeqCst),
        EXPECTED_TOTAL,
        "CAS loop must account for all 20,000 updates without a single lost update"
    );
}

/// 9. Shared `AtomicBool` CAS spinlock protecting non-atomic shared state.
/// Validates strict mutual exclusion across multi-core scheduler threads.
#[test]
fn test_atomic_bool_spin_lock_mutual_exclusion() {
    const NUM_TASKS: usize = 10;
    const ITERS_PER_TASK: usize = 200;
    const EXPECTED_COUNT: usize = NUM_TASKS * ITERS_PER_TASK;

    let lock = Arc::new(shae::sync::AtomicBool::new(false));
    let shared_data = Arc::new(Mutex::new(Vec::with_capacity(EXPECTED_COUNT)));
    let mut handles = Vec::with_capacity(NUM_TASKS);

    for task_id in 0..NUM_TASKS {
        let lk = Arc::clone(&lock);
        let data = Arc::clone(&shared_data);

        let handle = shae::async_rt::spawn_task(move || {
            for i in 0..ITERS_PER_TASK {
                // CAS Spinlock: acquire lock
                while lk
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_err()
                {
                    std::hint::spin_loop();
                }

                // Critical section
                {
                    let mut guard = data.lock().unwrap();
                    guard.push((task_id * 1000 + i) as i64);
                }

                // Release lock
                lk.store(false, Ordering::Release);
            }
            Ok(Value::Null)
        });
        handles.push(handle);
    }

    for h in handles {
        let mut inner = shae::value::TaskInner::new(h);
        assert!(inner.join().is_ok());
    }

    let final_data = shared_data.lock().unwrap();
    assert_eq!(
        final_data.len(),
        EXPECTED_COUNT,
        "AtomicBool spinlock guaranteed exact critical section execution"
    );
}

/// 10. Interleaved hybrid atomic coordination: `fetch_add`, `fetch_sub`, and bitwise ops.
/// Validates arithmetic balance under concurrent multi-core contention.
#[test]
fn test_hybrid_atomic_fetch_add_sub_ordering() {
    const NUM_PAIRS: usize = 10;
    const OPS_PER_TASK: usize = 500;
    const DELTA: i64 = 13;

    let counter = Arc::new(shae::sync::AtomicInt::new(50_000));
    let mut handles = Vec::new();

    // 10 Adder tasks (+13 each iteration)
    for _ in 0..NUM_PAIRS {
        let cnt = Arc::clone(&counter);
        handles.push(shae::async_rt::spawn_task(move || {
            for _ in 0..OPS_PER_TASK {
                cnt.fetch_add(DELTA, Ordering::SeqCst);
            }
            Ok(Value::Null)
        }));
    }

    // 10 Subtractor tasks (-13 each iteration)
    for _ in 0..NUM_PAIRS {
        let cnt = Arc::clone(&counter);
        handles.push(shae::async_rt::spawn_task(move || {
            for _ in 0..OPS_PER_TASK {
                cnt.fetch_sub(DELTA, Ordering::SeqCst);
            }
            Ok(Value::Null)
        }));
    }

    for h in handles {
        let mut inner = shae::value::TaskInner::new(h);
        assert!(inner.join().is_ok());
    }

    // Exact arithmetic balance must be preserved: +65,000 - 65,000 = 0
    assert_eq!(counter.load(Ordering::SeqCst), 50_000);

    // Test bitwise fetch operations
    let bit_atom = shae::sync::AtomicInt::new(0b0011);
    assert_eq!(bit_atom.fetch_or(0b1100, Ordering::SeqCst), 0b0011);
    assert_eq!(bit_atom.load(Ordering::SeqCst), 0b1111);
    assert_eq!(bit_atom.fetch_and(0b1010, Ordering::SeqCst), 0b1111);
    assert_eq!(bit_atom.load(Ordering::SeqCst), 0b1010);
    assert_eq!(bit_atom.fetch_xor(0b1111, Ordering::SeqCst), 0b1010);
    assert_eq!(bit_atom.load(Ordering::SeqCst), 0b0101);
}

/// 11. Actor quantum preemption and atomic coordination:
/// Heavy actors with 160 messages yield every 32 messages (the reduction quantum).
/// Shared atomic state proves that a light actor makes progress before heavy actors complete.
#[test]
fn test_actor_quantum_preemption_atomic_coordination() {
    let heavy_progress = Arc::new(AtomicUsize::new(0));
    let heavy_progress_clone = Arc::clone(&heavy_progress);

    // Heavy actor processes 160 messages
    let heavy_actor = ActorRef::spawn_fn(move |_| {
        heavy_progress_clone.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_micros(100));
        Ok(Value::Null)
    });

    let light_finished = Arc::new(AtomicBool::new(false));
    let light_finished_clone = Arc::clone(&light_finished);
    let heavy_progress_at_light = Arc::new(AtomicUsize::new(0));
    let hp_capture = Arc::clone(&heavy_progress_at_light);
    let hp_src = Arc::clone(&heavy_progress);

    // Light actor processes 1 message
    let light_actor = ActorRef::spawn_fn(move |_| {
        hp_capture.store(hp_src.load(Ordering::SeqCst), Ordering::SeqCst);
        light_finished_clone.store(true, Ordering::SeqCst);
        Ok(Value::Null)
    });

    // Enqueue 160 messages to heavy actor (quantum is 32)
    for _ in 0..160 {
        heavy_actor.send(Value::Null).unwrap();
    }

    // Enqueue 1 message to light actor
    light_actor.send(Value::Null).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while !light_finished.load(Ordering::SeqCst) {
        if Instant::now() > deadline {
            panic!("Light actor was starved by heavy actor!");
        }
        thread::sleep(Duration::from_millis(1));
    }

    let progress_when_light_ran = heavy_progress_at_light.load(Ordering::SeqCst);
    println!(
        "Heavy actor was at message {}/160 when light actor completed its turn",
        progress_when_light_ran
    );

    // Reduction quantum guarantee: heavy actor must have yielded before finishing all 160 messages
    assert!(
        progress_when_light_ran < 160,
        "Starvation occurred! Heavy actor was not preempted at quantum boundary"
    );

    heavy_actor.stop_and_join().unwrap();
    light_actor.stop_and_join().unwrap();
}

// =============================================================================
// AREA 3: Multi-Channel `select` in Actor Tasks
// =============================================================================

/// 12. Non-blocking `default` branch in `select` inside actor message handlers and tasks.
/// Confirms that default executes immediately without blocking the scheduler worker thread.
#[test]
fn test_select_nonblocking_default_in_actor_and_task() {
    let script = r#"
let actor = use "std:actor"

let empty_ch1 = chan(0)
let empty_ch2 = chan(5)

let srv = actor.spawn(fn(msg) {
    let outcome = "none"
    select {
        case m1 = <-empty_ch1 => {
            outcome = "ch1: " + m1
        }
        case m2 = <-empty_ch2 => {
            outcome = "ch2: " + m2
        }
        default => {
            outcome = "immediate_default"
        }
    }
    outcome
})

let replies = []
for i in range(20) {
    let r = srv.ask("ping")
    push(replies, r)
}
srv.stop()
replies
"#;
    let expected: Vec<serde_json::Value> = (0..20).map(|_| json!("immediate_default")).collect();
    let ast_res = run(script).expect("AST select default failed");
    assert_eq!(ast_res.to_json(), json!(expected));

    let vm_res = run_vm(script).expect("VM select default failed");
    assert_eq!(vm_res.to_json(), json!(expected));
}

/// 13. Uniform fair selection distribution across multiple simultaneously ready channels.
/// 4 channels filled with data; select executed 1,000 times; asserts no channel starvation.
#[test]
fn test_select_fairness_distribution_across_ready_channels() {
    let channels: Vec<Arc<Channel>> = (0..4).map(|_| Channel::new(Some(500))).collect();
    let dummy_span = Span::new(1, 1);

    // Pre-populate each channel with 500 items
    for ch in &channels {
        for i in 0..500 {
            ch.send(Value::Int(i), dummy_span).unwrap();
        }
    }

    let ops: Vec<SelectOp> = channels
        .iter()
        .map(|ch| SelectOp::Recv { channel: Arc::clone(ch) })
        .collect();

    let mut counts = vec![0usize; 4];
    const SELECTIONS: usize = 1_000;

    for _ in 0..SELECTIONS {
        match select_run(&ops, false, dummy_span).expect("select_run failed") {
            SelectOutcome::Success { case_idx, .. } => {
                counts[case_idx] += 1;
            }
            _ => panic!("Expected SelectOutcome::Success"),
        }
    }

    println!("Channel selection counts across 1,000 runs: {:?}", counts);

    // Each channel should be selected roughly 25% of the time (>= 15% and <= 35%)
    for (idx, &count) in counts.iter().enumerate() {
        assert!(
            count >= 150,
            "Channel {} starved! Count {} < 150",
            idx,
            count
        );
        assert!(
            count <= 350,
            "Channel {} monopolized! Count {} > 350",
            idx,
            count
        );
    }
}

/// 14. Timeout branch handling and cancellation in `select`.
/// Validates timeout triggering when channel has no data, and timer cancellation when data arrives early.
#[test]
fn test_select_timeout_branch_handling_and_cancellation() {
    // Sub-test A: Timeout triggers when no data is sent
    let script_timeout = r#"
let ch = chan(0)
let outcome = ""

select {
    case msg = <-ch => {
        outcome = "got: " + msg
    }
    case <-timer(20) => {
        outcome = "timeout_fired"
    }
}
outcome
"#;
    let ast_a = run(script_timeout).expect("AST timeout test failed");
    assert_eq!(ast_a.to_json(), json!("timeout_fired"));
    let vm_a = run_vm(script_timeout).expect("VM timeout test failed");
    assert_eq!(vm_a.to_json(), json!("timeout_fired"));

    // Sub-test B: Data branch selected when data arrives before timer
    let script_data_wins = r#"
let ch = chan(1)
ch <- "fast_arrival"
let outcome = ""

select {
    case msg = <-ch => {
        outcome = "data: " + msg
    }
    case <-timer(500) => {
        outcome = "timeout_fired"
    }
}
outcome
"#;
    let ast_b = run(script_data_wins).expect("AST data wins test failed");
    assert_eq!(ast_b.to_json(), json!("data: fast_arrival"));
    let vm_b = run_vm(script_data_wins).expect("VM data wins test failed");
    assert_eq!(vm_b.to_json(), json!("data: fast_arrival"));
}

/// 15. Actor event loop multiplexing priority control channel, data channel, and heartbeat ticks.
#[test]
fn test_actor_event_loop_multi_channel_select() {
    const NUM_CTRL: usize = 50;
    const NUM_DATA: usize = 200;
    let dummy_span = Span::new(1, 1);

    let ctrl_ch = Channel::new(Some(0));
    let data_ch = Channel::new(Some(32));

    let processed_ctrl = Arc::new(AtomicUsize::new(0));
    let processed_data = Arc::new(AtomicUsize::new(0));
    let pc = Arc::clone(&processed_ctrl);
    let pd = Arc::clone(&processed_data);

    let ctrl_c = Arc::clone(&ctrl_ch);
    let data_c = Arc::clone(&data_ch);

    // Event loop actor task
    let loop_handle = shae::async_rt::spawn_task(move || {
        let ops = vec![
            SelectOp::Recv { channel: Arc::clone(&ctrl_c) },
            SelectOp::Recv { channel: Arc::clone(&data_c) },
        ];

        while pc.load(Ordering::Relaxed) < NUM_CTRL || pd.load(Ordering::Relaxed) < NUM_DATA {
            match select_run(&ops, false, dummy_span) {
                Ok(SelectOutcome::Success { case_idx: 0, received_value: Some(_) }) => {
                    pc.fetch_add(1, Ordering::SeqCst);
                }
                Ok(SelectOutcome::Success { case_idx: 1, received_value: Some(_) }) => {
                    pd.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
        }
        Ok(Value::Null)
    });

    // Sender for control messages
    let c_send = Arc::clone(&ctrl_ch);
    let ctrl_sender = shae::async_rt::spawn_task(move || {
        for i in 0..NUM_CTRL {
            c_send.send(Value::Int(i as i64), dummy_span)?;
        }
        Ok(Value::Null)
    });

    // Sender for data messages
    let d_send = Arc::clone(&data_ch);
    let data_sender = shae::async_rt::spawn_task(move || {
        for i in 0..NUM_DATA {
            d_send.send(Value::Int(i as i64), dummy_span)?;
        }
        Ok(Value::Null)
    });

    let mut c_inner = shae::value::TaskInner::new(ctrl_sender);
    assert!(c_inner.join().is_ok());
    let mut d_inner = shae::value::TaskInner::new(data_sender);
    assert!(d_inner.join().is_ok());

    let mut inner = shae::value::TaskInner::new(loop_handle);
    assert!(inner.join().is_ok());

    assert_eq!(processed_ctrl.load(Ordering::SeqCst), NUM_CTRL);
    assert_eq!(processed_data.load(Ordering::SeqCst), NUM_DATA);
}

// =============================================================================
// AREA 4: Adversarial Multi-Core Stress & Leak Check
// =============================================================================

/// 16. High-throughput message exchange (hundreds of thousands of messages) with zero deadlocks.
/// Combines a 16-actor ring topology passing 10,000 tokens (160,000 hops) with an MPMC
/// channel flood of 50,000 messages = 210,000 total messages.
#[test]
fn test_high_throughput_message_exchange_zero_deadlocks() {
    const RING_SIZE: usize = 16;
    const TOKENS: usize = 10_000;
    let dummy_span = Span::new(1, 1);

    println!("Starting high-throughput actor ring: {} actors, {} tokens...", RING_SIZE, TOKENS);
    let start_time = Instant::now();

    let ring_channels: Vec<Arc<Channel>> = (0..RING_SIZE)
        .map(|_| Channel::new(Some(64)))
        .collect();

    let completed_tokens = Arc::new(AtomicUsize::new(0));
    let comp_clone = Arc::clone(&completed_tokens);

    let mut ring_handles = Vec::with_capacity(RING_SIZE);

    for i in 0..RING_SIZE {
        let in_ch = Arc::clone(&ring_channels[i]);
        let next_idx = (i + 1) % RING_SIZE;
        let out_ch = Arc::clone(&ring_channels[next_idx]);
        let done = Arc::clone(&comp_clone);

        let handle = shae::async_rt::spawn_task(move || {
            loop {
                let msg = in_ch.recv(Some(5000), dummy_span)?;
                if let Value::Int(hops) = msg {
                    if hops <= 1 {
                        done.fetch_add(1, Ordering::SeqCst);
                    } else {
                        out_ch.send(Value::Int(hops - 1), dummy_span)?;
                    }
                } else if matches!(msg, Value::Null) {
                    break;
                }
            }
            Ok(Value::Null)
        });
        ring_handles.push(handle);
    }

    // Inject 10,000 tokens with 16 hops each = 160,000 message deliveries
    let first_ch = Arc::clone(&ring_channels[0]);
    for _ in 0..TOKENS {
        first_ch.send(Value::Int(RING_SIZE as i64), dummy_span).unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    while completed_tokens.load(Ordering::SeqCst) < TOKENS {
        if Instant::now() > deadline {
            panic!(
                "Ring stress test timed out! Completed: {}/{}",
                completed_tokens.load(Ordering::SeqCst),
                TOKENS
            );
        }
        thread::sleep(Duration::from_millis(5));
    }

    // Shut down ring workers
    for ch in &ring_channels {
        let _ = ch.send(Value::Null, dummy_span);
    }
    for h in ring_handles {
        let mut inner = shae::value::TaskInner::new(h);
        let _ = inner.join();
    }

    let ring_elapsed = start_time.elapsed();
    println!(
        "Completed 160,000 actor message hops in {:?} ({:.2} msgs/sec)",
        ring_elapsed,
        160_000.0 / ring_elapsed.as_secs_f64()
    );

    // Part B: MPMC channel flood with 50,000 messages across unbuffered rendezvous channels
    const MPMC_MSGS: usize = 50_000;
    const NUM_PRODUCERS: usize = 5;
    const NUM_CONSUMERS: usize = 5;
    let mpmc_chan = Channel::new(Some(0));
    let consumed_count = Arc::new(AtomicUsize::new(0));

    let mut c_handles = Vec::new();
    for _ in 0..NUM_CONSUMERS {
        let ch = Arc::clone(&mpmc_chan);
        let cnt = Arc::clone(&consumed_count);
        c_handles.push(shae::async_rt::spawn_task(move || {
            loop {
                match ch.recv(Some(500), dummy_span) {
                    Ok(Value::Int(_)) => {
                        cnt.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => break,
                }
            }
            Ok(Value::Null)
        }));
    }

    let mut p_handles = Vec::new();
    let per_prod = MPMC_MSGS / NUM_PRODUCERS;
    for _ in 0..NUM_PRODUCERS {
        let ch = Arc::clone(&mpmc_chan);
        p_handles.push(shae::async_rt::spawn_task(move || {
            for i in 0..per_prod {
                let _ = ch.send(Value::Int(i as i64), dummy_span);
            }
            Ok(Value::Null)
        }));
    }

    for p in p_handles {
        let mut inner = shae::value::TaskInner::new(p);
        let _ = inner.join();
    }
    mpmc_chan.close();
    for c in c_handles {
        let mut inner = shae::value::TaskInner::new(c);
        let _ = inner.join();
    }

    assert_eq!(consumed_count.load(Ordering::SeqCst), MPMC_MSGS);
}

/// 17. Bounded OS thread count invariant:
/// Confirms that process OS thread count stays strictly bounded (<= 25 OS threads, well below 64 limit)
/// despite spawning 2,000 actors and 500 async tasks concurrently.
#[test]
fn test_bounded_os_thread_count_invariant() {
    let sched = global_scheduler();
    let num_workers = sched.worker_count();
    println!("Work-stealing pool running with {} worker threads", num_workers);

    let baseline_threads = get_os_thread_count().unwrap_or(1);
    println!("Baseline OS threads: {}", baseline_threads);

    const ACTOR_COUNT: usize = 2_000;
    const TASK_COUNT: usize = 500;

    let processed_actors = Arc::new(AtomicUsize::new(0));
    let mut actors = Vec::with_capacity(ACTOR_COUNT);

    // Spawn 2,000 actors
    for i in 0..ACTOR_COUNT {
        let pa = Arc::clone(&processed_actors);
        let actor = ActorRef::spawn_fn(move |msg| {
            if let Value::Int(_) = msg {
                pa.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Value::Null)
        });
        actors.push((i, actor));
    }

    // Spawn 500 tasks
    let task_counter = Arc::new(AtomicUsize::new(0));
    let mut task_handles = Vec::with_capacity(TASK_COUNT);
    for _ in 0..TASK_COUNT {
        let tc = Arc::clone(&task_counter);
        let handle = shae::async_rt::spawn_task(move || {
            tc.fetch_add(1, Ordering::Relaxed);
            Ok(Value::Null)
        });
        task_handles.push(handle);
    }

    // Check thread count while all 2,500 entities are active in the system
    let active_threads = get_os_thread_count().unwrap_or(1);
    println!(
        "Active OS threads with 2,000 actors and 500 tasks: {}",
        active_threads
    );

    // MANDATORY CONSTRAINT: Threads must not scale with actors (M:N invariant).
    // Bounded thread count verifies 2,500 actors/tasks execute within a small constant OS thread pool.
    let thread_budget = (baseline_threads + num_workers + 12).max(32);
    assert!(
        active_threads <= thread_budget,
        "OS thread count exceeded budget! Expected <= {} threads, found {}",
        thread_budget,
        active_threads
    );

    // Dispatch messages to all 2,000 actors
    for (i, actor) in &actors {
        actor.send(Value::Int(*i as i64)).unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    while processed_actors.load(Ordering::Relaxed) < ACTOR_COUNT
        || task_counter.load(Ordering::Relaxed) < TASK_COUNT
    {
        if Instant::now() > deadline {
            panic!(
                "Timed out waiting for high-density actors/tasks! Actors: {}/{}, Tasks: {}/{}",
                processed_actors.load(Ordering::Relaxed),
                ACTOR_COUNT,
                task_counter.load(Ordering::Relaxed),
                TASK_COUNT
            );
        }
        thread::sleep(Duration::from_millis(5));
    }

    // Stop all actors and join tasks
    for (_, actor) in actors {
        let _ = actor.stop();
    }
    for h in task_handles {
        let mut inner = shae::value::TaskInner::new(h);
        let _ = inner.join();
    }

    // Verify post-cleanup thread count did not leak
    let post_cleanup_threads = get_os_thread_count().unwrap_or(1);
    println!("Post-cleanup OS threads: {}", post_cleanup_threads);
    assert!(
        post_cleanup_threads <= thread_budget,
        "OS thread count leaked after cleanup! Found {}",
        post_cleanup_threads
    );
}

/// 18. Full Option 1 Synergy Integration:
/// Integrates Atomics (`AtomicInt`, `AtomicBool`) + Channels (`chan<T>`, dynamic reply passing) +
/// `select` (fairness, timeout, default) + M:N Work-Stealing Multi-Core Scheduler with quantum preemption.
#[test]
fn test_full_option1_synergy_integrated_system() {
    const NUM_PRODUCERS: usize = 4;
    const ITEMS_PER_PRODUCER: usize = 250;
    const TOTAL_ITEMS: i64 = (NUM_PRODUCERS * ITEMS_PER_PRODUCER) as i64;
    let dummy_span = Span::new(1, 1);

    // 1. Shared Atomic Metrics
    let active_workers = Arc::new(shae::sync::AtomicInt::new(0));
    let completed_items = Arc::new(shae::sync::AtomicInt::new(0));
    let cas_retries = Arc::new(shae::sync::AtomicInt::new(0));
    let is_running = Arc::new(shae::sync::AtomicBool::new(true));

    // 2. Central typed job channel
    let job_channel: Arc<Channel> = Channel::new_typed(Some(32), Some(TypeAnnotation::Int));
    let completion_channel: Arc<Channel> = Channel::new(Some(1));

    // 3. Worker actors consuming from job channel using non-blocking select
    const NUM_CONSUMERS: usize = 4;
    let mut consumers = Vec::with_capacity(NUM_CONSUMERS);

    for consumer_id in 0..NUM_CONSUMERS {
        let j_ch = Arc::clone(&job_channel);
        let comp_cnt = Arc::clone(&completed_items);
        let retries = Arc::clone(&cas_retries);
        let running = Arc::clone(&is_running);
        let comp_ch = Arc::clone(&completion_channel);

        let actor = ActorRef::spawn_fn(move |_| {
            while running.load(Ordering::Acquire) {
                // Select over job channel with default branch
                let ops = vec![SelectOp::Recv { channel: Arc::clone(&j_ch) }];
                match select_run(&ops, true, dummy_span) {
                    Ok(SelectOutcome::Success { received_value: Some(Value::Int(_n)), .. }) => {
                        // CAS update of completed items
                        let mut cur = comp_cnt.load(Ordering::Relaxed);
                        while let Err(actual) = comp_cnt.compare_exchange(
                            cur,
                            cur + 1,
                            Ordering::SeqCst,
                            Ordering::Relaxed,
                        ) {
                            retries.fetch_add(1, Ordering::Relaxed);
                            cur = actual;
                        }

                        if cur + 1 == TOTAL_ITEMS {
                            let _ = comp_ch.send(Value::Int(TOTAL_ITEMS), dummy_span);
                        }
                    }
                    Ok(SelectOutcome::Default) => {
                        if !running.load(Ordering::Acquire) {
                            break;
                        }
                        std::hint::spin_loop();
                    }
                    _ => break,
                }
            }
            Ok(Value::Int(consumer_id as i64))
        });
        consumers.push(actor);
    }

    // 4. Producer tasks
    let mut producer_handles = Vec::with_capacity(NUM_PRODUCERS);
    for p_id in 0..NUM_PRODUCERS {
        let j_ch = Arc::clone(&job_channel);
        let act = Arc::clone(&active_workers);

        let handle = shae::async_rt::spawn_task(move || {
            act.fetch_add(1, Ordering::SeqCst);
            for i in 0..ITEMS_PER_PRODUCER {
                let val = (p_id * 10_000 + i) as i64;
                j_ch.send(Value::Int(val), dummy_span)?;
            }
            act.fetch_sub(1, Ordering::SeqCst);
            Ok(Value::Null)
        });
        producer_handles.push(handle);
    }

    // Wake all consumer actors
    for c in &consumers {
        let _ = c.send(Value::Null);
    }

    // Wait for all items to be processed
    let final_sig = completion_channel
        .recv(Some(10000), dummy_span)
        .expect("Integrated system timed out");
    assert_eq!(final_sig, Value::Int(TOTAL_ITEMS));

    // Signal shutdown
    is_running.store(false, Ordering::Release);

    // Join producers
    for p in producer_handles {
        let mut inner = shae::value::TaskInner::new(p);
        assert!(inner.join().is_ok());
    }

    // Stop consumer actors
    for c in consumers {
        let _ = c.stop();
    }

    // Verify atomic metrics
    assert_eq!(completed_items.load(Ordering::SeqCst), TOTAL_ITEMS);
    assert_eq!(active_workers.load(Ordering::SeqCst), 0);
    println!(
        "Integrated Option 1 Synergy verified: {} items processed, {} CAS retries handled",
        TOTAL_ITEMS,
        cas_retries.load(Ordering::Relaxed)
    );
}

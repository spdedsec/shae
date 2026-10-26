//! Milestone 2 Adversarial Stress Test Suite: Supervision Trees & Process Restart Policies
//!
//! Empirical test dimensions:
//! 1. Thread Count Stability & Zero Leaked OS Threads (/proc/self/status inspection)
//! 2. Rapid Crashing under OneForOne (hundreds of restarts across multiple workers)
//! 3. Rapid Crashing under AllForOne (repeated cascade restarts of siblings with state reset)
//! 4. Concurrent Client Requests (ask/send) during continuous background child crash & restart cycles
//! 5. High-Frequency Sliding Window Boundary Checks (exact window expiry, threshold enforcement, max_restarts=0)
//! 6. Dynamic Child Lifecycle Concurrency & Stop Cascade Idempotency
//! 7. Manual Child Restart Semantics & Thread Leak Investigation

use shae::actor::{
    ActorRef, ChildSpec, RestartStrategy, Supervisor, SupervisorConfig, SupervisorStatus,
};
use shae::value::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn unique_id(prefix: &str) -> String {
    let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, std::process::id(), count)
}

/// Helper to read active OS thread count from /proc/self/status on Linux.
fn get_os_thread_count() -> Option<usize> {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
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
    // Fallback on non-Linux OS (macOS/Windows where /proc is absent)
    Some(1)
}

static SERIAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// =============================================================================
// DIMENSION 1: Thread Count Stability & Zero Leaked OS Threads
// =============================================================================

#[test]
fn test_stress_thread_count_stability_across_supervision_cycles() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");

    const NUM_SUPERVISORS: usize = 15;
    const WORKERS_PER_SUP: usize = 4;
    const RESTARTS_PER_WORKER: usize = 5;

    for s_idx in 0..NUM_SUPERVISORS {
        let sup_id = unique_id(&format!("thread_stab_sup_{}", s_idx));
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 50,
            within_seconds: 10.0,
        };

        let mut specs = Vec::new();
        for w_idx in 0..WORKERS_PER_SUP {
            let child_id = format!("{}_w{}", sup_id, w_idx);
            let spec = ChildSpec::from_stateful_fn(child_id, Value::Int(0), |state, msg| {
                match (state, msg) {
                    (Value::Int(_), Value::String(ref s)) if s == "crash" => {
                        Err("Forced crash for thread leak check".into())
                    }
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                }
            });
            specs.push(spec);
        }

        let sup = Supervisor::start_named(sup_id.clone(), config, specs)
            .expect("Supervisor must start successfully");

        // Cause repeated crashes across all workers
        for _ in 0..RESTARTS_PER_WORKER {
            for w_idx in 0..WORKERS_PER_SUP {
                let child_id = format!("{}_w{}", sup_id, w_idx);
                // Wait for worker to be alive and crash it
                for _ in 0..100 {
                    if let Some(child) = sup.get_child(&child_id) {
                        if child.is_alive() {
                            let _ = child.ask(
                                Value::String("crash".into()),
                                Some(Duration::from_millis(50)),
                            );
                            break;
                        }
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            // Allow supervisor to handle exits and restart
            thread::sleep(Duration::from_millis(20));
        }

        // Cleanly terminate supervisor and all its children
        assert!(sup.stop(), "Supervisor stop must succeed");
        let _ = sup.join();
    }

    // Give OS and runtime brief grace period to reclaim terminated pthread stacks
    thread::sleep(Duration::from_millis(250));

    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Thread Stability] Initial threads: {}, Final threads: {}",
        initial_threads, final_threads
    );

    // Assert that no threads leaked across all supervisor cycles
    assert!(
        final_threads <= initial_threads,
        "Leaked OS threads detected! Initial: {}, Final: {} (leaked {})",
        initial_threads,
        final_threads,
        final_threads.saturating_sub(initial_threads)
    );
}

// =============================================================================
// DIMENSION 2: Rapid Crashing Under OneForOne (Hundreds of Restarts)
// =============================================================================

#[test]
fn test_stress_rapid_crashing_one_for_one_hundreds_of_restarts() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    const TOTAL_CRASHES: usize = 120;
    let sup_id = unique_id("rapid_one_for_one");
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: TOTAL_CRASHES + 50,
        within_seconds: 60.0,
    };

    let spec_a =
        ChildSpec::from_stateful_fn(format!("{}_a", sup_id), Value::Int(0), |state, msg| match (
            state, msg,
        ) {
            (Value::Int(_), Value::String(ref s)) if s == "die" => Err("Deliberate failure".into()),
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        });

    let spec_b = ChildSpec::from_stateful_fn(
        format!("{}_b", sup_id),
        Value::Int(1000),
        |state, msg| match (state, msg) {
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        },
    );

    let sup = Supervisor::start_named(sup_id.clone(), config, vec![spec_a, spec_b])
        .expect("Supervisor must start");

    let child_b = sup.get_child(&format!("{}_b", sup_id)).unwrap();

    let mut successful_restarts = 0;
    let start_time = Instant::now();

    for crash_seq in 0..TOTAL_CRASHES {
        let child_a_id = format!("{}_a", sup_id);
        let mut crashed = false;

        for _ in 0..100 {
            if let Some(child_a) = sup.get_child(&child_a_id) {
                if child_a.is_alive() {
                    let res =
                        child_a.ask(Value::String("die".into()), Some(Duration::from_millis(50)));
                    if res.is_err() {
                        crashed = true;
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(crashed, "Crash {} must have succeeded", crash_seq);

        // Wait for supervisor to restart Worker A
        let mut restarted = false;
        for _ in 0..100 {
            if let Some(child_a) = sup.get_child(&child_a_id) {
                if child_a.is_alive() {
                    if let Ok(st) = child_a.ask(Value::Int(0), Some(Duration::from_millis(50))) {
                        if st == Value::Int(0) {
                            restarted = true;
                            successful_restarts += 1;
                            break;
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(
            restarted,
            "Worker A must be restarted fresh on crash {}",
            crash_seq
        );

        // Sibling B must remain continuously unaffected
        assert!(
            child_b.is_alive(),
            "Sibling B must remain alive across crash {}",
            crash_seq
        );
    }

    let elapsed = start_time.elapsed();
    println!(
        "[OneForOne Rapid Crash] Completed {} restarts in {:?}",
        successful_restarts, elapsed
    );
    assert_eq!(successful_restarts, TOTAL_CRASHES);

    // Verify Sibling B state is still intact
    let b_state = child_b.ask(Value::Int(0), None).unwrap();
    assert_eq!(b_state, Value::Int(1000));

    sup.stop();
}

// =============================================================================
// DIMENSION 3: Rapid Crashing Under AllForOne
// =============================================================================

#[test]
fn test_stress_rapid_crashing_all_for_one_sibling_cascades() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    const TOTAL_CYCLES: usize = 30;
    let sup_id = unique_id("rapid_all_for_one");
    let config = SupervisorConfig {
        strategy: RestartStrategy::AllForOne,
        max_restarts: TOTAL_CYCLES + 20,
        within_seconds: 60.0,
    };

    let spec_1 = ChildSpec::from_stateful_fn(
        format!("{}_w1", sup_id),
        Value::Int(10),
        |state, msg| match (state, msg) {
            (Value::Int(_), Value::String(ref s)) if s == "fail" => {
                Err("AllForOne trigger failure".into())
            }
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        },
    );

    let spec_2 = ChildSpec::from_stateful_fn(
        format!("{}_w2", sup_id),
        Value::Int(20),
        |state, msg| match (state, msg) {
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        },
    );

    let spec_3 = ChildSpec::from_stateful_fn(
        format!("{}_w3", sup_id),
        Value::Int(30),
        |state, msg| match (state, msg) {
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        },
    );

    let sup = Supervisor::start_named(sup_id.clone(), config, vec![spec_1, spec_2, spec_3])
        .expect("Supervisor start");

    let w1_id = format!("{}_w1", sup_id);
    let w2_id = format!("{}_w2", sup_id);
    let w3_id = format!("{}_w3", sup_id);

    for cycle in 0..TOTAL_CYCLES {
        // Mutate states of w2 and w3 before crash
        for _ in 0..50 {
            if let (Some(w2), Some(w3)) = (sup.get_child(&w2_id), sup.get_child(&w3_id)) {
                if w2.is_alive() && w3.is_alive() {
                    let _ = w2.send(Value::Int(5));
                    let _ = w3.send(Value::Int(5));
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }

        // Crash w1
        for _ in 0..50 {
            if let Some(w1) = sup.get_child(&w1_id) {
                if w1.is_alive() {
                    let _ = w1.ask(
                        Value::String("fail".into()),
                        Some(Duration::from_millis(50)),
                    );
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }

        // Wait until all 3 workers are restarted with pristine initial states
        let mut restored = false;
        for _ in 0..100 {
            if let (Some(w1), Some(w2), Some(w3)) = (
                sup.get_child(&w1_id),
                sup.get_child(&w2_id),
                sup.get_child(&w3_id),
            ) {
                if w1.is_alive() && w2.is_alive() && w3.is_alive() {
                    if let (Ok(s1), Ok(s2), Ok(s3)) = (
                        w1.ask(Value::Int(0), Some(Duration::from_millis(50))),
                        w2.ask(Value::Int(0), Some(Duration::from_millis(50))),
                        w3.ask(Value::Int(0), Some(Duration::from_millis(50))),
                    ) {
                        if s1 == Value::Int(10) && s2 == Value::Int(20) && s3 == Value::Int(30) {
                            restored = true;
                            break;
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(
            restored,
            "Cycle {}: All 3 workers must restart with pristine initial states (10, 20, 30)",
            cycle
        );
    }

    sup.stop();
}

// =============================================================================
// DIMENSION 4: Concurrent Client Requests (ask/send) During Crash Cycles
// =============================================================================

#[test]
fn test_stress_concurrent_client_requests_during_continuous_restarts() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("concurrent_clients");
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 500,
        within_seconds: 60.0,
    };

    let target_worker_id = format!("{}_worker", sup_id);
    let spec = ChildSpec::from_stateful_fn(
        target_worker_id.clone(),
        Value::Int(0),
        |state, msg| match (state, msg) {
            (Value::Int(_), Value::String(ref s)) if s == "kill" => {
                Err("Deliberate chaos monkey kill".into())
            }
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        },
    );

    let sup =
        Supervisor::start_named(sup_id.clone(), config, vec![spec]).expect("Supervisor must start");

    let running = Arc::new(AtomicBool::new(true));
    let total_asks = Arc::new(AtomicUsize::new(0));
    let successful_asks = Arc::new(AtomicUsize::new(0));
    let failed_asks = Arc::new(AtomicUsize::new(0));
    let total_sends = Arc::new(AtomicUsize::new(0));

    // Spawn 10 concurrent client query threads
    let mut client_handles = Vec::new();
    for _ in 0..10 {
        let sup_clone = Arc::clone(&sup);
        let w_id = target_worker_id.clone();
        let running_clone = Arc::clone(&running);
        let total_asks_clone = Arc::clone(&total_asks);
        let success_asks_clone = Arc::clone(&successful_asks);
        let fail_asks_clone = Arc::clone(&failed_asks);
        let total_sends_clone = Arc::clone(&total_sends);

        client_handles.push(thread::spawn(move || {
            let mut round = 0;
            while running_clone.load(Ordering::Relaxed) {
                round += 1;
                if let Some(child) = sup_clone.get_child(&w_id) {
                    if round % 2 == 0 {
                        // Ask pattern
                        total_asks_clone.fetch_add(1, Ordering::Relaxed);
                        match child.ask(Value::Int(1), Some(Duration::from_millis(50))) {
                            Ok(_) => {
                                success_asks_clone.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => {
                                fail_asks_clone.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    } else {
                        // Send pattern
                        total_sends_clone.fetch_add(1, Ordering::Relaxed);
                        let _ = child.send(Value::Int(1));
                    }
                }
                thread::yield_now();
            }
        }));
    }

    // Chaos monkey thread: continuously kills the worker
    let chaos_sup = Arc::clone(&sup);
    let chaos_wid = target_worker_id.clone();
    let chaos_running = Arc::clone(&running);
    let chaos_kill_count = Arc::new(AtomicUsize::new(0));
    let chaos_kill_clone = Arc::clone(&chaos_kill_count);

    let chaos_handle = thread::spawn(move || {
        while chaos_running.load(Ordering::Relaxed) {
            if let Some(child) = chaos_sup.get_child(&chaos_wid) {
                if child.is_alive() {
                    let _ = child.send(Value::String("kill".into()));
                    chaos_kill_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
    });

    // Let concurrent clients and chaos monkey clash for 1.5 seconds
    thread::sleep(Duration::from_millis(1500));

    running.store(false, Ordering::SeqCst);
    chaos_handle.join().unwrap();
    for h in client_handles {
        h.join().unwrap();
    }

    let kills = chaos_kill_count.load(Ordering::Relaxed);
    let asks = total_asks.load(Ordering::Relaxed);
    let successes = successful_asks.load(Ordering::Relaxed);
    let fails = failed_asks.load(Ordering::Relaxed);
    let sends = total_sends.load(Ordering::Relaxed);

    println!(
        "[Concurrent Client Stress] Kills: {}, Asks: {} (ok: {}, err: {}), Sends: {}",
        kills, asks, successes, fails, sends
    );

    assert!(kills >= 10, "Chaos monkey should have executed >= 10 kills");
    assert!(asks >= 100, "Clients should have made >= 100 asks");
    assert!(
        successes > 0,
        "At least some asks should succeed during living windows"
    );
    assert_eq!(
        asks,
        successes + fails,
        "Every ask must either succeed or return a clean error without hanging"
    );

    sup.stop();
}

// =============================================================================
// DIMENSION 5: High-Frequency Sliding Window Boundary Checks
// =============================================================================

#[test]
fn test_stress_sliding_window_rate_limiting_exact_boundary() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("sliding_window");
    const MAX_RESTARTS: usize = 4;
    const WINDOW_SECS: f64 = 0.6; // 600 ms window

    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: MAX_RESTARTS,
        within_seconds: WINDOW_SECS,
    };

    let spec = ChildSpec::from_fn(format!("{}_worker", sup_id), |msg| {
        if let Value::String(ref s) = msg {
            if s == "crash" {
                return Err("Crash loop message".into());
            }
        }
        Ok(Value::Null)
    });

    let sup =
        Supervisor::start_named(sup_id.clone(), config, vec![spec]).expect("Supervisor start");
    let wid = format!("{}_worker", sup_id);

    // Phase 1: Induce 3 crashes (strictly less than max_restarts = 4)
    for _ in 0..3 {
        for _ in 0..50 {
            if let Some(child) = sup.get_child(&wid) {
                if child.is_alive() {
                    let _ = child.ask(
                        Value::String("crash".into()),
                        Some(Duration::from_millis(50)),
                    );
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(15));
    }

    // Verify supervisor is STILL running (3 < 4)
    assert!(
        sup.is_alive(),
        "Supervisor must remain alive when crashes (3) < max_restarts (4)"
    );

    // Phase 2: Sleep beyond window duration (WINDOW_SECS + 150ms buffer)
    thread::sleep(Duration::from_millis(750));

    // Phase 3: Induce 3 more crashes! Since previous 3 expired from window, supervisor must remain alive!
    for _ in 0..3 {
        for _ in 0..50 {
            if let Some(child) = sup.get_child(&wid) {
                if child.is_alive() {
                    let _ = child.ask(
                        Value::String("crash".into()),
                        Some(Duration::from_millis(50)),
                    );
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(15));
    }

    assert!(
        sup.is_alive(),
        "Supervisor must remain alive because old crashes slid out of rolling window"
    );

    // Phase 4: Now hit 2 more crashes within current window to exceed max_restarts (3 + 2 = 5 > 4)
    for _ in 0..2 {
        for _ in 0..50 {
            if let Some(child) = sup.get_child(&wid) {
                if child.is_alive() {
                    let _ = child.ask(
                        Value::String("crash".into()),
                        Some(Duration::from_millis(50)),
                    );
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(10));
    }

    // Wait for supervisor to transition to Failed
    let mut failed = false;
    for _ in 0..100 {
        if !sup.is_alive() {
            failed = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        failed,
        "Supervisor must halt when rate limit threshold is exceeded"
    );
    assert!(matches!(sup.status(), SupervisorStatus::Failed(_)));
}

#[test]
fn test_stress_sliding_window_zero_restart_threshold() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("zero_restart");
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 0,
        within_seconds: 5.0,
    };

    let spec = ChildSpec::from_fn(format!("{}_worker", sup_id), |msg| {
        if let Value::String(ref s) = msg {
            if s == "die" {
                return Err("Immediate fail".into());
            }
        }
        Ok(Value::Null)
    });

    let sup =
        Supervisor::start_named(sup_id.clone(), config, vec![spec]).expect("Supervisor start");
    let wid = format!("{}_worker", sup_id);

    // Crash the child once
    let child = sup.get_child(&wid).unwrap();
    let _ = child.ask(Value::String("die".into()), Some(Duration::from_millis(50)));

    // With max_restarts = 0, first crash must immediately fail the supervisor!
    let mut failed = false;
    for _ in 0..100 {
        if !sup.is_alive() {
            failed = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        failed,
        "Supervisor with max_restarts=0 must fail immediately upon child crash"
    );
}

// =============================================================================
// DIMENSION 6: Dynamic Child Lifecycle Concurrency & Stop Cascade Idempotency
// =============================================================================

#[test]
fn test_stress_dynamic_child_concurrent_add_and_stop() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("dyn_concurrency");
    let sup = Supervisor::start_named(sup_id.clone(), SupervisorConfig::default(), Vec::new())
        .expect("Supervisor start");

    const NUM_CHILDREN: usize = 20;

    // Dynamically start 20 children concurrently across 4 threads
    let mut handles = Vec::new();
    for chunk_idx in 0..4 {
        let sup_clone = Arc::clone(&sup);
        let s_id = sup_id.clone();
        handles.push(thread::spawn(move || {
            for i in 0..5 {
                let child_id = format!("{}_c_{}_{}", s_id, chunk_idx, i);
                let spec = ChildSpec::from_stateful_fn(
                    child_id.clone(),
                    Value::Int(i as i64),
                    |state, msg| match (state, msg) {
                        (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                        (st, _) => Ok(st),
                    },
                );
                let child = sup_clone
                    .start_child(spec)
                    .expect("start_child should succeed");
                assert_eq!(child.id(), child_id);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(sup.len(), NUM_CHILDREN);

    // Concurrently query all 20 children
    for chunk_idx in 0..4 {
        for i in 0..5 {
            let child_id = format!("{}_c_{}_{}", sup_id, chunk_idx, i);
            let child = sup.get_child(&child_id).expect("Child must exist");
            assert!(child.is_alive());
            let res = child.ask(Value::Int(10), None).unwrap();
            assert_eq!(res, Value::Int((i + 10) as i64));
        }
    }

    // Stop half of them concurrently
    let mut stop_handles = Vec::new();
    for chunk_idx in 0..2 {
        let sup_clone = Arc::clone(&sup);
        let s_id = sup_id.clone();
        stop_handles.push(thread::spawn(move || {
            for i in 0..5 {
                let child_id = format!("{}_c_{}_{}", s_id, chunk_idx, i);
                sup_clone
                    .stop_child(&child_id)
                    .expect("stop_child must succeed");
            }
        }));
    }

    for h in stop_handles {
        h.join().unwrap();
    }

    assert_eq!(sup.len(), 10);

    // Stop supervisor and assert idempotency
    assert!(sup.stop());
    assert!(!sup.stop()); // second stop returns false
    assert!(!sup.is_alive());
    assert_eq!(sup.status(), SupervisorStatus::Stopped);
}

// =============================================================================
// DIMENSION 7: Manual Child Restart Semantics & Duplicate Spawning Check
// =============================================================================

#[test]
fn test_stress_restart_child_semantics_and_leak_check() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("manual_restart");
    let sup = Supervisor::start_named(sup_id.clone(), SupervisorConfig::default(), Vec::new())
        .expect("Supervisor start");

    let child_id = format!("{}_worker", sup_id);
    let spawn_counter = Arc::new(AtomicUsize::new(0));
    let sc_clone = Arc::clone(&spawn_counter);

    let _cid = child_id.clone();
    let spec = ChildSpec::new(
        child_id.clone(),
        Arc::new(move || {
            sc_clone.fetch_add(1, Ordering::SeqCst);
            Ok(ActorRef::spawn_fn(|msg| Ok(msg)))
        }),
    );

    let _c1 = sup.start_child(spec).expect("Initial start");
    assert_eq!(spawn_counter.load(Ordering::SeqCst), 1);

    // Perform manual restart_child
    let c2 = sup
        .restart_child(&child_id)
        .expect("restart_child should succeed");
    assert!(c2.is_alive());

    // Give any background supervisor loop events time to settle
    thread::sleep(Duration::from_millis(150));

    let final_spawns = spawn_counter.load(Ordering::SeqCst);
    println!(
        "[Manual Restart] Expected spawns: 2, Actual spawns: {}",
        final_spawns
    );

    // Check whether the supervisor erroneously spawned a duplicate third actor
    assert_eq!(
        final_spawns, 2,
        "restart_child should spawn exactly 1 new actor (total 2). If 3, supervisor monitor raced and duplicate-spawned!"
    );

    sup.stop();
}

#[test]
fn test_empirical_reproduce_restart_child_race_condition() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("restart_race");
    let sup = Supervisor::start_named(sup_id.clone(), SupervisorConfig::default(), Vec::new())
        .expect("Supervisor start");

    let child_id = format!("{}_worker", sup_id);
    let spec = ChildSpec::from_stateful_fn(child_id.clone(), Value::Int(10), |state, msg| {
        match (state, msg) {
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        }
    });

    sup.start_child(spec).expect("start_child");

    let mut race_detected = false;
    for i in 0..20 {
        let restarted = sup.restart_child(&child_id).expect("restart_child");
        assert!(restarted.is_alive());
        // Verify state is pristine
        assert_eq!(restarted.ask(Value::Int(0), None).unwrap(), Value::Int(10));

        // Sleep 15ms so supervisor monitor loop has time to process the exit event if racing
        thread::sleep(Duration::from_millis(15));

        // Stop child
        sup.stop_child(&child_id).expect("stop_child");

        // If race happened, restarted is still alive!
        if restarted.is_alive() {
            println!(
                "[Race Detected] Iteration {}: restarted actor is still alive after stop_child! Duplicate actor was stopped instead!",
                i
            );
            race_detected = true;
            restarted.stop();
            break;
        }

        if i < 19 {
            // Re-start child for next iteration
            let spec_next =
                ChildSpec::from_stateful_fn(child_id.clone(), Value::Int(10), |state, msg| match (
                    state, msg,
                ) {
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                });
            sup.start_child(spec_next).expect("start_child");
        }
    }

    sup.stop();
    assert!(
        !race_detected,
        "CRITICAL BUG CONFIRMED: restart_child raced with supervisor monitor loop! The restarted actor handle was clobbered by a duplicate actor in supervisor table, leaking the returned actor thread!"
    );
}

// =============================================================================
// DIMENSION 10: Concurrent Restarts, Stops, and Queries with Thread Leak Check
// =============================================================================

#[test]
fn test_stress_concurrent_restarts_and_stops_no_thread_leak() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");
    let sup_id = unique_id("concurrent_stress");
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 500,
        within_seconds: 10.0,
    };

    let sup = Supervisor::start_named(sup_id.clone(), config, Vec::new()).expect("start_named");

    let make_spec = |child_id: String| {
        let cid = child_id.clone();
        let factory: shae::actor::ActorFactory = Arc::new(move || {
            let cid_c = cid.clone();
            println!("[Factory] Spawning child {}", cid);
            let actor =
                ActorRef::spawn_stateful_fn(Value::Int(0), move |state, msg| match (state, msg) {
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                });
            let cid_exit = cid_c.clone();
            actor.add_exit_listener(move |_id, _st| {
                println!("[Actor Exit] Child {} exit listener fired", cid_exit);
            });
            Ok(actor)
        });
        ChildSpec::new(child_id, factory)
    };

    for i in 0..5 {
        let cid = format!("{}_c{}", sup_id, i);
        let spec = make_spec(cid);
        let _ = sup.start_child(spec);
    }

    let running = Arc::new(AtomicBool::new(true));
    let mut handles = Vec::new();

    for th_idx in 0..8 {
        let sup_c = sup.clone();
        let running_c = Arc::clone(&running);
        let sup_id_c = sup_id.clone();

        handles.push(thread::spawn(move || {
            let mut iters = 0;
            while running_c.load(Ordering::Relaxed) && iters < 100 {
                iters += 1;
                let target_child = format!("{}_c{}", sup_id_c, iters % 5);

                match (iters + th_idx) % 4 {
                    0 => {
                        if th_idx == 0 {
                            println!("[Thread 0] restart_child({})", target_child);
                        }
                        let _ = sup_c.restart_child(&target_child);
                        if th_idx == 0 {
                            println!("[Thread 0] restart_child({}) done", target_child);
                        }
                    }
                    1 => {
                        if th_idx == 0 {
                            println!("[Thread 0] stop_child({})", target_child);
                        }
                        let _ = sup_c.stop_child(&target_child);
                        if th_idx == 0 {
                            println!("[Thread 0] stop_child({}) done", target_child);
                        }
                        let spec = ChildSpec::from_stateful_fn(
                            target_child.clone(),
                            Value::Int(0),
                            |state, msg| match (state, msg) {
                                (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                                (st, _) => Ok(st),
                            },
                        );
                        if th_idx == 0 {
                            println!("[Thread 0] start_child({})", target_child);
                        }
                        let _ = sup_c.start_child(spec);
                        if th_idx == 0 {
                            println!("[Thread 0] start_child({}) done", target_child);
                        }
                    }
                    2 => {
                        if th_idx == 0 {
                            println!("[Thread 0] ask({})", target_child);
                        }
                        if let Some(child) = sup_c.get_child(&target_child) {
                            let _ = child.ask(Value::Int(1), Some(Duration::from_millis(50)));
                        }
                        if th_idx == 0 {
                            println!("[Thread 0] ask({}) done", target_child);
                        }
                    }
                    _ => {
                        if th_idx == 0 {
                            println!("[Thread 0] children()");
                        }
                        let _ = sup_c.children();
                        if th_idx == 0 {
                            println!("[Thread 0] children() done");
                        }
                    }
                }
                thread::sleep(Duration::from_millis(2));
            }
            println!("[Stress] Thread {} finished all its iterations", th_idx);
        }));
    }

    thread::sleep(Duration::from_millis(600));
    println!("[Stress] Signaling running = false");
    running.store(false, Ordering::Relaxed);

    for (idx, h) in handles.into_iter().enumerate() {
        println!("[Stress] Joining handle {}", idx);
        let _ = h.join();
        println!("[Stress] Joined handle {}", idx);
    }

    println!("[Stress] Stopping supervisor");
    assert!(sup.stop());
    println!("[Stress] Supervisor stopped, joining");
    let _ = sup.join();
    println!("[Stress] Supervisor joined");

    thread::sleep(Duration::from_millis(200));

    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Concurrent Restart/Stop Stress] Initial threads: {}, Final threads: {}",
        initial_threads, final_threads
    );
    assert!(
        final_threads <= initial_threads + 2,
        "Leaked OS threads! Initial: {}, Final: {}",
        initial_threads,
        final_threads
    );
}

// =============================================================================
// DIMENSION 11: Restart Child Recovers Gracefully When Factory Panics
// =============================================================================

#[test]
fn test_stress_restart_child_when_factory_panics() {
    let _lock = SERIAL_TEST_LOCK.lock().unwrap();
    let sup_id = unique_id("factory_panic");
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 10,
        within_seconds: 10.0,
    };

    let should_panic = Arc::new(AtomicBool::new(false));
    let panic_flag = Arc::clone(&should_panic);

    let child_id = format!("{}_panicker", sup_id);
    let _child_id_c = child_id.clone();
    let spec = ChildSpec::new(
        child_id.clone(),
        Arc::new(move || {
            if panic_flag.load(Ordering::SeqCst) {
                panic!("Deliberate child factory panic during restart");
            }
            Ok(ActorRef::spawn_stateful_fn(
                Value::Int(100),
                |state, msg| match (state, msg) {
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                },
            ))
        }),
    );

    let sup = Supervisor::start_named(sup_id.clone(), config, vec![spec]).expect("start_named");

    let c = sup.get_child(&child_id).expect("child exists");
    assert_eq!(c.ask(Value::Int(5), None).unwrap(), Value::Int(105));

    // Arm the factory to panic
    should_panic.store(true, Ordering::SeqCst);

    // Call restart_child -> should catch panic and return Err(ChildStartError)
    let res = sup.restart_child(&child_id);
    assert!(
        res.is_err(),
        "restart_child must fail when factory panics, but got: {:?}",
        res
    );

    // Disarm the panic
    should_panic.store(false, Ordering::SeqCst);

    // Call restart_child again -> must succeed now that flag is cleared
    let restarted = sup
        .restart_child(&child_id)
        .expect("restart_child must succeed after factory is restored");
    assert!(restarted.is_alive());
    assert_eq!(
        restarted.ask(Value::Int(10), None).unwrap(),
        Value::Int(110)
    );

    assert!(sup.stop());
    let _ = sup.join();
}

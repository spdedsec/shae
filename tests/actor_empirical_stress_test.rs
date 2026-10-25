//! Empirical Stress and Adversarial Challenge Harness for Shae Phase 4 Milestone 1
//! Tests error containment, ask timeouts, and registry concurrency.

use shae::actor::{ActorError, ActorRef, ActorRegistry, ActorStatus};
use shae::value::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

static REGISTRY_MUTEX: Mutex<()> = Mutex::new(());

// Helper to count running OS threads from /proc/self/stat or /proc/self/status on Linux
fn get_current_thread_count() -> Option<usize> {
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
    None
}

// =========================================================================
// DIMENSION 1: Error Containment & Panic Isolation
// =========================================================================

#[test]
fn test_empirical_stateless_err_containment() {
    let actor = ActorRef::spawn_fn(|msg| {
        if let Value::String(ref s) = msg {
            if s == "fail" {
                return Err("Simulated stateless handler error".into());
            }
        }
        Ok(Value::String("ok".into()))
    });

    assert!(actor.is_alive());
    assert_eq!(actor.status(), ActorStatus::Running);

    // Send the failure message
    actor.send(Value::String("fail".into())).unwrap();

    // Give the actor thread a brief moment to process and terminate
    let start = Instant::now();
    while actor.is_alive() && start.elapsed() < Duration::from_millis(500) {
        thread::sleep(Duration::from_millis(5));
    }

    assert!(
        !actor.is_alive(),
        "Actor must not be alive after handler Err"
    );
    match actor.status() {
        ActorStatus::Failed(ref err) => {
            assert!(
                err.contains("Simulated stateless handler error"),
                "Expected failure message, got: {}",
                err
            );
        }
        other => panic!("Expected ActorStatus::Failed, got: {:?}", other),
    }

    // Subsequent send must fail gracefully
    let send_res = actor.send(Value::Int(123));
    assert!(send_res.is_err());
    match send_res.unwrap_err() {
        ActorError::ActorFailed(err) => {
            assert!(err.contains("Simulated stateless handler error"));
        }
        other => panic!("Expected ActorError::ActorFailed, got: {:?}", other),
    }

    // Subsequent ask must fail gracefully and immediately without waiting for timeout
    let ask_start = Instant::now();
    let ask_res = actor.ask(Value::Int(456), Some(Duration::from_secs(5)));
    assert!(
        ask_start.elapsed() < Duration::from_millis(200),
        "Ask on failed actor must return immediately"
    );
    assert!(ask_res.is_err());
    match ask_res.unwrap_err() {
        ActorError::ActorFailed(err) => {
            assert!(err.contains("Simulated stateless handler error"));
        }
        other => panic!("Expected ActorError::ActorFailed, got: {:?}", other),
    }
}

#[test]
fn test_empirical_panic_payload_isolation() {
    // 1. Literal string slice panic
    let actor1 = ActorRef::spawn_fn(|_| panic!("str_panic_literal"));
    actor1.send(Value::Null).unwrap();
    thread::sleep(Duration::from_millis(50));
    assert!(!actor1.is_alive());
    assert_eq!(
        actor1.status(),
        ActorStatus::Failed("str_panic_literal".into())
    );

    // 2. Formatted String panic
    let actor2 = ActorRef::spawn_fn(|_| panic!("formatted_panic_{}", 12345));
    actor2.send(Value::Null).unwrap();
    thread::sleep(Duration::from_millis(50));
    assert!(!actor2.is_alive());
    assert_eq!(
        actor2.status(),
        ActorStatus::Failed("formatted_panic_12345".into())
    );

    // 3. Non-string panic (panic_any)
    let actor3 = ActorRef::spawn_fn(|_| std::panic::panic_any(98765));
    actor3.send(Value::Null).unwrap();
    thread::sleep(Duration::from_millis(50));
    assert!(!actor3.is_alive());
    assert_eq!(
        actor3.status(),
        ActorStatus::Failed("Actor panicked with unformatted payload".into())
    );
}

#[test]
fn test_empirical_stateful_panic_containment() {
    let actor = ActorRef::spawn_stateful_fn(Value::Int(100), |state, msg| match (state, msg) {
        (Value::Int(s), Value::String(cmd)) => {
            if cmd == "boom" {
                panic!("Stateful calculation panic at state={}", s);
            }
            Ok(Value::Int(s + 1))
        }
        _ => Err("Invalid args".into()),
    });

    actor.send(Value::String("inc".into())).unwrap();
    thread::sleep(Duration::from_millis(30));
    assert!(actor.is_alive());

    actor.send(Value::String("boom".into())).unwrap();
    thread::sleep(Duration::from_millis(50));
    assert!(!actor.is_alive());
    match actor.status() {
        ActorStatus::Failed(err) => {
            assert!(err.contains("Stateful calculation panic at state=101"));
        }
        other => panic!("Expected Failed status, got: {:?}", other),
    }
}

#[test]
fn test_empirical_ask_error_race_condition() {
    // Stress-test the race condition: Does `actor.ask()` returning an error guarantee
    // that `actor.is_alive()` is already `false` and `actor.status()` is already `Failed`?
    // In src/actor.rs, reply_tx.send is currently called BEFORE status.write().
    let mut race_detected_count = 0;
    let iterations = 100;

    for i in 0..iterations {
        let actor = ActorRef::spawn_fn(|msg| {
            if let Value::String(s) = msg {
                if s == "fail" {
                    return Err("instant_err".into());
                }
            }
            Ok(Value::Null)
        });

        let res = actor.ask(Value::String("fail".into()), Some(Duration::from_secs(2)));
        assert!(res.is_err(), "Iteration {} ask must fail", i);

        // Immediately inspect status without sleeping
        let is_alive = actor.is_alive();
        let status = actor.status();

        if is_alive || status == ActorStatus::Running {
            race_detected_count += 1;
        }

        actor.stop();
    }

    println!(
        "===> EMPIRICAL RESULT: Ask Error Race Condition detected in {} / {} iterations",
        race_detected_count, iterations
    );
    assert_eq!(
        race_detected_count, 0,
        "Ask Error Race Condition detected in {} / {} iterations",
        race_detected_count, iterations
    );
}

// =========================================================================
// DIMENSION 2: Ask Pattern Stress, Timeouts & Resource Management
// =========================================================================

#[test]
fn test_empirical_ask_timeout_bounds() {
    // Non-responsive actor sleeping 2 seconds
    let actor = ActorRef::spawn_fn(|_| {
        thread::sleep(Duration::from_millis(2000));
        Ok(Value::Null)
    });

    // 1. Timeout 50ms: verify caller unblocks in [45ms, 200ms]
    let start = Instant::now();
    let res = actor.ask(Value::Null, Some(Duration::from_millis(50)));
    let elapsed = start.elapsed();
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), ActorError::Timeout);
    assert!(
        elapsed >= Duration::from_millis(45),
        "Elapsed {:?} was shorter than requested timeout",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(250),
        "Elapsed {:?} took too long (bounded timeout violated)",
        elapsed
    );

    // 2. Timeout 10ms: verify caller unblocks rapidly
    let start = Instant::now();
    let res = actor.ask(Value::Null, Some(Duration::from_millis(10)));
    let elapsed = start.elapsed();
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), ActorError::Timeout);
    assert!(
        elapsed < Duration::from_millis(100),
        "10ms timeout took {:?}, exceeds bound",
        elapsed
    );

    // 3. Timeout 0ms: verify immediate return
    let start = Instant::now();
    let res = actor.ask(Value::Null, Some(Duration::from_millis(0)));
    let elapsed = start.elapsed();
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), ActorError::Timeout);
    assert!(
        elapsed < Duration::from_millis(50),
        "0ms timeout took {:?}, exceeds bound",
        elapsed
    );

    actor.stop();
}

#[test]
fn test_empirical_multi_threaded_ask_timeout_storm() {
    // Non-responsive actor
    let actor = ActorRef::spawn_fn(|_| {
        thread::sleep(Duration::from_millis(3000));
        Ok(Value::Null)
    });

    let thread_count = 40;
    let barrier = Arc::new(Barrier::new(thread_count));
    let timeout_count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    let wall_clock_start = Instant::now();

    for _ in 0..thread_count {
        let a = Arc::clone(&actor);
        let b = Arc::clone(&barrier);
        let tc = Arc::clone(&timeout_count);

        handles.push(thread::spawn(move || {
            b.wait();
            let start = Instant::now();
            let res = a.ask(Value::Int(1), Some(Duration::from_millis(60)));
            let elapsed = start.elapsed();

            if let Err(ActorError::Timeout) = res {
                if elapsed >= Duration::from_millis(50) && elapsed < Duration::from_millis(400) {
                    tc.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let total_wall_clock = wall_clock_start.elapsed();
    let timeouts = timeout_count.load(Ordering::Relaxed);

    println!(
        "===> EMPIRICAL RESULT: Multi-threaded ask storm: {} / {} threads timed out correctly within bounds in {:?}",
        timeouts, thread_count, total_wall_clock
    );

    assert_eq!(
        timeouts, thread_count,
        "All 40 threads must timeout within bounds"
    );
    assert!(
        total_wall_clock < Duration::from_millis(800),
        "Wall clock elapsed took too long: {:?}",
        total_wall_clock
    );

    actor.stop();
}

#[test]
fn test_empirical_high_throughput_ask_reply() {
    // Fast stateful counter actor
    let actor = ActorRef::spawn_stateful_fn(Value::Int(0), |state, msg| match (state, msg) {
        (Value::Int(count), Value::Int(increment)) => {
            let new_count = count + increment;
            let mut map = indexmap::IndexMap::new();
            map.insert("state".to_string(), Value::Int(new_count));
            map.insert("reply".to_string(), Value::Int(new_count));
            Ok(Value::Map(Arc::new(std::sync::RwLock::new(map))))
        }
        _ => Err("Invalid args".into()),
    });

    let threads = 8;
    let asks_per_thread = 100;
    let barrier = Arc::new(Barrier::new(threads));
    let success_count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    let start = Instant::now();

    for _t in 0..threads {
        let a = Arc::clone(&actor);
        let b = Arc::clone(&barrier);
        let sc = Arc::clone(&success_count);

        handles.push(thread::spawn(move || {
            b.wait();
            for _ in 0..asks_per_thread {
                let res = a.ask(Value::Int(1), Some(Duration::from_millis(2000)));
                if let Ok(Value::Int(_)) = res {
                    sc.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total_success = success_count.load(Ordering::Relaxed);
    let expected = threads * asks_per_thread;

    println!(
        "===> EMPIRICAL RESULT: Ask throughput: {}/{} requests succeeded in {:?} ({:.1} req/sec)",
        total_success,
        expected,
        elapsed,
        (expected as f64) / elapsed.as_secs_f64()
    );

    assert_eq!(total_success, expected);

    // Final value must equal 800
    let final_val = actor
        .ask(Value::Int(0), Some(Duration::from_millis(1000)))
        .unwrap();
    assert_eq!(final_val, Value::Int(800));

    actor.stop();
}

#[test]
fn test_empirical_thread_leak_on_spawn_named_failure() {
    let _lock = REGISTRY_MUTEX.lock().unwrap();
    ActorRegistry::clear();

    // Register initial actor
    let initial_actor = ActorRef::spawn_fn(|_| Ok(Value::Null));
    ActorRegistry::register("contested_name", Arc::clone(&initial_actor)).unwrap();

    let initial_threads = get_current_thread_count();

    fn dummy_builtin(
        _: &mut shae::eval::Evaluator,
        _: Vec<Value>,
        _: shae::ast::Span,
    ) -> Result<Value, shae::eval::RuntimeError> {
        Ok(Value::Null)
    }
    let dummy_fn = Value::Builtin {
        name: "dummy".to_string(),
        func: dummy_builtin,
    };
    let mut fail_count = 0;

    for _ in 0..30 {
        let res = ActorRef::spawn_named("contested_name", None, dummy_fn.clone());
        if let Err(ActorError::NameAlreadyRegistered(_)) = res {
            fail_count += 1;
        }
    }

    assert_eq!(fail_count, 30);

    let post_threads = get_current_thread_count();
    if let (Some(before), Some(after)) = (initial_threads, post_threads) {
        println!(
            "===> EMPIRICAL RESULT: Threads before: {}, Threads after 30 failed spawn_named calls: {} (Delta: +{})",
            before,
            after,
            after.saturating_sub(before)
        );
        assert_eq!(
            after.saturating_sub(before),
            0,
            "Thread leak detected: before={}, after={}",
            before,
            after
        );
    }

    initial_actor.stop();
    ActorRegistry::clear();
}

// =========================================================================
// DIMENSION 3: Registry Concurrency & Multi-Thread Hammer
// =========================================================================

#[test]
fn test_empirical_registry_duplicate_registration_race() {
    let _lock = REGISTRY_MUTEX.lock().unwrap();
    ActorRegistry::clear();

    let thread_count = 24;
    let barrier = Arc::new(Barrier::new(thread_count));
    let success_count = Arc::new(AtomicUsize::new(0));
    let duplicate_error_count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    let actors: Vec<Arc<ActorRef>> = (0..thread_count)
        .map(|_| ActorRef::spawn_fn(|_| Ok(Value::Null)))
        .collect();

    for i in 0..thread_count {
        let b = Arc::clone(&barrier);
        let actor = Arc::clone(&actors[i]);
        let sc = Arc::clone(&success_count);
        let dec = Arc::clone(&duplicate_error_count);

        handles.push(thread::spawn(move || {
            b.wait();
            let res = ActorRegistry::register("singleton_service", actor);
            match res {
                Ok(()) => {
                    sc.fetch_add(1, Ordering::SeqCst);
                }
                Err(ActorError::NameAlreadyRegistered(name)) => {
                    if name == "singleton_service" {
                        dec.fetch_add(1, Ordering::SeqCst);
                    }
                }
                Err(other) => panic!("Unexpected error: {:?}", other),
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let successes = success_count.load(Ordering::SeqCst);
    let duplicates = duplicate_error_count.load(Ordering::SeqCst);

    println!(
        "===> EMPIRICAL RESULT: Registry contention: {} won, {} got NameAlreadyRegistered (out of {})",
        successes, duplicates, thread_count
    );

    assert_eq!(
        successes, 1,
        "EXACTLY ONE thread must succeed in registration"
    );
    assert_eq!(
        duplicates,
        thread_count - 1,
        "All other threads must receive NameAlreadyRegistered"
    );

    let winner = ActorRegistry::whereis("singleton_service");
    assert!(winner.is_some());

    // Cleanup
    for a in actors {
        a.stop();
    }
    ActorRegistry::clear();
}

#[test]
fn test_empirical_registry_heavy_concurrent_hammer() {
    let _lock = REGISTRY_MUTEX.lock().unwrap();
    ActorRegistry::clear();

    let num_keys = 8;
    let threads = 16;
    let ops_per_thread = 250;
    let barrier = Arc::new(Barrier::new(threads));
    let total_ops = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    // Pre-create some actors
    let actors: Arc<Vec<Arc<ActorRef>>> = Arc::new(
        (0..num_keys)
            .map(|_| ActorRef::spawn_fn(|_| Ok(Value::Null)))
            .collect(),
    );

    let start = Instant::now();

    for t in 0..threads {
        let b = Arc::clone(&barrier);
        let act_list = Arc::clone(&actors);
        let op_counter = Arc::clone(&total_ops);

        handles.push(thread::spawn(move || {
            b.wait();
            for i in 0..ops_per_thread {
                let key = format!("service_{}", (t + i) % num_keys);
                match (t + i) % 4 {
                    0 => {
                        // Register
                        let actor = Arc::clone(&act_list[(t + i) % num_keys]);
                        let _ = ActorRegistry::register(&key, actor);
                    }
                    1 => {
                        // Whereis
                        let _ = ActorRegistry::whereis(&key);
                    }
                    2 => {
                        // Unregister
                        let _ = ActorRegistry::unregister(&key);
                    }
                    3 => {
                        // List registered
                        let list = ActorRegistry::registered();
                        // Verify sorting invariant
                        for w in list.windows(2) {
                            assert!(w[0] <= w[1], "Registered list must be sorted!");
                        }
                    }
                    _ => unreachable!(),
                }
                op_counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let elapsed = start.elapsed();
    let performed = total_ops.load(Ordering::Relaxed);

    println!(
        "===> EMPIRICAL RESULT: Registry Hammer: {} operations across {} threads completed in {:?} ({:.0} ops/sec)",
        performed,
        threads,
        elapsed,
        (performed as f64) / elapsed.as_secs_f64()
    );

    assert_eq!(performed, threads * ops_per_thread);

    // Cleanup
    for a in actors.iter() {
        a.stop();
    }
    ActorRegistry::clear();
}

#[test]
fn test_empirical_dead_actor_lifecycle_in_registry() {
    let _lock = REGISTRY_MUTEX.lock().unwrap();
    ActorRegistry::clear();

    let actor1 = ActorRef::spawn_fn(|_| Ok(Value::Null));
    ActorRegistry::register("worker_service", Arc::clone(&actor1)).unwrap();

    assert_eq!(
        ActorRegistry::registered(),
        vec!["worker_service".to_string()]
    );
    assert_eq!(
        ActorRegistry::whereis("worker_service").unwrap().id(),
        actor1.id()
    );

    // Stop actor1
    actor1.stop_and_join().unwrap();
    assert!(!actor1.is_alive());

    // Whereis should now return None even though name was not explicitly unregistered
    assert!(
        ActorRegistry::whereis("worker_service").is_none(),
        "Whereis on stopped actor must return None"
    );
    assert!(
        !ActorRegistry::registered().contains(&"worker_service".to_string()),
        "Registered list must not include dead actors"
    );

    // Now re-register with a new alive actor under the same name: should succeed!
    let actor2 = ActorRef::spawn_fn(|_| Ok(Value::Null));
    let reg_res = ActorRegistry::register("worker_service", Arc::clone(&actor2));
    assert!(
        reg_res.is_ok(),
        "Registering over dead actor must succeed, got: {:?}",
        reg_res
    );

    assert_eq!(
        ActorRegistry::whereis("worker_service").unwrap().id(),
        actor2.id()
    );

    actor2.stop();
    ActorRegistry::clear();
}

#[test]
fn test_empirical_stop_discards_pending_asks_causing_timeout() {
    // Actor that takes 20ms per message
    let actor = ActorRef::spawn_fn(|msg| {
        thread::sleep(Duration::from_millis(20));
        Ok(msg)
    });

    // Enqueue 3 normal messages
    actor.send(Value::Int(1)).unwrap();
    actor.send(Value::Int(2)).unwrap();
    actor.send(Value::Int(3)).unwrap();

    // Now spawn a thread to do an ask with 200ms timeout
    let a_clone = Arc::clone(&actor);
    let ask_handle = thread::spawn(move || {
        let start = Instant::now();
        let res = a_clone.ask(
            Value::String("query".into()),
            Some(Duration::from_millis(200)),
        );
        (res, start.elapsed())
    });

    // Give it 5ms to enqueue
    thread::sleep(Duration::from_millis(5));

    // Call stop()
    actor.stop();

    let (res, elapsed) = ask_handle.join().unwrap();
    println!(
        "===> EMPIRICAL RESULT: Ask pending behind stop returned {:?} after {:?}",
        res, elapsed
    );
    // Graceful stop processes messages enqueued prior to stop, so ask succeeds promptly!
    assert!(
        res.is_ok(),
        "Expected ask enqueued before stop to be processed, got: {:?}",
        res
    );
    assert_eq!(res.unwrap(), Value::String("query".into()));
    assert!(
        elapsed < Duration::from_millis(150),
        "Expected caller to unblock promptly without waiting for timeout, elapsed was {:?}",
        elapsed
    );
}

#[test]
fn test_empirical_shae_evaluator_error_containment() {
    let mut eval = shae::eval::Evaluator::new();
    // Define a Shae function that crashes (e.g. division by zero or panic)
    let source = "fn crashing_worker(msg) { return 10 / 0; }";
    let prog = shae::parser::parse(shae::lexer::tokenize(source).unwrap()).unwrap();
    eval.eval_program(&prog).unwrap();

    let crashing_fn = eval
        .global_env
        .read()
        .unwrap()
        .get("crashing_worker")
        .unwrap();

    let actor = ActorRef::spawn_with_eval(&eval, None, None, crashing_fn).unwrap();
    assert!(actor.is_alive());

    // Send a message to invoke crashing_worker
    actor.send(Value::Int(42)).unwrap();

    // Wait for crash
    let start = Instant::now();
    while actor.is_alive() && start.elapsed() < Duration::from_millis(500) {
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        !actor.is_alive(),
        "Actor running crashing Shae function must fail"
    );
    match actor.status() {
        ActorStatus::Failed(err) => {
            assert!(
                err.contains("Division by zero") || err.contains("zero"),
                "Expected division by zero error, got: {}",
                err
            );
        }
        other => panic!("Expected ActorStatus::Failed, got: {:?}", other),
    }

    // Host runtime is completely intact and operational!
    let eval_res = eval.eval_program(&prog);
    assert!(
        eval_res.is_ok(),
        "Host evaluator must remain fully functional"
    );
}

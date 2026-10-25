//! Empirical Adversarial Edge-Case and Boundary Verification for Milestone 2 Supervision Trees.
//!
//! Verification Scenarios:
//! 1. Immediate crashes on child spawn (actor panics/errors on first message).
//! 2. Poisoned restart loops: actor crashes repeatedly until rate limit triggers,
//!    verifying `SupervisorStatus::Failed` and clean cascade shutdown of all siblings.
//! 3. Idempotent and concurrent `stop()` calls from multiple threads simultaneously.
//! 4. Dynamic operations (`start_child`, `stop_child`, `restart_child`, `get_child`) under concurrent crash events.
//! 5. Verification that stopped children are properly cleaned up and cannot be sent messages without error.
//! 6. Sliding-window rate limiter expiration and recovery.
//! 7. Restart policies: Temporary vs Transient vs Permanent semantics under failure and clean exit.

use shae::actor::{
    ActorError, ActorRegistry, ActorStatus, ChildSpec, RestartPolicy, RestartStrategy, Supervisor,
    SupervisorConfig, SupervisorError, SupervisorStatus,
};
use shae::value::Value;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

// =========================================================================
// PART 1: Immediate crashes on child spawn
// =========================================================================

#[test]
fn test_immediate_panic_on_first_message_one_for_one() {
    let spawn_counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&spawn_counter);

    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 5,
        within_seconds: 5.0,
    };

    // Child panics on very first message, but works normally on subsequent spawns
    let spec = ChildSpec::from_fn("immediate_panicker", move |msg| {
        let count = counter_clone.fetch_add(1, Ordering::SeqCst);
        if count == 0 {
            panic!("Immediate fatal panic on first message!");
        }
        // Subsequent spawns respond cleanly
        Ok(msg)
    });

    let sup = Supervisor::start(config, vec![spec]).expect("Supervisor should start");
    let child = sup
        .get_child("immediate_panicker")
        .expect("Child should exist");

    // Send first message that triggers panic
    let _ = child.send(Value::String("trigger_panic".into()));

    // Poll until supervisor restarts the child with fresh state
    let mut restarted_child = None;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(c) = sup.get_child("immediate_panicker") {
            if !Arc::ptr_eq(&c, &child) && c.is_alive() {
                restarted_child = Some(c);
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    let restarted = restarted_child.expect("Supervisor must restart panicked child");
    assert!(restarted.is_alive());

    // The restarted child should now successfully handle messages
    let reply = restarted
        .ask(
            Value::String("hello_after_restart".into()),
            Some(Duration::from_millis(500)),
        )
        .expect("Restarted child should process message");
    assert_eq!(reply, Value::String("hello_after_restart".into()));

    sup.stop();
}

#[test]
fn test_immediate_error_on_first_message_all_for_one() {
    let w1_spawn_count = Arc::new(AtomicUsize::new(0));
    let w1_clone = Arc::clone(&w1_spawn_count);

    let config = SupervisorConfig {
        strategy: RestartStrategy::AllForOne,
        max_restarts: 5,
        within_seconds: 5.0,
    };

    let spec1 = ChildSpec::from_fn("bnd_all4one_w1", move |msg| {
        let count = w1_clone.fetch_add(1, Ordering::SeqCst);
        if count == 0 {
            return Err("Deliberate error on first invocation".into());
        }
        Ok(msg)
    });

    let spec2 =
        ChildSpec::from_stateful_fn("bnd_all4one_w2", Value::Int(100), |state, msg| {
            match (state, msg) {
                (Value::Int(s), Value::Int(delta)) => Ok(Value::Int(s + delta)),
                (st, _) => Ok(st),
            }
        });

    let sup = Supervisor::start(config, vec![spec1, spec2]).expect("Supervisor should start");

    let w1 = sup.get_child("bnd_all4one_w1").unwrap();
    let w2 = sup.get_child("bnd_all4one_w2").unwrap();

    // Mutate worker 2 state prior to worker 1 crash
    let res = w2
        .ask(Value::Int(50), Some(Duration::from_millis(500)))
        .unwrap();
    assert_eq!(res, Value::Int(150));

    // Trigger immediate error in worker 1
    let _ = w1.ask(
        Value::String("fail".into()),
        Some(Duration::from_millis(200)),
    );

    // Poll until AllForOne has restarted both workers
    let mut both_restarted = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let (Some(new_w1), Some(new_w2)) = (
            sup.get_child("bnd_all4one_w1"),
            sup.get_child("bnd_all4one_w2"),
        ) {
            if !Arc::ptr_eq(&new_w1, &w1)
                && !Arc::ptr_eq(&new_w2, &w2)
                && new_w1.is_alive()
                && new_w2.is_alive()
            {
                // Verify worker 2 state was reset to pristine 100
                if let Ok(w2_st) = new_w2.ask(Value::Int(0), Some(Duration::from_millis(100))) {
                    if w2_st == Value::Int(100) {
                        both_restarted = true;
                        break;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(15));
    }

    assert!(
        both_restarted,
        "AllForOne must restart all siblings with pristine initial state"
    );

    sup.stop();
}

// =========================================================================
// PART 2: Poisoned restart loops and clean cascade
// =========================================================================

#[test]
fn test_poisoned_restart_loop_triggers_supervisor_failed_and_cascades() {
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 3,
        within_seconds: 2.0,
    };

    // Poisoned child crashes unconditionally on every message
    let poison_spec = ChildSpec::from_fn("poisoned_actor", |_| {
        Err("Poisoned actor cannot process any message".into())
    });

    let innocent_spec = ChildSpec::from_fn("innocent_sibling", |msg| Ok(msg));

    let sup = Supervisor::start(config, vec![poison_spec, innocent_spec])
        .expect("Supervisor should start");

    let innocent = sup.get_child("innocent_sibling").unwrap();
    assert!(innocent.is_alive());

    // Induce rapid crashes on poisoned actor until rate limit is exceeded
    let deadline = Instant::now() + Duration::from_secs(4);
    while sup.is_alive() && Instant::now() < deadline {
        if let Some(child) = sup.get_child("poisoned_actor") {
            if child.is_alive() {
                let _ = child.ask(
                    Value::String("crash".into()),
                    Some(Duration::from_millis(50)),
                );
            }
        }
        thread::sleep(Duration::from_millis(15));
    }

    // Verify supervisor transitioned to Failed
    assert!(
        !sup.is_alive(),
        "Supervisor must halt when restart limit exceeded"
    );
    match sup.status() {
        SupervisorStatus::Failed(reason) => {
            assert!(
                reason.contains("Restart rate limit exceeded"),
                "Failure reason should reflect rate limit breach: {}",
                reason
            );
        }
        other => panic!("Supervisor status should be Failed, got {:?}", other),
    }

    // Verify innocent sibling was terminated by the clean cascade
    let mut innocent_dead = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !innocent.is_alive() {
            innocent_dead = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        innocent_dead,
        "Innocent sibling must be cleanly terminated on cascade"
    );

    // Verify attempting to start or restart children on a failed supervisor errors out
    let dummy_spec = ChildSpec::from_fn("dummy", |_| Ok(Value::Null));
    assert!(matches!(
        sup.start_child(dummy_spec).unwrap_err(),
        SupervisorError::SupervisorFailed(_)
    ));
    assert!(matches!(
        sup.restart_child("innocent_sibling").unwrap_err(),
        SupervisorError::SupervisorNotAlive
    ));
}

// =========================================================================
// PART 3: Idempotent and concurrent stop() calls
// =========================================================================

#[test]
fn test_concurrent_supervisor_stop_idempotency() {
    const NUM_THREADS: usize = 12;

    let config = SupervisorConfig::default();
    let spec1 = ChildSpec::from_fn("worker_x", |msg| Ok(msg));
    let spec2 = ChildSpec::from_fn("worker_y", |msg| Ok(msg));

    let sup = Supervisor::start(config, vec![spec1, spec2]).unwrap();
    let wx = sup.get_child("worker_x").unwrap();
    let wy = sup.get_child("worker_y").unwrap();

    let barrier = Arc::new(Barrier::new(NUM_THREADS));
    let true_count = Arc::new(AtomicUsize::new(0));
    let false_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..NUM_THREADS {
        let sup_clone = Arc::clone(&sup);
        let bar_clone = Arc::clone(&barrier);
        let t_count = Arc::clone(&true_count);
        let f_count = Arc::clone(&false_count);

        handles.push(thread::spawn(move || {
            bar_clone.wait();
            if sup_clone.stop() {
                t_count.fetch_add(1, Ordering::SeqCst);
            } else {
                f_count.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Exactly one thread should transition from Running to Stopped (return true)
    assert_eq!(
        true_count.load(Ordering::SeqCst),
        1,
        "Exactly one stop() call must return true"
    );
    assert_eq!(
        false_count.load(Ordering::SeqCst),
        NUM_THREADS - 1,
        "All other concurrent stop() calls must return false"
    );

    // Supervisor and all children must be stopped
    assert!(!sup.is_alive());
    assert_eq!(sup.status(), SupervisorStatus::Stopped);
    assert!(!wx.is_alive());
    assert!(!wy.is_alive());

    // Subsequent sequential stop() calls must also return false
    assert!(!sup.stop());
    assert!(!sup.stop());

    // join() on already stopped supervisor must succeed immediately
    assert!(sup.join().is_ok());
}

// =========================================================================
// PART 4: Dynamic operations under concurrent crash events
// =========================================================================

#[test]
fn test_dynamic_child_operations_under_concurrent_crashes() {
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 100, // Generous limit for high-frequency test
        within_seconds: 10.0,
    };

    let spec = ChildSpec::from_fn("crashing_worker", |msg| {
        if let Value::String(ref s) = msg {
            if s == "crash" {
                return Err("Deliberate background crash".into());
            }
        }
        Ok(Value::Int(1))
    });

    let sup = Supervisor::start(config, vec![spec]).unwrap();
    let running = Arc::new(AtomicBool::new(true));

    // Thread 1: Rapidly crash "crashing_worker"
    let sup_crasher = Arc::clone(&sup);
    let running_crasher = Arc::clone(&running);
    let crasher_handle = thread::spawn(move || {
        while running_crasher.load(Ordering::Acquire) {
            if let Some(child) = sup_crasher.get_child("crashing_worker") {
                let _ = child.ask(
                    Value::String("crash".into()),
                    Some(Duration::from_millis(20)),
                );
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    // Thread 2: Repeatedly query children() and get_child()
    let sup_reader = Arc::clone(&sup);
    let running_reader = Arc::clone(&running);
    let reader_handle = thread::spawn(move || {
        while running_reader.load(Ordering::Acquire) {
            let _ = sup_reader.children();
            let _ = sup_reader.get_child("crashing_worker");
            let _ = sup_reader.len();
            thread::sleep(Duration::from_millis(2));
        }
    });

    // Main thread: Dynamically start and stop temporary workers concurrently
    for i in 0..15 {
        let child_id = format!("temp_worker_{}", i);
        let spec = ChildSpec::from_fn(child_id.clone(), |msg| Ok(msg));
        let started = sup.start_child(spec);
        assert!(
            started.is_ok(),
            "start_child should succeed under concurrent crashes: {:?}",
            started.err()
        );

        let child = started.unwrap();
        assert!(child.is_alive());

        // Stop the dynamic child
        let stopped = sup.stop_child(&child_id);
        assert!(
            stopped.is_ok(),
            "stop_child should succeed under concurrent crashes: {:?}",
            stopped.err()
        );
        assert!(!child.is_alive());
        assert!(sup.get_child(&child_id).is_none());
    }

    // Stop background worker threads
    running.store(false, Ordering::Release);
    crasher_handle.join().unwrap();
    reader_handle.join().unwrap();

    sup.stop();
}

// =========================================================================
// PART 5: Stopped children cleanup & post-termination safety
// =========================================================================

#[test]
fn test_stopped_child_cannot_receive_messages_and_is_unregistered() {
    let sup = Supervisor::start(SupervisorConfig::default(), Vec::new()).unwrap();

    let spec = ChildSpec::from_fn("target_child", |msg| Ok(msg));
    let child = sup.start_child(spec).unwrap();
    assert!(child.is_alive());

    // Verify registered in ActorRegistry
    assert!(ActorRegistry::whereis("target_child").is_some());

    // Stop the child via supervisor
    sup.stop_child("target_child").unwrap();

    // Verify child is marked stopped
    assert!(!child.is_alive());
    assert_eq!(child.status(), ActorStatus::Stopped);

    // Verify get_child returns None
    assert!(sup.get_child("target_child").is_none());

    // Verify unregistered from ActorRegistry
    assert!(ActorRegistry::whereis("target_child").is_none());

    // Verify send returns ActorError::ActorStopped
    let send_err = child.send(Value::Int(42));
    assert_eq!(send_err, Err(ActorError::ActorStopped));

    // Verify ask returns ActorError::ActorStopped
    let ask_err = child.ask(Value::Int(42), None);
    assert_eq!(ask_err, Err(ActorError::ActorStopped));

    sup.stop();
}

// =========================================================================
// PART 6: Restart policies (Temporary, Transient, Permanent)
// =========================================================================

#[test]
fn test_restart_policy_temporary_never_restarts() {
    let config = SupervisorConfig::default();
    let spec = ChildSpec::from_fn("temp_child", |_| {
        Err("Temporary child intentional crash".into())
    })
    .with_restart(RestartPolicy::Temporary);

    let sup = Supervisor::start(config, vec![spec]).unwrap();
    let child = sup.get_child("temp_child").unwrap();

    // Crash the child
    let _ = child.ask(Value::Null, Some(Duration::from_millis(50)));

    // Wait and verify child stays dead and is never restarted
    thread::sleep(Duration::from_millis(150));
    assert!(
        !child.is_alive(),
        "Temporary child must stay dead after crash"
    );

    // get_child still returns the dead child handle (or None) with Failed/Stopped status
    if let Some(c) = sup.get_child("temp_child") {
        assert!(!c.is_alive());
    }

    sup.stop();
}

#[test]
fn test_restart_policy_transient_does_not_restart_on_clean_stop() {
    let config = SupervisorConfig::default();
    let spec =
        ChildSpec::from_fn("transient_child", |msg| Ok(msg)).with_restart(RestartPolicy::Transient);

    let sup = Supervisor::start(config, vec![spec]).unwrap();
    let child = sup.get_child("transient_child").unwrap();
    assert!(child.is_alive());

    // Cleanly stop child directly
    child.stop();
    let _ = child.join();

    // Verify supervisor does NOT restart the cleanly stopped transient child
    thread::sleep(Duration::from_millis(150));
    let cur_child = sup.get_child("transient_child").unwrap();
    assert!(
        !cur_child.is_alive(),
        "Transient child must NOT restart after clean stop"
    );

    sup.stop();
}

// =========================================================================
// PART 7: Sliding window rate limiter recovery
// =========================================================================

#[test]
fn test_sliding_window_rate_limiter_recovery() {
    // 2 restarts allowed within 0.4 seconds
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 3,
        within_seconds: 0.4,
    };

    let spec = ChildSpec::from_fn("recovering_looper", |msg| {
        if let Value::String(ref s) = msg {
            if s == "crash" {
                return Err("Crash requested".into());
            }
        }
        Ok(Value::String("ok".into()))
    });

    let sup = Supervisor::start(config, vec![spec]).unwrap();

    // Crash 2 times (under the limit of 3)
    for _ in 0..2 {
        for _ in 0..50 {
            if let Some(c) = sup.get_child("recovering_looper") {
                if c.is_alive() {
                    let _ = c.ask(
                        Value::String("crash".into()),
                        Some(Duration::from_millis(50)),
                    );
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(20));
    }

    // Verify supervisor is still alive after 2 restarts
    assert!(sup.is_alive());

    // Sleep long enough for the rolling window (0.4s) to expire older restarts
    thread::sleep(Duration::from_millis(500));

    // Now trigger 1 more crash — this would have exceeded 3 if not pruned!
    for _ in 0..50 {
        if let Some(c) = sup.get_child("recovering_looper") {
            if c.is_alive() {
                let _ = c.ask(
                    Value::String("crash".into()),
                    Some(Duration::from_millis(50)),
                );
                break;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    // Give supervisor time to process restart
    thread::sleep(Duration::from_millis(100));

    // Supervisor must STILL be alive because previous crashes rolled out of window!
    assert!(
        sup.is_alive(),
        "Supervisor must remain alive as older restarts aged out of rolling window"
    );

    sup.stop();
}

// =========================================================================
// PART 8: Manual restart_child race condition under Permanent restart policy
// =========================================================================

#[test]
fn test_restart_child_permanent_race_condition() {
    let spawn_counter = Arc::new(AtomicUsize::new(0));
    let sc = Arc::clone(&spawn_counter);

    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 20,
        within_seconds: 5.0,
    };

    let sc_closure = Arc::clone(&spawn_counter);
    let spec = ChildSpec::from_fn("bnd_race_worker", move |msg| {
        sc_closure.fetch_add(1, Ordering::SeqCst);
        Ok(msg)
    });

    let sup = Supervisor::start(config, vec![spec]).unwrap();

    // Loop 10 times restarting the child
    for i in 0..10 {
        let restarted = sup
            .restart_child("bnd_race_worker")
            .unwrap_or_else(|e| panic!("restart_child failed at iteration {}: {:?}", i, e));

        assert!(restarted.is_alive());

        // Yield to allow supervisor monitor thread to process events
        thread::sleep(Duration::from_millis(20));

        // Ensure restarted is the current child known to supervisor
        let current = sup.get_child("bnd_race_worker").expect("Child must exist");
        if !Arc::ptr_eq(&restarted, &current) {
            eprintln!(
                "BUG DETECTED at iteration {}: restarted {:?} (id={}) != current {:?} (id={}), total spawns = {}",
                i,
                Arc::as_ptr(&restarted),
                restarted.id(),
                Arc::as_ptr(&current),
                current.id(),
                sc.load(Ordering::SeqCst)
            );
            panic!(
                "restarted handle must match get_child; supervisor monitor spawned an extra actor concurrently! (iteration {})",
                i
            );
        }
    }

    let restarted = sup.get_child("bnd_race_worker").unwrap();
    sup.stop_child("bnd_race_worker").unwrap();
    assert!(
        !restarted.is_alive(),
        "Child must be cleanly dead after stop_child"
    );

    sup.stop();
}

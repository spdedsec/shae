//! Forensic Audit Integration Test Suite for Milestone 2:
//! Supervision Trees & Process Restart Policies

use shae::actor::{
    ActorRef, ActorStatus, ChildSpec, RestartPolicy, RestartStrategy, Supervisor, SupervisorConfig,
};
use shae::value::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn test_forensic_temporary_restart_policy_never_restarts() {
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 5,
        within_seconds: 5.0,
    };

    let factory_count = Arc::new(AtomicUsize::new(0));
    let fc = Arc::clone(&factory_count);

    let factory = Arc::new(move || {
        fc.fetch_add(1, Ordering::SeqCst);
        Ok(ActorRef::spawn_fn(|msg| {
            if let Value::String(s) = msg {
                if s == "crash" {
                    return Err("deliberate temp crash".into());
                }
            }
            Ok(Value::Null)
        }))
    });

    let spec = ChildSpec::new("temp_worker", factory).with_restart(RestartPolicy::Temporary);

    let sup = Supervisor::start(config, vec![spec]).expect("Supervisor should start");
    let child = sup.get_child("temp_worker").expect("Child must exist");

    assert_eq!(factory_count.load(Ordering::SeqCst), 1);
    assert!(child.is_alive());

    // Crash the temporary child
    let _ = child.ask(
        Value::String("crash".into()),
        Some(Duration::from_millis(200)),
    );
    assert!(!child.is_alive());

    // Wait 200ms to ensure supervisor does NOT restart it
    thread::sleep(Duration::from_millis(200));

    // Must still have factory_count == 1 (never restarted)
    assert_eq!(
        factory_count.load(Ordering::SeqCst),
        1,
        "Temporary child must NEVER be restarted"
    );
    let child_after = sup.get_child("temp_worker").unwrap();
    assert!(!child_after.is_alive());
    assert!(sup.is_alive(), "Supervisor itself must remain running");

    sup.stop();
}

#[test]
fn test_forensic_transient_restart_policy_normal_exit_vs_crash() {
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 5,
        within_seconds: 5.0,
    };

    let factory_count = Arc::new(AtomicUsize::new(0));
    let fc = Arc::clone(&factory_count);

    let factory = Arc::new(move || {
        fc.fetch_add(1, Ordering::SeqCst);
        Ok(ActorRef::spawn_fn(|msg| {
            if let Value::String(s) = msg {
                if s == "crash" {
                    return Err("deliberate crash".into());
                }
            }
            Ok(Value::Null)
        }))
    });

    let spec = ChildSpec::new("transient_worker", factory).with_restart(RestartPolicy::Transient);

    let sup = Supervisor::start(config, vec![spec]).expect("Supervisor should start");
    let child = sup.get_child("transient_worker").expect("Child must exist");
    assert_eq!(factory_count.load(Ordering::SeqCst), 1);

    // 1. Crash the transient child -> MUST restart
    let _ = child.ask(
        Value::String("crash".into()),
        Some(Duration::from_millis(200)),
    );

    let mut restarted = false;
    for _ in 0..100 {
        if factory_count.load(Ordering::SeqCst) > 1 {
            if let Some(c) = sup.get_child("transient_worker") {
                if c.is_alive() && !Arc::ptr_eq(&c, &child) {
                    restarted = true;
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
    assert!(
        restarted,
        "Transient child MUST restart upon crash/abnormal termination"
    );
    assert_eq!(factory_count.load(Ordering::SeqCst), 2);

    let child_restarted = sup.get_child("transient_worker").unwrap();

    // 2. Normal stop on transient child -> MUST NOT restart
    child_restarted.stop();
    thread::sleep(Duration::from_millis(200));

    assert_eq!(
        factory_count.load(Ordering::SeqCst),
        2,
        "Transient child MUST NOT restart on normal stop"
    );
    assert!(!child_restarted.is_alive());

    sup.stop();
}

#[test]
fn test_forensic_permanent_restart_policy_restarts_on_normal_stop() {
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 5,
        within_seconds: 5.0,
    };

    let spec = ChildSpec::from_fn("perm_worker", |_| Ok(Value::Null))
        .with_restart(RestartPolicy::Permanent);

    let sup = Supervisor::start(config, vec![spec]).expect("Supervisor should start");
    let child = sup.get_child("perm_worker").expect("Child must exist");

    // Stop child normally
    child.stop();

    // Supervisor must restart permanent child even on normal stop
    let mut restarted = false;
    for _ in 0..100 {
        if let Some(c) = sup.get_child("perm_worker") {
            if c.is_alive() && !Arc::ptr_eq(&c, &child) {
                restarted = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(15));
    }

    assert!(
        restarted,
        "Permanent child MUST be restarted even after normal stop"
    );

    sup.stop();
}

#[test]
fn test_forensic_rate_limiter_window_eviction() {
    // max 2 restarts within 0.3 seconds
    let config = SupervisorConfig {
        strategy: RestartStrategy::OneForOne,
        max_restarts: 2,
        within_seconds: 0.3,
    };

    let spec = ChildSpec::from_fn("spaced_worker", |msg| {
        if let Value::String(s) = msg {
            if s == "crash" {
                return Err("crash".into());
            }
        }
        Ok(Value::Null)
    });

    let sup = Supervisor::start(config, vec![spec]).unwrap();

    // 1st crash
    let c1 = sup.get_child("spaced_worker").unwrap();
    let _ = c1.ask(
        Value::String("crash".into()),
        Some(Duration::from_millis(100)),
    );

    // Wait until restarted
    let mut c2 = None;
    for _ in 0..50 {
        if let Some(c) = sup.get_child("spaced_worker") {
            if c.is_alive() && !Arc::ptr_eq(&c, &c1) {
                c2 = Some(c);
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(c2.is_some(), "First restart must succeed");
    assert!(sup.is_alive());

    // Sleep 400ms (window of 300ms expires!)
    thread::sleep(Duration::from_millis(400));

    // 2nd crash after window expired — should NOT exceed rate limit because 1st was pruned!
    let c2 = c2.unwrap();
    let _ = c2.ask(
        Value::String("crash".into()),
        Some(Duration::from_millis(100)),
    );

    let mut c3 = None;
    for _ in 0..50 {
        if let Some(c) = sup.get_child("spaced_worker") {
            if c.is_alive() && !Arc::ptr_eq(&c, &c2) {
                c3 = Some(c);
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        c3.is_some(),
        "Second spaced restart must succeed after window eviction"
    );
    assert!(
        sup.is_alive(),
        "Supervisor must stay alive because window pruned old restart"
    );

    sup.stop();
}

#[test]
fn test_forensic_thread_cleanup_no_orphans() {
    let config = SupervisorConfig::default();
    let spec1 = ChildSpec::from_fn("child_1", |_| Ok(Value::Null));
    let spec2 = ChildSpec::from_fn("child_2", |_| Ok(Value::Null));
    let spec3 = ChildSpec::from_fn("child_3", |_| Ok(Value::Null));

    let sup = Supervisor::start(config, vec![spec1, spec2, spec3]).unwrap();
    let c1 = sup.get_child("child_1").unwrap();
    let c2 = sup.get_child("child_2").unwrap();
    let c3 = sup.get_child("child_3").unwrap();

    assert!(sup.is_alive());
    assert!(c1.is_alive());
    assert!(c2.is_alive());
    assert!(c3.is_alive());

    // Stop supervisor
    let stopped = sup.stop_and_join().unwrap();
    assert!(stopped);

    // Verify all children are dead and stopped
    assert!(!sup.is_alive());
    assert!(!c1.is_alive());
    assert!(!c2.is_alive());
    assert!(!c3.is_alive());
    assert_eq!(c1.status(), ActorStatus::Stopped);
    assert_eq!(c2.status(), ActorStatus::Stopped);
    assert_eq!(c3.status(), ActorStatus::Stopped);
}

#[test]
fn test_forensic_all_for_one_state_reset_cascade() {
    let config = SupervisorConfig {
        strategy: RestartStrategy::AllForOne,
        max_restarts: 5,
        within_seconds: 5.0,
    };

    let spec1 =
        ChildSpec::from_stateful_fn("w1", Value::Int(10), |state, msg| match (state, msg) {
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        });
    let spec2 =
        ChildSpec::from_stateful_fn("w2", Value::Int(20), |state, msg| match (state, msg) {
            (Value::Int(_), Value::String(ref s)) if s == "crash" => {
                Err("w2 deliberate crash".into())
            }
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        });
    let spec3 =
        ChildSpec::from_stateful_fn("w3", Value::Int(30), |state, msg| match (state, msg) {
            (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
            (st, _) => Ok(st),
        });

    let sup = Supervisor::start(config, vec![spec1, spec2, spec3]).unwrap();
    let w1_0 = sup.get_child("w1").unwrap();
    let w2_0 = sup.get_child("w2").unwrap();
    let w3_0 = sup.get_child("w3").unwrap();

    // Mutate all workers
    assert_eq!(w1_0.ask(Value::Int(5), None).unwrap(), Value::Int(15));
    assert_eq!(w2_0.ask(Value::Int(5), None).unwrap(), Value::Int(25));
    assert_eq!(w3_0.ask(Value::Int(5), None).unwrap(), Value::Int(35));

    // Crash the middle worker w2
    let _ = w2_0.ask(
        Value::String("crash".into()),
        Some(Duration::from_millis(100)),
    );

    // Under AllForOne, w1, w2, and w3 MUST all be restarted with clean initial states (10, 20, 30)
    let mut all_restarted = false;
    for _ in 0..100 {
        if let (Some(w1), Some(w2), Some(w3)) = (
            sup.get_child("w1"),
            sup.get_child("w2"),
            sup.get_child("w3"),
        ) {
            if !Arc::ptr_eq(&w1, &w1_0) && !Arc::ptr_eq(&w2, &w2_0) && !Arc::ptr_eq(&w3, &w3_0) {
                if w1.is_alive() && w2.is_alive() && w3.is_alive() {
                    let s1 = w1.ask(Value::Int(0), Some(Duration::from_millis(50)));
                    let s2 = w2.ask(Value::Int(0), Some(Duration::from_millis(50)));
                    let s3 = w3.ask(Value::Int(0), Some(Duration::from_millis(50)));
                    if s1 == Ok(Value::Int(10))
                        && s2 == Ok(Value::Int(20))
                        && s3 == Ok(Value::Int(30))
                    {
                        all_restarted = true;
                        break;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(15));
    }

    assert!(
        all_restarted,
        "All 3 workers must be cleanly restarted with initial states under AllForOne"
    );
    sup.stop();
}

#[test]
fn test_forensic_dynamic_lifecycle_stress() {
    let sup = Supervisor::start(SupervisorConfig::default(), Vec::new()).unwrap();

    // Dynamically spawn 5 workers
    let mut child_refs = Vec::new();
    for i in 1..=5 {
        let name = format!("dyn_{}", i);
        let spec = ChildSpec::from_stateful_fn(&name, Value::Int(i * 10), |state, msg| {
            match (state, msg) {
                (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                (st, _) => Ok(st),
            }
        });
        let c = sup.start_child(spec).expect("Start child should succeed");
        child_refs.push((name, c));
    }

    assert_eq!(sup.len(), 5);

    // Send messages to all
    for (name, c) in &child_refs {
        let val = c.ask(Value::Int(1), None).unwrap();
        let idx: i64 = name.strip_prefix("dyn_").unwrap().parse().unwrap();
        assert_eq!(val, Value::Int(idx * 10 + 1));
    }

    // Stop 2 workers dynamically
    sup.stop_child("dyn_2").expect("Stop dyn_2");
    sup.stop_child("dyn_4").expect("Stop dyn_4");

    assert_eq!(sup.len(), 3);
    assert!(sup.get_child("dyn_2").is_none());
    assert!(sup.get_child("dyn_4").is_none());
    assert!(sup.get_child("dyn_1").is_some());
    assert!(sup.get_child("dyn_3").is_some());
    assert!(sup.get_child("dyn_5").is_some());

    // Stop supervisor, cascades to remaining 3
    sup.stop();
    assert!(!sup.is_alive());
    for (_, st) in sup.children() {
        assert!(!st.is_alive());
    }
}

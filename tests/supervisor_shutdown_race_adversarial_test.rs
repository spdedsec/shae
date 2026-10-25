//! Adversarial Stress Test: Supervisor Shutdown Concurrency Race & Thread Stability
//!
//! Specifically stress-tests:
//! 1. Concurrent `sup.stop()` during violent child crash cascades under `OneForOne`.
//! 2. Concurrent `sup.stop()` during violent child crash cascades under `AllForOne`.
//! 3. Concurrent `sup.stop()` during slow child factory instantiation (`spec.start`).
//! 4. Concurrent `sup.stop()` racing with manual `sup.restart_child()`.
//! 5. 50 rapid sequential supervisor start -> crash -> stop cycles with zero OS thread leakage.

use shae::actor::{
    ActorRef, ActorRegistry, ChildSpec, RestartStrategy, Supervisor, SupervisorConfig,
};
use shae::value::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(1000);

fn unique_id(prefix: &str) -> String {
    let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, std::process::id(), count)
}

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
    None
}

/// Test 1: Rapid OneForOne crashes with concurrent sup.stop()
#[test]
fn test_adversarial_one_for_one_shutdown_race_thread_stability() {
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");

    for cycle in 0..20 {
        let sup_id = unique_id(&format!("ofo_race_{}", cycle));
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 1000,
            within_seconds: 30.0,
        };

        let mut specs = Vec::new();
        for w in 0..4 {
            let cid = format!("{}_w{}", sup_id, w);
            specs.push(ChildSpec::from_stateful_fn(
                cid,
                Value::Int(0),
                |state, msg| match msg {
                    Value::String(ref s) if s == "die" => Err("adversarial crash".into()),
                    Value::Int(n) => {
                        let cur = match state {
                            Value::Int(c) => c,
                            _ => 0,
                        };
                        Ok(Value::Int(cur + n))
                    }
                    _ => Ok(state),
                },
            ));
        }

        let sup = Supervisor::start_named(sup_id.clone(), config, specs).expect("Supervisor start");
        let sup_c = Arc::clone(&sup);
        let stop_signal = Arc::new(AtomicBool::new(false));
        let stop_sig_c = Arc::clone(&stop_signal);
        let s_id = sup_id.clone();

        // Worker thread continuously crashing children
        let crasher = thread::spawn(move || {
            let mut i = 0;
            while !stop_sig_c.load(Ordering::Relaxed) {
                let target = format!("{}_w{}", s_id, i % 4);
                if let Some(child) = sup_c.get_child(&target) {
                    let _ = child.ask(Value::String("die".into()), Some(Duration::from_millis(10)));
                }
                i += 1;
                thread::sleep(Duration::from_millis(1));
            }
        });

        // Sleep arbitrary short duration so restarts are actively occurring
        thread::sleep(Duration::from_millis(10 + (cycle % 15) as u64));

        // Concurrently stop supervisor during crash storm
        assert!(
            sup.stop(),
            "Supervisor stop must return true on running supervisor"
        );
        let _ = sup.join();

        stop_signal.store(true, Ordering::Relaxed);
        let _ = crasher.join();

        // Check that all children are stopped
        for w in 0..4 {
            let cid = format!("{}_w{}", sup_id, w);
            if let Some(child) = sup.get_child(&cid) {
                assert!(
                    !child.is_alive(),
                    "Supervised child {} must be dead after sup.stop()",
                    cid
                );
            }
        }
    }

    // Grace period for OS pthread cleanup
    thread::sleep(Duration::from_millis(300));
    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Test 1 OneForOne Race] Initial: {}, Final: {}",
        initial_threads, final_threads
    );
    assert!(
        final_threads <= initial_threads + 1,
        "OS thread leak detected after OneForOne shutdown races! Initial: {}, Final: {}",
        initial_threads,
        final_threads
    );
}

/// Test 2: Rapid AllForOne crashes with concurrent sup.stop()
#[test]
fn test_adversarial_all_for_one_shutdown_race_thread_stability() {
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");

    for cycle in 0..15 {
        let sup_id = unique_id(&format!("afo_race_{}", cycle));
        let config = SupervisorConfig {
            strategy: RestartStrategy::AllForOne,
            max_restarts: 1000,
            within_seconds: 30.0,
        };

        let mut specs = Vec::new();
        for w in 0..3 {
            let cid = format!("{}_w{}", sup_id, w);
            specs.push(ChildSpec::from_stateful_fn(
                cid,
                Value::Int(0),
                |state, msg| match msg {
                    Value::String(ref s) if s == "die" => Err("adversarial crash".into()),
                    _ => Ok(state),
                },
            ));
        }

        let sup = Supervisor::start_named(sup_id.clone(), config, specs).expect("Supervisor start");
        let sup_c = Arc::clone(&sup);
        let stop_signal = Arc::new(AtomicBool::new(false));
        let stop_sig_c = Arc::clone(&stop_signal);
        let s_id = sup_id.clone();

        let crasher = thread::spawn(move || {
            let mut i = 0;
            while !stop_sig_c.load(Ordering::Relaxed) {
                let target = format!("{}_w{}", s_id, i % 3);
                if let Some(child) = sup_c.get_child(&target) {
                    let _ = child.ask(Value::String("die".into()), Some(Duration::from_millis(10)));
                }
                i += 1;
                thread::sleep(Duration::from_millis(2));
            }
        });

        thread::sleep(Duration::from_millis(15 + (cycle % 20) as u64));

        assert!(sup.stop());
        let _ = sup.join();

        stop_signal.store(true, Ordering::Relaxed);
        let _ = crasher.join();

        for w in 0..3 {
            let cid = format!("{}_w{}", sup_id, w);
            if let Some(child) = sup.get_child(&cid) {
                assert!(
                    !child.is_alive(),
                    "Supervised child {} must be dead after sup.stop()",
                    cid
                );
            }
        }
    }

    thread::sleep(Duration::from_millis(300));
    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Test 2 AllForOne Race] Initial: {}, Final: {}",
        initial_threads, final_threads
    );
    assert!(
        final_threads <= initial_threads + 1,
        "OS thread leak detected after AllForOne shutdown races! Initial: {}, Final: {}",
        initial_threads,
        final_threads
    );
}

/// Test 3: Slow Factory - Supervisor stops while factory is executing
#[test]
fn test_adversarial_slow_factory_shutdown_race() {
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");

    for cycle in 0..10 {
        let sup_id = unique_id(&format!("slow_fac_{}", cycle));
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 100,
            within_seconds: 30.0,
        };

        let factory_active = Arc::new(AtomicBool::new(false));
        let factory_active_c = Arc::clone(&factory_active);

        let cid = format!("{}_slow", sup_id);
        let factory: shae::actor::ActorFactory = Arc::new(move || {
            factory_active_c.store(true, Ordering::SeqCst);
            // Delay during factory to widen race window with supervisor stop
            thread::sleep(Duration::from_millis(40));
            let actor = ActorRef::spawn_stateful_fn(Value::Int(0), |st, msg| match msg {
                Value::String(ref s) if s == "die" => Err("die".into()),
                _ => Ok(st),
            });
            Ok(actor)
        });

        let spec = ChildSpec::new(cid.clone(), factory);
        let sup = Supervisor::start_named(sup_id.clone(), config, vec![spec]).expect("start");

        // Wait for initial spawn to complete
        thread::sleep(Duration::from_millis(60));

        // Crash the child to trigger slow factory restart
        factory_active.store(false, Ordering::SeqCst);
        let child = sup.get_child(&cid).expect("child exists");
        let _ = child.ask(Value::String("die".into()), Some(Duration::from_millis(20)));

        // Wait until factory has started executing
        let wait_start = Instant::now();
        while !factory_active.load(Ordering::SeqCst)
            && wait_start.elapsed() < Duration::from_millis(500)
        {
            thread::sleep(Duration::from_millis(2));
        }

        // Now STOP supervisor while factory is inside its sleep
        assert!(sup.stop());
        let _ = sup.join();

        // Wait for factory to finish its sleep and return
        thread::sleep(Duration::from_millis(60));

        // Ensure registry has no dangling reference
        let registered = ActorRegistry::whereis(&cid);
        assert!(
            registered.is_none() || !registered.as_ref().unwrap().is_alive(),
            "Dangling alive actor in registry after slow factory stop!"
        );
    }

    thread::sleep(Duration::from_millis(300));
    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Test 3 Slow Factory] Initial: {}, Final: {}",
        initial_threads, final_threads
    );
    assert!(
        final_threads <= initial_threads + 1,
        "OS thread leak detected in slow factory shutdown race! Initial: {}, Final: {}",
        initial_threads,
        final_threads
    );
}

/// Test 4: Concurrent restart_child() calls racing with sup.stop()
#[test]
fn test_adversarial_concurrent_restart_child_racing_with_stop() {
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");

    for cycle in 0..15 {
        let sup_id = unique_id(&format!("restarts_stop_race_{}", cycle));
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 1000,
            within_seconds: 30.0,
        };

        let mut specs = Vec::new();
        for w in 0..3 {
            let cid = format!("{}_w{}", sup_id, w);
            specs.push(ChildSpec::from_stateful_fn(
                cid,
                Value::Int(0),
                |state, msg| match msg {
                    Value::String(ref s) if s == "die" => Err("die".into()),
                    _ => Ok(state),
                },
            ));
        }

        let sup = Supervisor::start_named(sup_id.clone(), config, specs).expect("start");

        let stop_signal = Arc::new(AtomicBool::new(false));
        let mut restarters = Vec::new();

        for th in 0..4 {
            let sup_c = Arc::clone(&sup);
            let s_id = sup_id.clone();
            let stop_sig = Arc::clone(&stop_signal);
            restarters.push(thread::spawn(move || {
                let mut i = 0;
                while !stop_sig.load(Ordering::Relaxed) {
                    let target = format!("{}_w{}", s_id, (i + th) % 3);
                    let _ = sup_c.restart_child(&target);
                    i += 1;
                    thread::sleep(Duration::from_millis(1));
                }
            }));
        }

        thread::sleep(Duration::from_millis(15 + (cycle % 15) as u64));

        // Stop supervisor concurrently while 4 threads hammer restart_child
        let _ = sup.stop();
        let _ = sup.join();

        stop_signal.store(true, Ordering::Relaxed);
        for r in restarters {
            let _ = r.join();
        }

        for w in 0..3 {
            let cid = format!("{}_w{}", sup_id, w);
            if let Some(child) = sup.get_child(&cid) {
                assert!(
                    !child.is_alive(),
                    "Supervised child {} must be dead after sup.stop()",
                    cid
                );
            }
        }
    }

    thread::sleep(Duration::from_millis(300));
    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Test 4 Concurrent restart_child vs stop] Initial: {}, Final: {}",
        initial_threads, final_threads
    );
    assert!(
        final_threads <= initial_threads + 1,
        "OS thread leak detected in restart_child vs stop race! Initial: {}, Final: {}",
        initial_threads,
        final_threads
    );
}

/// Test 5: 40 Rapid Sequential Supervisor Start -> Crash -> Stop Cycles
#[test]
fn test_adversarial_rapid_supervisor_restart_cycles_stability() {
    let initial_threads = get_os_thread_count().expect("Must read /proc/self/status");

    const CYCLES: usize = 40;
    for c in 0..CYCLES {
        let sup_id = unique_id(&format!("rapid_cycle_{}", c));
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 10,
            within_seconds: 10.0,
        };

        let cid = format!("{}_worker", sup_id);
        let spec = ChildSpec::from_stateful_fn(cid.clone(), Value::Int(0), |st, msg| match msg {
            Value::String(ref s) if s == "die" => Err("planned death".into()),
            Value::Int(n) => {
                let v = match st {
                    Value::Int(x) => x,
                    _ => 0,
                };
                Ok(Value::Int(v + n))
            }
            _ => Ok(st),
        });

        let sup = Supervisor::start_named(sup_id.clone(), config, vec![spec]).expect("start");

        // Send a few messages
        if let Some(child) = sup.get_child(&cid) {
            let _ = child.send(Value::Int(5));
        }

        // Cause a crash
        if let Some(child) = sup.get_child(&cid) {
            let _ = child.ask(Value::String("die".into()), Some(Duration::from_millis(20)));
        }

        // Wait a tiny moment for restart
        thread::sleep(Duration::from_millis(5));

        // Stop
        assert!(sup.stop());
        let _ = sup.join();
    }

    thread::sleep(Duration::from_millis(250));
    let final_threads = get_os_thread_count().expect("Must read /proc/self/status");
    println!(
        "[Test 5 Rapid 40 Cycles] Initial: {}, Final: {}",
        initial_threads, final_threads
    );
    assert!(
        final_threads <= initial_threads,
        "OS thread leak detected after 40 rapid supervisor cycles! Initial: {}, Final: {}",
        initial_threads,
        final_threads
    );
}

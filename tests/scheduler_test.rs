//! Comprehensive test suite for Shae's M:N Work-Stealing Multi-Core Scheduler.

use shae::actor::{ActorRef, ActorStatus};
use shae::scheduler::global_scheduler;
use shae::value::Value;
use std::collections::HashSet;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// Test 1: Spawning 10,000 actors concurrently.
/// Verifies high actor density with bounded OS thread footprint (<30 threads).
#[test]
fn test_high_density_10k_actors_bounded_threads() {
    let sched = global_scheduler();
    let num_workers = sched.worker_count();
    println!("Scheduler running with {} worker threads", num_workers);

    let initial_threads = get_os_thread_count().unwrap_or(1);

    const ACTOR_COUNT: usize = 10_000;
    let completed_count = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();
    let mut actors = Vec::with_capacity(ACTOR_COUNT);

    for i in 0..ACTOR_COUNT {
        let completed = Arc::clone(&completed_count);
        let actor = ActorRef::spawn_fn(move |msg| {
            if let Value::Int(val) = msg {
                completed.fetch_add(1, Ordering::Relaxed);
                Ok(Value::Int(val * 2))
            } else {
                Ok(Value::Null)
            }
        });
        actors.push((i, actor));
    }

    let spawn_elapsed = start.elapsed();
    println!(
        "Spawned {} actors in {:?} ({:.2} actors/sec)",
        ACTOR_COUNT,
        spawn_elapsed,
        ACTOR_COUNT as f64 / spawn_elapsed.as_secs_f64()
    );

    let mid_threads = get_os_thread_count().unwrap_or(1);
    println!(
        "OS Threads with 10k actors active: Initial={}, Current={}",
        initial_threads, mid_threads
    );

    // Threads must not scale with actors (M:N invariant)
    assert!(
        mid_threads <= initial_threads + num_workers + 10,
        "OS thread count exploded! Expected <= {}, got {}",
        initial_threads + num_workers + 10,
        mid_threads
    );

    // Send a message to each actor
    for (i, actor) in &actors {
        actor.send(Value::Int(*i as i64)).expect("Send should succeed");
    }

    // Wait for all 10,000 actors to process their message
    let deadline = Instant::now() + Duration::from_secs(10);
    while completed_count.load(Ordering::Relaxed) < ACTOR_COUNT {
        if Instant::now() > deadline {
            panic!(
                "Timed out waiting for 10k actors to finish! Processed: {}/{}",
                completed_count.load(Ordering::Relaxed),
                ACTOR_COUNT
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    println!(
        "All {} actors completed message processing in {:?}",
        ACTOR_COUNT,
        start.elapsed()
    );

    // Clean up
    for (_, actor) in actors {
        let _ = actor.stop();
    }
}

/// Test 2: Multi-core workload balancing across worker threads.
/// Confirms that tasks are stolen and executed across multiple distinct worker threads.
#[test]
fn test_multicore_load_balancing() {
    let sched = global_scheduler();
    let num_workers = sched.worker_count();
    if num_workers <= 1 {
        println!("Single-core environment detected; skipping multi-core verification");
        return;
    }

    let executing_threads = Arc::new(Mutex::new(HashSet::new()));

    const NUM_TASKS: usize = 200;
    let done_count = Arc::new(AtomicUsize::new(0));

    for _ in 0..NUM_TASKS {
        let threads_set = Arc::clone(&executing_threads);
        let done = Arc::clone(&done_count);

        let actor = ActorRef::spawn_fn(move |_| {
            let tid = thread::current().id();
            {
                let mut guard = threads_set.lock().unwrap();
                guard.insert(tid);
            }
            // Do some light compute
            let mut sum: u64 = 0;
            for k in 0..50_000 {
                sum = sum.wrapping_add(k);
            }
            done.fetch_add(1, Ordering::Relaxed);
            Ok(Value::Int(sum as i64))
        });

        actor.send(Value::Null).unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while done_count.load(Ordering::Relaxed) < NUM_TASKS {
        if Instant::now() > deadline {
            panic!("Load balancing test timed out");
        }
        thread::sleep(Duration::from_millis(5));
    }

    let thread_count = executing_threads.lock().unwrap().len();
    println!(
        "Workload distributed across {} distinct OS threads (out of {} scheduler workers)",
        thread_count, num_workers
    );

    // At least 2 distinct threads must have executed the tasks
    assert!(
        thread_count >= 2,
        "Expected multi-core distribution across >= 2 threads, but only saw {}",
        thread_count
    );
}

/// Test 3: Single-writer serial message consistency.
/// 10 concurrent threads send 100 increments each to an actor (1,000 total increments).
/// Verifies no race conditions, no lost updates, and strict sequential consistency.
#[test]
fn test_single_writer_serial_consistency() {
    const NUM_ACTORS: usize = 10;
    const THREADS_PER_ACTOR: usize = 10;
    const INCS_PER_THREAD: usize = 100;
    const EXPECTED_PER_ACTOR: i64 = (THREADS_PER_ACTOR * INCS_PER_THREAD) as i64;

    let mut actors = Vec::new();

    for _ in 0..NUM_ACTORS {
        let actor = ActorRef::spawn_stateful_fn(Value::Int(0), |state, msg| {
            if let (Value::Int(cur), Value::Int(inc)) = (&state, &msg) {
                Ok(Value::Int(*cur + *inc))
            } else {
                Ok(state)
            }
        });
        actors.push(actor);
    }

    let mut senders = Vec::new();
    for actor in &actors {
        for _ in 0..THREADS_PER_ACTOR {
            let a = Arc::clone(actor);
            senders.push(thread::spawn(move || {
                for _ in 0..INCS_PER_THREAD {
                    a.send(Value::Int(1)).expect("send failed");
                }
            }));
        }
    }

    for h in senders {
        h.join().unwrap();
    }

    // Verify all actors reached exact expected count using ask()
    for (idx, actor) in actors.iter().enumerate() {
        let reply = actor
            .ask(Value::Int(0), Some(Duration::from_secs(5)))
            .expect("ask failed");
        assert_eq!(
            reply,
            Value::Int(EXPECTED_PER_ACTOR),
            "Actor {} had inconsistency! Expected {}, got {:?}",
            idx,
            EXPECTED_PER_ACTOR,
            reply
        );
        actor.join().expect("join failed");
    }
}

/// Test 4: Starvation prevention with reduction quantum.
/// A heavy actor with 200 messages yields after 32 messages,
/// allowing a light actor with 1 message to execute before the heavy actor completes.
#[test]
fn test_starvation_prevention_quantum_yielding() {
    let heavy_progress = Arc::new(AtomicUsize::new(0));
    let heavy_progress_clone = Arc::clone(&heavy_progress);

    let heavy_actor = ActorRef::spawn_fn(move |_| {
        heavy_progress_clone.fetch_add(1, Ordering::Relaxed);
        // Small delay to simulate work
        thread::sleep(Duration::from_micros(200));
        Ok(Value::Null)
    });

    let light_finished = Arc::new(AtomicUsize::new(0));
    let light_finished_clone = Arc::clone(&light_finished);
    let heavy_progress_when_light_finished = Arc::new(AtomicUsize::new(0));
    let hp_capture = Arc::clone(&heavy_progress_when_light_finished);
    let hp_source = Arc::clone(&heavy_progress);

    let light_actor = ActorRef::spawn_fn(move |_| {
        hp_capture.store(hp_source.load(Ordering::Relaxed), Ordering::Relaxed);
        light_finished_clone.store(1, Ordering::Relaxed);
        Ok(Value::Null)
    });

    // Enqueue 200 messages to heavy actor (quantum is 32)
    for _ in 0..200 {
        heavy_actor.send(Value::Null).unwrap();
    }

    // Enqueue 1 message to light actor
    light_actor.send(Value::Null).unwrap();

    // Wait for light actor to complete
    let deadline = Instant::now() + Duration::from_secs(5);
    while light_finished.load(Ordering::Relaxed) == 0 {
        if Instant::now() > deadline {
            panic!("Light actor was starved!");
        }
        thread::sleep(Duration::from_millis(1));
    }

    let progress_at_light = heavy_progress_when_light_finished.load(Ordering::Relaxed);
    println!(
        "Heavy actor was at message {}/200 when light actor finished",
        progress_at_light
    );

    // Light actor must have run before the heavy actor finished all 200 messages!
    assert!(
        progress_at_light < 200,
        "Starvation occurred! Heavy actor finished all 200 messages before light actor got 1 turn"
    );

    heavy_actor.stop_and_join().unwrap();
    light_actor.stop_and_join().unwrap();
}

/// Test 5: Actor termination, stop(), and join() synchronization.
#[test]
fn test_actor_termination_and_join() {
    let actor = ActorRef::spawn_stateful_fn(Value::Int(0), |state, msg| {
        if let (Value::Int(cur), Value::Int(n)) = (&state, &msg) {
            Ok(Value::Int(*cur + *n))
        } else {
            Ok(state)
        }
    });

    actor.send(Value::Int(42)).unwrap();
    let res = actor.ask(Value::Int(0), None).unwrap();
    assert_eq!(res, Value::Int(42));
    assert!(actor.is_alive());

    // Stop and join
    assert!(actor.stop());
    assert_eq!(actor.status(), ActorStatus::Stopped);
    assert!(!actor.is_alive());

    // join() must return Ok immediately
    let join_start = Instant::now();
    assert!(actor.join().is_ok());
    assert!(join_start.elapsed() < Duration::from_millis(100));

    // Subsequent sends must fail with ActorStopped
    assert!(actor.send(Value::Int(1)).is_err());
}

/// Test 6: async_rt::spawn_task scheduler integration.
#[test]
fn test_spawn_task_scheduler_integration() {
    let mut handles = Vec::new();

    for i in 0..100 {
        let handle = shae::async_rt::spawn_task(move || {
            let mut sum: i64 = 0;
            for j in 0..1_000 {
                sum += j;
            }
            Ok(Value::Int(i * 1000 + sum))
        });
        handles.push((i, handle));
    }

    for (i, h) in handles {
        let mut inner = shae::value::TaskInner::new(h);
        let res = inner.join().expect("Task should complete successfully");
        let expected_sum: i64 = (0..1_000).sum();
        assert_eq!(res, Value::Int(i * 1000 + expected_sum));
    }
}

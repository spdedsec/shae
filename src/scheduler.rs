//! M:N work-stealing multi-core scheduler for Shae.
//!
//! Maps M actors and async tasks onto N worker threads (sized to CPU parallelism)
//! using lock-free work-stealing deques (via `crossbeam-deque`), a global injector,
//! bounded reduction quanta (32 messages), periodic injector polling (64 ticks),
//! and 0% CPU idle parking via condition variables.

use crate::actor::ActorInner;
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Maximum number of messages an actor processes in a single scheduling slice
/// before yielding to prevent starvation.
pub const QUANTUM_REDUCTIONS: usize = 32;

/// A schedulable work item in the M:N runtime.
pub enum SchedItem {
    /// An actor whose mailbox has pending messages.
    Actor(Arc<ActorInner>),
    /// A general asynchronous closure task.
    Task(Box<dyn FnOnce() + Send + 'static>),
}

/// Work-stealing scheduler managing a multi-core thread pool.
pub struct Scheduler {
    injector: Arc<Injector<SchedItem>>,
    stealers: Arc<Vec<Stealer<SchedItem>>>,
    _worker_handles: Mutex<Vec<JoinHandle<()>>>,
    num_workers: usize,
    sleeping_count: Arc<AtomicUsize>,
    park_lock: Arc<Mutex<()>>,
    park_cvar: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
}

impl Scheduler {
    /// Creates a new work-stealing scheduler with `num_workers` threads.
    pub fn new(num_workers: usize) -> Self {
        let num_workers = num_workers.max(1);
        let injector = Arc::new(Injector::new());

        let mut workers = Vec::with_capacity(num_workers);
        let mut stealers = Vec::with_capacity(num_workers);

        for _ in 0..num_workers {
            let w = Worker::new_fifo();
            stealers.push(w.stealer());
            workers.push(w);
        }

        let stealers = Arc::new(stealers);
        let sleeping_count = Arc::new(AtomicUsize::new(0));
        let park_lock = Arc::new(Mutex::new(()));
        let park_cvar = Arc::new(Condvar::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let mut worker_handles = Vec::with_capacity(num_workers);

        for (worker_id, worker) in workers.into_iter().enumerate() {
            let inj = Arc::clone(&injector);
            let stl = Arc::clone(&stealers);
            let sleep_cnt = Arc::clone(&sleeping_count);
            let p_lock = Arc::clone(&park_lock);
            let p_cvar = Arc::clone(&park_cvar);
            let s_down = Arc::clone(&shutdown);

            let handle = thread::Builder::new()
                .name(format!("shae-sched-{}", worker_id))
                .spawn(move || {
                    worker_loop(
                        worker_id,
                        worker,
                        inj,
                        stl,
                        sleep_cnt,
                        p_lock,
                        p_cvar,
                        s_down,
                    );
                })
                .expect("Failed to spawn scheduler worker thread");

            worker_handles.push(handle);
        }

        Self {
            injector,
            stealers,
            _worker_handles: Mutex::new(worker_handles),
            num_workers,
            sleeping_count,
            park_lock,
            park_cvar,
            shutdown,
        }
    }

    /// Schedules an actor whose mailbox needs processing.
    pub fn schedule_actor(&self, actor: Arc<ActorInner>) {
        self.injector.push(SchedItem::Actor(actor));
        self.wake_one();
    }

    /// Schedules an asynchronous task closure onto the multi-core pool.
    pub fn spawn_task<F: FnOnce() + Send + 'static>(&self, f: F) {
        self.injector.push(SchedItem::Task(Box::new(f)));
        self.wake_one();
    }

    /// Returns the number of worker threads in the pool.
    pub fn worker_count(&self) -> usize {
        self.num_workers
    }

    pub fn stealers(&self) -> &Arc<Vec<Stealer<SchedItem>>> {
        &self.stealers
    }

    /// Checks if all worker threads are currently sleeping/idle.
    pub fn is_idle(&self) -> bool {
        self.sleeping_count.load(Ordering::SeqCst) == self.num_workers
            && self.injector.is_empty()
    }

    /// Notifies one sleeping worker that work is available.
    fn wake_one(&self) {
        if self.sleeping_count.load(Ordering::SeqCst) > 0 {
            let _guard = match self.park_lock.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            self.park_cvar.notify_one();
        }
    }

    /// Signals shutdown to all worker threads.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _guard = match self.park_lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        self.park_cvar.notify_all();
    }
}

static GLOBAL_SCHEDULER: OnceLock<Scheduler> = OnceLock::new();

/// Returns a reference to the global singleton work-stealing scheduler.
pub fn global_scheduler() -> &'static Scheduler {
    GLOBAL_SCHEDULER.get_or_init(|| {
        let parallelism = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let pool_size = (parallelism * 8).max(16);
        Scheduler::new(pool_size)
    })
}

/// Main execution loop for each worker thread.
fn worker_loop(
    worker_id: usize,
    worker: Worker<SchedItem>,
    injector: Arc<Injector<SchedItem>>,
    stealers: Arc<Vec<Stealer<SchedItem>>>,
    sleeping_count: Arc<AtomicUsize>,
    park_lock: Arc<Mutex<()>>,
    park_cvar: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
) {
    let mut tick: usize = 0;
    let mut rng: usize = (worker_id + 1).wrapping_mul(0x9e3779b9);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        tick = tick.wrapping_add(1);

        // 1. Periodic global injector check (every 64 ticks) to guarantee fairness
        // and prevent local loops from starving externally injected tasks.
        let mut next_item = if tick % 64 == 0 {
            find_injector_item(&injector, &worker)
        } else {
            None
        };

        // 2. Pop from local deque
        if next_item.is_none() {
            next_item = worker.pop();
        }

        // 3. If local deque was empty, check global injector
        if next_item.is_none() {
            next_item = find_injector_item(&injector, &worker);
        }

        // 4. If injector was empty, steal from peers
        if next_item.is_none() {
            next_item = steal_from_peers(worker_id, &stealers, &worker, &mut rng);
        }

        // 5. If work was found, execute it
        if let Some(item) = next_item {
            run_sched_item(item, &worker);
            continue;
        }

        // 6. No work found: spin briefly, then park
        let mut spun_item = None;
        for _ in 0..4 {
            std::hint::spin_loop();
            if let Some(item) = worker.pop() {
                spun_item = Some(item);
                break;
            }
            if let Some(item) = find_injector_item(&injector, &worker) {
                spun_item = Some(item);
                break;
            }
        }

        if let Some(item) = spun_item {
            run_sched_item(item, &worker);
            continue;
        }

        // Park idle worker
        sleeping_count.fetch_add(1, Ordering::SeqCst);

        // Re-check after announcing sleeping to close race windows
        let recheck = worker
            .pop()
            .or_else(|| find_injector_item(&injector, &worker))
            .or_else(|| steal_from_peers(worker_id, &stealers, &worker, &mut rng));

        if let Some(item) = recheck {
            sleeping_count.fetch_sub(1, Ordering::SeqCst);
            run_sched_item(item, &worker);
            continue;
        }

        // Sleep on condvar with timeout
        {
            let guard = match park_lock.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let _ = park_cvar.wait_timeout(guard, Duration::from_millis(10));
        }

        sleeping_count.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Executes a single work item.
fn run_sched_item(item: SchedItem, worker: &Worker<SchedItem>) {
    match item {
        SchedItem::Actor(actor) => {
            let has_more = actor.execute_slice(QUANTUM_REDUCTIONS);
            if has_more {
                // Actor yielded because it reached its quantum: re-enqueue on local deque
                worker.push(SchedItem::Actor(actor));
            }
        }
        SchedItem::Task(task) => {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
        }
    }
}

/// Steals a batch of tasks from the global injector and pops one.
fn find_injector_item(
    injector: &Injector<SchedItem>,
    worker: &Worker<SchedItem>,
) -> Option<SchedItem> {
    loop {
        match injector.steal_batch_and_pop(worker) {
            Steal::Success(item) => return Some(item),
            Steal::Empty => return None,
            Steal::Retry => continue,
        }
    }
}

/// Attempts to steal work from peer workers starting at a pseudo-random index.
fn steal_from_peers(
    worker_id: usize,
    stealers: &[Stealer<SchedItem>],
    worker: &Worker<SchedItem>,
    rng: &mut usize,
) -> Option<SchedItem> {
    let num_workers = stealers.len();
    if num_workers <= 1 {
        return None;
    }

    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    let start = *rng % num_workers;

    for i in 0..num_workers {
        let victim = (start + i) % num_workers;
        if victim == worker_id {
            continue;
        }

        loop {
            match stealers[victim].steal_batch_and_pop(worker) {
                Steal::Success(item) => return Some(item),
                Steal::Empty => break,
                Steal::Retry => continue,
            }
        }
    }

    None
}

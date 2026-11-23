use crate::eval::RuntimeError;
use crate::value::Value;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct ThreadPool {
    sender: Option<Sender<Job>>,
    _workers: Vec<thread::JoinHandle<()>>,
}

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        let (sender, receiver) = channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for _ in 0..size {
            let rx = receiver.clone();
            let handle = thread::Builder::new()
                .name("shae-worker".to_string())
                .spawn(move || {
                    while let Ok(job) = rx.lock().unwrap().recv() {
                        job();
                    }
                })
                .expect("Failed to spawn worker thread");
            workers.push(handle);
        }

        Self {
            sender: Some(sender),
            _workers: workers,
        }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(Box::new(f));
        }
    }
}

static GLOBAL_POOL: OnceLock<ThreadPool> = OnceLock::new();

pub fn global_pool() -> &'static ThreadPool {
    GLOBAL_POOL.get_or_init(|| {
        let num_cpus = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let pool_size = (num_cpus * 4).max(8);
        ThreadPool::new(pool_size)
    })
}

pub fn spawn_task<F>(f: F) -> crate::value::TaskHandle
where
    F: FnOnce() -> Result<Value, RuntimeError> + Send + 'static,
{
    let parent = std::thread::current().id();
    let shared = Arc::new(crate::value::TaskShared {
        result: Mutex::new(None),
        cv: std::sync::Condvar::new(),
        finished: std::sync::atomic::AtomicBool::new(false),
    });

    let s = Arc::clone(&shared);
    crate::scheduler::global_scheduler().spawn_task(move || {
        crate::test_runner::register_worker_thread(parent);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        crate::test_runner::unregister_worker_thread();

        let final_res = match res {
            Ok(r) => r,
            Err(_) => Err(RuntimeError::new("Async task panicked".to_string())),
        };

        {
            let mut guard = match s.result.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            *guard = Some(final_res);
        }
        s.finished.store(true, std::sync::atomic::Ordering::Release);
        s.cv.notify_all();
    });

    crate::value::TaskHandle::Async(shared)
}

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

pub fn spawn_task<F>(f: F) -> std::thread::JoinHandle<Result<Value, RuntimeError>>
where
    F: FnOnce() -> Result<Value, RuntimeError> + Send + 'static,
{
    thread::spawn(f)
}

use crate::ast::Span;
use crate::eval::RuntimeError;
use crate::value::Value;
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub struct ChannelInner {
    queue: VecDeque<Value>,
    capacity: Option<usize>,
    closed: bool,
}

pub struct Channel {
    inner: Mutex<ChannelInner>,
    not_empty: Condvar,
    not_full: Condvar,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock().unwrap();
        write!(
            f,
            "Channel {{ len: {}, capacity: {:?}, closed: {} }}",
            inner.queue.len(),
            inner.capacity,
            inner.closed
        )
    }
}

impl Channel {
    pub fn new(capacity: Option<usize>) -> Arc<Self> {
        let cap = match capacity {
            Some(0) => None,
            other => other,
        };
        Arc::new(Self {
            inner: Mutex::new(ChannelInner {
                queue: VecDeque::new(),
                capacity: cap,
                closed: false,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        })
    }

    pub fn send(&self, val: Value, span: Span) -> Result<(), RuntimeError> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            if inner.closed {
                return Err(RuntimeError::new("Cannot send on a closed channel".into()).at(span));
            }
            if let Some(cap) = inner.capacity {
                if inner.queue.len() >= cap {
                    inner = self.not_full.wait(inner).unwrap();
                    continue;
                }
            }
            inner.queue.push_back(val);
            self.not_empty.notify_one();
            return Ok(());
        }
    }

    pub fn recv(&self, timeout_ms: Option<u64>, _span: Span) -> Result<Value, RuntimeError> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            if let Some(val) = inner.queue.pop_front() {
                self.not_full.notify_one();
                return Ok(val);
            }
            if inner.closed {
                return Ok(Value::Null);
            }
            if let Some(timeout) = timeout_ms {
                let (new_inner, timeout_result) = self
                    .not_empty
                    .wait_timeout(inner, Duration::from_millis(timeout))
                    .unwrap();
                inner = new_inner;
                if timeout_result.timed_out() {
                    return Ok(inner.queue.pop_front().unwrap_or(Value::Null));
                }
            } else {
                inner = self.not_empty.wait(inner).unwrap();
            }
        }
    }

    pub fn try_recv(&self, _span: Span) -> Result<Value, RuntimeError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(val) = inner.queue.pop_front() {
            self.not_full.notify_one();
            Ok(val)
        } else {
            Ok(Value::Null)
        }
    }

    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().queue.is_empty()
    }

    pub fn capacity(&self) -> Option<usize> {
        self.inner.lock().unwrap().capacity
    }
}

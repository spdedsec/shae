use crate::ast::Span;
use crate::ast::TypeAnnotation;
use crate::eval::RuntimeError;
use crate::value::Value;
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

static NEXT_WAITER_ID: AtomicU64 = AtomicU64::new(1);

fn next_waiter_id() -> u64 {
    NEXT_WAITER_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct SelectWinner {
    pub case_idx: usize,
    pub value: Option<Value>,
}

pub struct SelectToken {
    pub state: Mutex<Option<SelectWinner>>,
    pub cond: Condvar,
}

impl SelectToken {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(None),
            cond: Condvar::new(),
        }
    }
}

pub struct SenderWaiter {
    pub id: u64,
    pub val: Value,
    pub token: Option<Arc<SelectToken>>,
    pub case_idx: usize,
    pub done_cond: Arc<Condvar>,
    pub done_mutex: Arc<Mutex<bool>>,
    pub is_closed_err: Arc<Mutex<bool>>,
}

impl SenderWaiter {
    pub fn is_active(&self) -> bool {
        if *self.is_closed_err.lock().unwrap() {
            return false;
        }
        if let Some(ref tok) = self.token {
            tok.state.lock().unwrap().is_none()
        } else {
            !*self.done_mutex.lock().unwrap()
        }
    }

    pub fn wake(&self) -> bool {
        if *self.is_closed_err.lock().unwrap() {
            if self.token.is_some() {
                // Select senders must NOT report success on closed channels
                return false;
            } else {
                *self.done_mutex.lock().unwrap() = true;
                self.done_cond.notify_one();
                return false;
            }
        }
        if let Some(ref tok) = self.token {
            let mut guard = tok.state.lock().unwrap();
            if guard.is_none() {
                *guard = Some(SelectWinner {
                    case_idx: self.case_idx,
                    value: None,
                });
                tok.cond.notify_one();
                true
            } else {
                false
            }
        } else {
            *self.done_mutex.lock().unwrap() = true;
            self.done_cond.notify_one();
            true
        }
    }
}

pub struct ReceiverWaiter {
    pub id: u64,
    pub token: Option<Arc<SelectToken>>,
    pub case_idx: usize,
    pub slot: Arc<Mutex<Option<Value>>>,
    pub done_cond: Arc<Condvar>,
    pub done_mutex: Arc<Mutex<bool>>,
}

impl ReceiverWaiter {
    pub fn is_active(&self) -> bool {
        if let Some(ref tok) = self.token {
            tok.state.lock().unwrap().is_none()
        } else {
            !*self.done_mutex.lock().unwrap()
        }
    }

    pub fn deliver(&self, val: Value) -> bool {
        if let Some(ref tok) = self.token {
            let mut guard = tok.state.lock().unwrap();
            if guard.is_none() {
                *guard = Some(SelectWinner {
                    case_idx: self.case_idx,
                    value: Some(val),
                });
                tok.cond.notify_one();
                true
            } else {
                false
            }
        } else {
            *self.slot.lock().unwrap() = Some(val);
            *self.done_mutex.lock().unwrap() = true;
            self.done_cond.notify_one();
            true
        }
    }
}

pub struct ChannelInner {
    pub queue: VecDeque<Value>,
    pub capacity: Option<usize>,
    pub closed: bool,
    pub elem_type: Option<TypeAnnotation>,
    pub send_waiters: VecDeque<SenderWaiter>,
    pub recv_waiters: VecDeque<ReceiverWaiter>,
}

impl ChannelInner {
    pub fn pop_active_recv_waiter(&mut self) -> Option<ReceiverWaiter> {
        while let Some(w) = self.recv_waiters.pop_front() {
            if w.is_active() {
                return Some(w);
            }
        }
        None
    }

    pub fn pop_active_send_waiter(&mut self) -> Option<SenderWaiter> {
        while let Some(w) = self.send_waiters.pop_front() {
            if w.is_active() {
                return Some(w);
            }
        }
        None
    }

    pub fn has_active_recv_waiter(&self) -> bool {
        self.recv_waiters.iter().any(|w| w.is_active())
    }

    pub fn has_active_send_waiter(&self) -> bool {
        self.send_waiters.iter().any(|w| w.is_active())
    }
}

pub struct Channel {
    pub inner: Mutex<ChannelInner>,
    pub not_empty: Condvar,
    pub not_full: Condvar,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock().unwrap();
        write!(
            f,
            "Channel {{ len: {}, capacity: {:?}, closed: {} }}",
            if inner.capacity == Some(0) { 0 } else { inner.queue.len() },
            inner.capacity,
            inner.closed
        )
    }
}

fn check_value_type(val: &Value, expected: &TypeAnnotation) -> bool {
    match (expected, val) {
        (TypeAnnotation::Any, _) => true,
        (TypeAnnotation::Int, Value::Int(_)) => true,
        (TypeAnnotation::Float, Value::Float(_)) => true,
        (TypeAnnotation::String, Value::String(_)) => true,
        (TypeAnnotation::Bool, Value::Bool(_)) => true,
        (TypeAnnotation::Null, Value::Null) => true,
        (TypeAnnotation::Array(_), Value::Array(_)) => true,
        (TypeAnnotation::Map { .. }, Value::Map(_)) => true,
        (TypeAnnotation::Channel(_), Value::Channel(_)) => true,
        _ => false,
    }
}

impl Channel {
    pub fn new(capacity: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ChannelInner {
                queue: VecDeque::new(),
                capacity,
                closed: false,
                elem_type: None,
                send_waiters: VecDeque::new(),
                recv_waiters: VecDeque::new(),
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        })
    }

    pub fn new_typed(capacity: Option<usize>, elem_type: Option<TypeAnnotation>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ChannelInner {
                queue: VecDeque::new(),
                capacity,
                closed: false,
                elem_type,
                send_waiters: VecDeque::new(),
                recv_waiters: VecDeque::new(),
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        })
    }

    pub fn element_type(&self) -> Option<TypeAnnotation> {
        self.inner.lock().unwrap().elem_type.clone()
    }

    pub fn send(&self, val: Value, span: Span) -> Result<(), RuntimeError> {
        let mut inner = self.inner.lock().unwrap();

        if inner.closed {
            return Err(RuntimeError::new("Cannot send on a closed channel".into()).at(span));
        }

        if let Some(ref expected) = inner.elem_type {
            if !check_value_type(&val, expected) {
                return Err(RuntimeError::new(format!(
                    "Type error: channel expects element type '{}', but got '{}'",
                    expected,
                    val.type_name()
                ))
                .at(span));
            }
        }

        // Unbuffered rendezvous channel
        if inner.capacity == Some(0) {
            while let Some(rw) = inner.pop_active_recv_waiter() {
                if rw.deliver(val.clone()) {
                    return Ok(());
                }
            }

            // No receiver available: block until a receiver arrives
            let id = next_waiter_id();
            let done_cond = Arc::new(Condvar::new());
            let done_mutex = Arc::new(Mutex::new(false));
            let is_closed_err = Arc::new(Mutex::new(false));

            inner.send_waiters.push_back(SenderWaiter {
                id,
                val,
                token: None,
                case_idx: 0,
                done_cond: done_cond.clone(),
                done_mutex: done_mutex.clone(),
                is_closed_err: is_closed_err.clone(),
            });

            drop(inner);

            let mut done = done_mutex.lock().unwrap();
            while !*done {
                done = done_cond.wait(done).unwrap();
            }

            if *is_closed_err.lock().unwrap() {
                return Err(RuntimeError::new("Cannot send on a closed channel".into()).at(span));
            }
            return Ok(());
        }

        // Buffered / unbounded channel
        loop {
            if inner.closed {
                return Err(RuntimeError::new("Cannot send on a closed channel".into()).at(span));
            }

            // Direct handover to waiting receiver if any
            while let Some(rw) = inner.pop_active_recv_waiter() {
                if rw.deliver(val.clone()) {
                    return Ok(());
                }
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

        // Unbuffered rendezvous channel
        if inner.capacity == Some(0) {
            while let Some(sw) = inner.pop_active_send_waiter() {
                let val = sw.val.clone();
                if sw.wake() {
                    return Ok(val);
                }
            }

            if inner.closed {
                return Ok(Value::Null);
            }

            if timeout_ms == Some(0) {
                return Ok(Value::Null);
            }

            let id = next_waiter_id();
            let slot = Arc::new(Mutex::new(None));
            let done_cond = Arc::new(Condvar::new());
            let done_mutex = Arc::new(Mutex::new(false));

            inner.recv_waiters.push_back(ReceiverWaiter {
                id,
                token: None,
                case_idx: 0,
                slot: slot.clone(),
                done_cond: done_cond.clone(),
                done_mutex: done_mutex.clone(),
            });

            drop(inner);

            let mut done = done_mutex.lock().unwrap();
            if let Some(timeout) = timeout_ms {
                let (new_done, timeout_result) = done_cond
                    .wait_timeout(done, Duration::from_millis(timeout))
                    .unwrap();
                done = new_done;
                if !*done && timeout_result.timed_out() {
                    let mut inner = self.inner.lock().unwrap();
                    inner.recv_waiters.retain(|w| w.id != id);
                    return Ok(Value::Null);
                }
            } else {
                while !*done {
                    done = done_cond.wait(done).unwrap();
                }
            }

            let res = slot.lock().unwrap().take().unwrap_or(Value::Null);
            return Ok(res);
        }

        // Buffered / unbounded channel
        loop {
            if let Some(val) = inner.queue.pop_front() {
                // Wake waiting sender if space freed
                while let Some(sw) = inner.pop_active_send_waiter() {
                    let s_val = sw.val.clone();
                    if sw.wake() {
                        inner.queue.push_back(s_val);
                        break;
                    }
                }
                self.not_full.notify_one();
                return Ok(val);
            }

            // If queue is empty, check if sender is waiting
            while let Some(sw) = inner.pop_active_send_waiter() {
                let val = sw.val.clone();
                if sw.wake() {
                    return Ok(val);
                }
            }

            if inner.closed {
                return Ok(Value::Null);
            }

            if timeout_ms == Some(0) {
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

    pub fn try_recv(&self, span: Span) -> Result<Value, RuntimeError> {
        self.recv(Some(0), span)
    }

    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;

        // Wake all waiting receivers with Null
        while let Some(rw) = inner.pop_active_recv_waiter() {
            rw.deliver(Value::Null);
        }

        // Wake all waiting direct senders with error
        while let Some(sw) = inner.pop_active_send_waiter() {
            *sw.is_closed_err.lock().unwrap() = true;
            sw.wake();
        }

        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        if inner.capacity == Some(0) {
            0
        } else {
            inner.queue.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        if inner.capacity == Some(0) {
            true
        } else {
            inner.queue.is_empty()
        }
    }

    pub fn capacity(&self) -> Option<usize> {
        self.inner.lock().unwrap().capacity
    }

    // -----------------------------------------------------------------------
    // Select Multiplexing Primitives
    // -----------------------------------------------------------------------

    pub fn poll_ready(&self, is_send: bool) -> bool {
        let inner = self.inner.lock().unwrap();
        if is_send {
            if inner.closed {
                return true; // will fail with closed error immediately
            }
            if inner.capacity == Some(0) {
                inner.has_active_recv_waiter()
            } else if let Some(cap) = inner.capacity {
                inner.queue.len() < cap || inner.has_active_recv_waiter()
            } else {
                true
            }
        } else {
            // recv
            if inner.closed {
                return true;
            }
            if inner.capacity == Some(0) {
                inner.has_active_send_waiter()
            } else {
                !inner.queue.is_empty() || inner.has_active_send_waiter()
            }
        }
    }

    pub fn try_select_recv(&self, token: &Arc<SelectToken>, case_idx: usize) -> Option<Value> {
        let mut inner = self.inner.lock().unwrap();

        // 1. Buffered elements
        if inner.capacity != Some(0) {
            if let Some(val) = inner.queue.pop_front() {
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    let mut skipped = Vec::new();
                    while let Some(sw) = inner.pop_active_send_waiter() {
                        if let Some(ref sw_tok) = sw.token {
                            if Arc::ptr_eq(sw_tok, token) {
                                skipped.push(sw);
                                continue;
                            }
                        }
                        let s_val = sw.val.clone();
                        if sw.wake() {
                            inner.queue.push_back(s_val);
                            break;
                        }
                    }
                    for s in skipped.into_iter().rev() {
                        inner.send_waiters.push_front(s);
                    }
                    self.not_full.notify_one();
                    *guard = Some(SelectWinner {
                        case_idx,
                        value: Some(val.clone()),
                    });
                    return Some(val);
                } else {
                    // Token already claimed elsewhere, put back
                    inner.queue.push_front(val);
                    return None;
                }
            }
        }

        // 2. Waiting sender handover
        let mut skipped = Vec::new();
        while let Some(sw) = inner.pop_active_send_waiter() {
            if let Some(ref sw_tok) = sw.token {
                if Arc::ptr_eq(sw_tok, token) {
                    skipped.push(sw);
                    continue;
                }
            }
            let mut guard = token.state.lock().unwrap();
            if guard.is_none() {
                let val = sw.val.clone();
                if sw.wake() {
                    for s in skipped.into_iter().rev() {
                        inner.send_waiters.push_front(s);
                    }
                    *guard = Some(SelectWinner {
                        case_idx,
                        value: Some(val.clone()),
                    });
                    return Some(val);
                }
            } else {
                inner.send_waiters.push_front(sw);
                for s in skipped.into_iter().rev() {
                    inner.send_waiters.push_front(s);
                }
                return None;
            }
        }
        for s in skipped.into_iter().rev() {
            inner.send_waiters.push_front(s);
        }

        // 3. Closed channel
        if inner.closed {
            let mut guard = token.state.lock().unwrap();
            if guard.is_none() {
                *guard = Some(SelectWinner {
                    case_idx,
                    value: Some(Value::Null),
                });
                return Some(Value::Null);
            } else {
                return None;
            }
        }

        None
    }

    pub fn try_select_send(
        &self,
        token: &Arc<SelectToken>,
        case_idx: usize,
        val: Value,
        span: Span,
    ) -> Result<bool, RuntimeError> {
        let mut inner = self.inner.lock().unwrap();

        if inner.closed {
            return Err(RuntimeError::new("Cannot send on a closed channel".into()).at(span));
        }

        if let Some(ref expected) = inner.elem_type {
            if !check_value_type(&val, expected) {
                return Err(RuntimeError::new(format!(
                    "Type error: channel expects element type '{}', but got '{}'",
                    expected,
                    val.type_name()
                ))
                .at(span));
            }
        }

        // 1. Direct handover to waiting receiver
        let mut skipped = Vec::new();
        while let Some(rw) = inner.pop_active_recv_waiter() {
            if let Some(ref rw_tok) = rw.token {
                if Arc::ptr_eq(rw_tok, token) {
                    skipped.push(rw);
                    continue;
                }
            }
            let mut guard = token.state.lock().unwrap();
            if guard.is_none() {
                if rw.deliver(val.clone()) {
                    for r in skipped.into_iter().rev() {
                        inner.recv_waiters.push_front(r);
                    }
                    *guard = Some(SelectWinner {
                        case_idx,
                        value: None,
                    });
                    return Ok(true);
                }
            } else {
                inner.recv_waiters.push_front(rw);
                for r in skipped.into_iter().rev() {
                    inner.recv_waiters.push_front(r);
                }
                return Ok(false);
            }
        }
        for r in skipped.into_iter().rev() {
            inner.recv_waiters.push_front(r);
        }

        // 2. Buffered room
        if inner.capacity != Some(0) {
            let has_space = match inner.capacity {
                Some(cap) => inner.queue.len() < cap,
                None => true,
            };

            if has_space {
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    inner.queue.push_back(val);
                    self.not_empty.notify_one();
                    *guard = Some(SelectWinner {
                        case_idx,
                        value: None,
                    });
                    return Ok(true);
                } else {
                    return Ok(false);
                }
            }
        }

        Ok(false)
    }

    pub fn register_recv_waiter(&self, token: Arc<SelectToken>, case_idx: usize) -> u64 {
        let mut inner = self.inner.lock().unwrap();

        // 1. Closed channel: immediately satisfy with Null
        if inner.closed {
            let mut guard = token.state.lock().unwrap();
            if guard.is_none() {
                *guard = Some(SelectWinner {
                    case_idx,
                    value: Some(Value::Null),
                });
                drop(guard);
                token.cond.notify_one();
            }
            return 0;
        }

        // 2. Unbuffered rendezvous: check if active sender is present
        if inner.capacity == Some(0) {
            let mut skipped = Vec::new();
            while let Some(sw) = inner.pop_active_send_waiter() {
                if let Some(ref sw_tok) = sw.token {
                    if Arc::ptr_eq(sw_tok, &token) {
                        skipped.push(sw);
                        continue;
                    }
                }
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    let val = sw.val.clone();
                    if sw.wake() {
                        for s in skipped.into_iter().rev() {
                            inner.send_waiters.push_front(s);
                        }
                        *guard = Some(SelectWinner {
                            case_idx,
                            value: Some(val),
                        });
                        drop(guard);
                        token.cond.notify_one();
                        return 0;
                    }
                } else {
                    inner.send_waiters.push_front(sw);
                    for s in skipped.into_iter().rev() {
                        inner.send_waiters.push_front(s);
                    }
                    return 0;
                }
            }
            for s in skipped.into_iter().rev() {
                inner.send_waiters.push_front(s);
            }
        } else {
            // 3. Buffered channel: check queue or waiting senders
            if let Some(val) = inner.queue.pop_front() {
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    let mut skipped = Vec::new();
                    while let Some(sw) = inner.pop_active_send_waiter() {
                        if let Some(ref sw_tok) = sw.token {
                            if Arc::ptr_eq(sw_tok, &token) {
                                skipped.push(sw);
                                continue;
                            }
                        }
                        let s_val = sw.val.clone();
                        if sw.wake() {
                            inner.queue.push_back(s_val);
                            break;
                        }
                    }
                    for s in skipped.into_iter().rev() {
                        inner.send_waiters.push_front(s);
                    }
                    self.not_full.notify_one();
                    *guard = Some(SelectWinner {
                        case_idx,
                        value: Some(val),
                    });
                    drop(guard);
                    token.cond.notify_one();
                    return 0;
                } else {
                    inner.queue.push_front(val);
                    return 0;
                }
            }

            // Queue empty, check if sender is waiting
            let mut skipped = Vec::new();
            while let Some(sw) = inner.pop_active_send_waiter() {
                if let Some(ref sw_tok) = sw.token {
                    if Arc::ptr_eq(sw_tok, &token) {
                        skipped.push(sw);
                        continue;
                    }
                }
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    let val = sw.val.clone();
                    if sw.wake() {
                        for s in skipped.into_iter().rev() {
                            inner.send_waiters.push_front(s);
                        }
                        *guard = Some(SelectWinner {
                            case_idx,
                            value: Some(val),
                        });
                        drop(guard);
                        token.cond.notify_one();
                        return 0;
                    }
                } else {
                    inner.send_waiters.push_front(sw);
                    for s in skipped.into_iter().rev() {
                        inner.send_waiters.push_front(s);
                    }
                    return 0;
                }
            }
            for s in skipped.into_iter().rev() {
                inner.send_waiters.push_front(s);
            }
        }

        // 4. No ready counterpart: append waiter
        let id = next_waiter_id();
        inner.recv_waiters.push_back(ReceiverWaiter {
            id,
            token: Some(token),
            case_idx,
            slot: Arc::new(Mutex::new(None)),
            done_cond: Arc::new(Condvar::new()),
            done_mutex: Arc::new(Mutex::new(false)),
        });
        id
    }

    pub fn register_send_waiter(
        &self,
        token: Arc<SelectToken>,
        case_idx: usize,
        val: Value,
        span: Span,
    ) -> Result<u64, RuntimeError> {
        let mut inner = self.inner.lock().unwrap();

        // 1. Type validation
        if let Some(ref expected) = inner.elem_type {
            if !check_value_type(&val, expected) {
                return Err(RuntimeError::new(format!(
                    "Type error: channel expects element type '{}', but got '{}'",
                    expected,
                    val.type_name()
                ))
                .at(span));
            }
        }

        // 2. Closed channel: cannot send on closed channel!
        if inner.closed {
            return Err(RuntimeError::new("Cannot send on a closed channel".into()).at(span));
        }

        // 3. Unbuffered rendezvous: direct handover to waiting receiver
        if inner.capacity == Some(0) {
            let mut skipped = Vec::new();
            while let Some(rw) = inner.pop_active_recv_waiter() {
                if let Some(ref rw_tok) = rw.token {
                    if Arc::ptr_eq(rw_tok, &token) {
                        skipped.push(rw);
                        continue;
                    }
                }
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    if rw.deliver(val.clone()) {
                        for r in skipped.into_iter().rev() {
                            inner.recv_waiters.push_front(r);
                        }
                        *guard = Some(SelectWinner {
                            case_idx,
                            value: None,
                        });
                        drop(guard);
                        token.cond.notify_one();
                        return Ok(0);
                    }
                } else {
                    inner.recv_waiters.push_front(rw);
                    for r in skipped.into_iter().rev() {
                        inner.recv_waiters.push_front(r);
                    }
                    return Ok(0);
                }
            }
            for r in skipped.into_iter().rev() {
                inner.recv_waiters.push_front(r);
            }
        } else {
            // 4. Buffered channel
            let has_space = match inner.capacity {
                Some(cap) => inner.queue.len() < cap,
                None => true,
            };

            // First check if receiver is waiting
            let mut skipped = Vec::new();
            while let Some(rw) = inner.pop_active_recv_waiter() {
                if let Some(ref rw_tok) = rw.token {
                    if Arc::ptr_eq(rw_tok, &token) {
                        skipped.push(rw);
                        continue;
                    }
                }
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    if rw.deliver(val.clone()) {
                        for r in skipped.into_iter().rev() {
                            inner.recv_waiters.push_front(r);
                        }
                        *guard = Some(SelectWinner {
                            case_idx,
                            value: None,
                        });
                        drop(guard);
                        token.cond.notify_one();
                        return Ok(0);
                    }
                } else {
                    inner.recv_waiters.push_front(rw);
                    for r in skipped.into_iter().rev() {
                        inner.recv_waiters.push_front(r);
                    }
                    return Ok(0);
                }
            }
            for r in skipped.into_iter().rev() {
                inner.recv_waiters.push_front(r);
            }

            if has_space {
                let mut guard = token.state.lock().unwrap();
                if guard.is_none() {
                    inner.queue.push_back(val.clone());
                    self.not_empty.notify_one();
                    *guard = Some(SelectWinner {
                        case_idx,
                        value: None,
                    });
                    drop(guard);
                    token.cond.notify_one();
                    return Ok(0);
                } else {
                    return Ok(0);
                }
            }
        }

        // 5. No receiver available and no buffer space: register as waiting sender
        let id = next_waiter_id();
        inner.send_waiters.push_back(SenderWaiter {
            id,
            val,
            token: Some(token),
            case_idx,
            done_cond: Arc::new(Condvar::new()),
            done_mutex: Arc::new(Mutex::new(false)),
            is_closed_err: Arc::new(Mutex::new(false)),
        });
        Ok(id)
    }

    pub fn remove_waiter(&self, id: u64) {
        if id == 0 {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.send_waiters.retain(|w| w.id != id);
        inner.recv_waiters.retain(|w| w.id != id);
    }
}

// ---------------------------------------------------------------------------
// select_run Implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum SelectOp {
    Recv { channel: Arc<Channel> },
    Send { channel: Arc<Channel>, value: Value },
}

pub enum SelectOutcome {
    Success {
        case_idx: usize,
        received_value: Option<Value>,
    },
    Default,
}

pub fn select_run(
    cases: &[SelectOp],
    has_default: bool,
    span: Span,
) -> Result<SelectOutcome, RuntimeError> {
    if cases.is_empty() {
        if has_default {
            return Ok(SelectOutcome::Default);
        }
        // Empty select with no default blocks forever
        let c = Condvar::new();
        let m = Mutex::new(());
        let _unused = c.wait(m.lock().unwrap()).unwrap();
        return Ok(SelectOutcome::Default);
    }

    // 1. Fast Path: Poll all cases
    let mut ready_indices = Vec::new();
    for (idx, op) in cases.iter().enumerate() {
        match op {
            SelectOp::Recv { channel } => {
                if channel.poll_ready(false) {
                    ready_indices.push(idx);
                }
            }
            SelectOp::Send { channel, .. } => {
                if channel.poll_ready(true) {
                    ready_indices.push(idx);
                }
            }
        }
    }

    if !ready_indices.is_empty() {
        // Uniform fair random shuffle among ready candidates
        fastrand::shuffle(&mut ready_indices);
        let token = Arc::new(SelectToken::new());

        for chosen in ready_indices {
            match &cases[chosen] {
                SelectOp::Recv { channel } => {
                    if let Some(val) = channel.try_select_recv(&token, chosen) {
                        return Ok(SelectOutcome::Success {
                            case_idx: chosen,
                            received_value: Some(val),
                        });
                    }
                }
                SelectOp::Send { channel, value } => {
                    if channel.try_select_send(&token, chosen, value.clone(), span)? {
                        return Ok(SelectOutcome::Success {
                            case_idx: chosen,
                            received_value: None,
                        });
                    }
                }
            }
        }
    }

    // 2. Default Path: If nothing was ready and default branch exists
    if has_default {
        return Ok(SelectOutcome::Default);
    }

    // 3. Slow Path: Register waiters on all channels and block
    let token = Arc::new(SelectToken::new());
    let mut registrations: Vec<(Arc<Channel>, u64)> = Vec::with_capacity(cases.len());

    for (idx, op) in cases.iter().enumerate() {
        if token.state.lock().unwrap().is_some() {
            break;
        }
        match op {
            SelectOp::Recv { channel } => {
                let id = channel.register_recv_waiter(token.clone(), idx);
                if id != 0 {
                    registrations.push((channel.clone(), id));
                }
            }
            SelectOp::Send { channel, value } => {
                match channel.register_send_waiter(token.clone(), idx, value.clone(), span) {
                    Ok(id) => {
                        if id != 0 {
                            registrations.push((channel.clone(), id));
                        }
                    }
                    Err(err) => {
                        for (ch, wid) in registrations {
                            ch.remove_waiter(wid);
                        }
                        return Err(err);
                    }
                }
            }
        }
    }

    // Check if one of the registered channels completed immediately
    let winner = {
        let mut guard = token.state.lock().unwrap();
        while guard.is_none() {
            guard = token.cond.wait(guard).unwrap();
        }
        guard.take().unwrap()
    };

    // Clean up registered waiters across all participating channels
    for (channel, id) in registrations {
        channel.remove_waiter(id);
    }

    Ok(SelectOutcome::Success {
        case_idx: winner.case_idx,
        received_value: winner.value,
    })
}

//! Actor-Based Concurrency & Distributed Mailboxes (`src/actor.rs`)

use crate::ast::Span;
use crate::channel::Channel;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use thiserror::Error;

static NEXT_ACTOR_SEQ: AtomicU64 = AtomicU64::new(1);

/// Generates a unique actor ID.
pub fn generate_actor_id(custom_name: Option<&str>) -> String {
    match custom_name {
        Some(name) => name.to_string(),
        None => format!("actor-{}", NEXT_ACTOR_SEQ.fetch_add(1, Ordering::Relaxed)),
    }
}

/// Strongly-typed error enum for actor operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ActorError {
    #[error("Actor is not alive")]
    ActorNotAlive,

    #[error("Actor has stopped")]
    ActorStopped,

    #[error("Actor failed with error: {0}")]
    ActorFailed(String),

    #[error("Actor mailbox is closed")]
    MailboxClosed,

    #[error("Request timed out")]
    Timeout,

    #[error("Actor name already registered: {0}")]
    NameAlreadyRegistered(String),

    #[error("Actor name not found: {0}")]
    NameNotFound(String),

    #[error("Invalid actor name: {0}")]
    InvalidName(String),

    #[error("Runtime evaluation error: {0}")]
    EvaluationError(String),

    #[error("Actor thread spawn failed for '{0}': {1}")]
    SpawnError(String, String),
}

impl ActorError {
    pub fn to_runtime_error(&self, span: Span) -> RuntimeError {
        RuntimeError::new(self.to_string()).at(span)
    }
}

/// Lifecycle status of an actor process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorStatus {
    Running,
    Stopped,
    Failed(String),
}

impl ActorStatus {
    pub fn as_str(&self) -> &str {
        match self {
            ActorStatus::Running => "running",
            ActorStatus::Stopped => "stopped",
            ActorStatus::Failed(_) => "failed",
        }
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        matches!(self, ActorStatus::Running)
    }

    #[inline]
    pub fn is_stopped(&self) -> bool {
        matches!(self, ActorStatus::Stopped)
    }

    #[inline]
    pub fn is_failed(&self) -> bool {
        matches!(self, ActorStatus::Failed(_))
    }

    pub fn failure_reason(&self) -> Option<&str> {
        match self {
            ActorStatus::Failed(r) => Some(r.as_str()),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        Value::String(self.as_str().to_string())
    }
}

/// Message envelopes queued in an actor's mailbox.
#[derive(Debug)]
pub enum ActorMessage {
    Normal(Value),
    Ask {
        payload: Value,
        reply_sender: Arc<Channel>,
    },
    Stop,
}

/// Unbounded FIFO concurrent queue with Condvar parking (0% idle CPU).
pub struct Mailbox {
    queue: Mutex<VecDeque<ActorMessage>>,
    not_empty: Condvar,
}

impl Default for Mailbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Mailbox {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            not_empty: Condvar::new(),
        }
    }

    /// Pushes a message onto the tail of the mailbox queue (O(1)).
    /// Wakes the parked worker thread immediately via `not_empty.notify_one()`.
    pub fn push(&self, msg: ActorMessage) {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        q.push_back(msg);
        self.not_empty.notify_one();
    }

    /// Pushes an urgent control message to the front of the queue.
    pub fn push_front(&self, msg: ActorMessage) {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        q.push_front(msg);
        self.not_empty.notify_one();
    }

    /// Blocking dequeue called by the worker thread.
    /// If the queue is empty, puts the calling thread to sleep via `Condvar::wait()`.
    pub fn pop(&self) -> ActorMessage {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        loop {
            if let Some(msg) = q.pop_front() {
                return msg;
            }
            q = match self.not_empty.wait(q) {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
        }
    }

    /// Dequeue with timeout. Returns None if timeout expires before a message arrives.
    pub fn pop_timeout(&self, timeout: Duration) -> Option<ActorMessage> {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(msg) = q.pop_front() {
                return Some(msg);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            let (new_q, timeout_result) = match self.not_empty.wait_timeout(q, remaining) {
                Ok(pair) => pair,
                Err(p) => p.into_inner(),
            };
            q = new_q;
            if timeout_result.timed_out() && q.is_empty() {
                return None;
            }
        }
    }

    /// Non-blocking pop. Returns None immediately if empty.
    pub fn try_pop(&self) -> Option<ActorMessage> {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        q.pop_front()
    }

    /// Returns the number of messages currently pending in the mailbox.
    pub fn len(&self) -> usize {
        match self.queue.lock() {
            Ok(g) => g.len(),
            Err(p) => p.into_inner().len(),
        }
    }

    /// Returns true if the mailbox has no pending messages.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears and discards all pending messages, returning the count of discarded messages.
    pub fn clear(&self) -> usize {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut count = 0;
        q.retain(|msg| {
            if matches!(msg, ActorMessage::Stop) {
                true
            } else {
                count += 1;
                false
            }
        });
        count
    }
}

/// Internal thread-shared state for an individual actor.
pub struct ActorInner {
    pub id: String,
    pub mailbox: Mailbox,
    pub status: RwLock<ActorStatus>,
    pub thread_handle: Mutex<Option<JoinHandle<()>>>,
    pub exit_listeners: Mutex<Vec<Arc<dyn Fn(&str, &ActorStatus) + Send + Sync>>>,
}

/// Public lightweight handle to an actor.
#[derive(Clone)]
pub struct ActorRef {
    pub(crate) inner: Arc<ActorInner>,
}

impl ActorRef {
    pub fn new(inner: Arc<ActorInner>) -> Self {
        Self { inner }
    }

    #[inline]
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    #[inline]
    pub fn status(&self) -> ActorStatus {
        match self.inner.status.read() {
            Ok(s) => s.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.status().is_alive()
    }

    #[inline]
    pub fn mailbox_len(&self) -> usize {
        self.inner.mailbox.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.mailbox.is_empty()
    }

    /// Signals the actor to terminate gracefully.
    /// Returns true if the status transitioned from Running to Stopped.
    pub fn stop(&self) -> bool {
        let mut status = match self.inner.status.write() {
            Ok(s) => s,
            Err(p) => p.into_inner(),
        };
        let transitioned = *status == ActorStatus::Running;
        *status = ActorStatus::Stopped;

        let has_thread = match self.inner.thread_handle.lock() {
            Ok(g) => g.is_some(),
            Err(p) => p.into_inner().is_some(),
        };
        if has_thread {
            self.inner.mailbox.push(ActorMessage::Stop);
        }
        transitioned
    }

    /// Waits for the actor thread to cleanly exit.
    pub fn join(&self) -> Result<(), ActorError> {
        let handle = {
            let mut guard = match self.inner.thread_handle.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.take()
        };
        if let Some(h) = handle {
            self.inner.mailbox.push(ActorMessage::Stop);
            h.join().map_err(|_| {
                ActorError::EvaluationError("Actor thread panicked during execution".into())
            })?;
        }
        Ok(())
    }

    /// Helper that issues a stop signal and immediately waits for thread termination.
    pub fn stop_and_join(&self) -> Result<bool, ActorError> {
        let transitioned = self.stop();
        self.join()?;
        Ok(transitioned)
    }

    /// Sends a message asynchronously to the actor's mailbox.
    /// The message is deep-cloned to enforce 100% memory isolation.
    pub fn send(&self, msg: Value) -> Result<(), ActorError> {
        {
            let status = self.status();
            match status {
                ActorStatus::Running => {}
                ActorStatus::Stopped => return Err(ActorError::ActorStopped),
                ActorStatus::Failed(err) => return Err(ActorError::ActorFailed(err)),
            }
        }

        let isolated = msg.deep_clone();
        self.inner.mailbox.push(ActorMessage::Normal(isolated));
        Ok(())
    }

    /// Synchronously sends a message and awaits the actor's response via an ephemeral channel.
    /// Default timeout is 5000 milliseconds if timeout is None.
    pub fn ask(&self, msg: Value, timeout: Option<Duration>) -> Result<Value, ActorError> {
        if let Some(cur) = current_actor() {
            if cur.id() == self.inner.id {
                return Err(ActorError::EvaluationError(
                    "Deadlock detected: actor cannot ask() itself synchronously".into(),
                ));
            }
        }

        {
            let status = self.status();
            match status {
                ActorStatus::Running => {}
                ActorStatus::Stopped => return Err(ActorError::ActorStopped),
                ActorStatus::Failed(err) => return Err(ActorError::ActorFailed(err)),
            }
        }

        let reply_channel = Channel::new(Some(1));
        let payload = msg.deep_clone();

        self.inner.mailbox.push(ActorMessage::Ask {
            payload,
            reply_sender: Arc::clone(&reply_channel),
        });

        let duration = timeout.unwrap_or(Duration::from_millis(5000));
        let timeout_ms = duration.as_millis() as u64;
        let span = Span::new(1, 1);

        let raw_result = reply_channel.recv(Some(timeout_ms), span);

        match raw_result {
            Ok(Value::Array(arr)) => {
                let items = match arr.read() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if items.len() == 2 {
                    if let Value::String(ref tag) = items[0] {
                        if tag == "ok" {
                            return Ok(items[1].clone());
                        } else if tag == "err" {
                            let err_str = match &items[1] {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            if err_str == "Actor stopped" {
                                return Err(ActorError::ActorStopped);
                            }
                            return Err(ActorError::ActorFailed(err_str));
                        }
                    }
                }
                Ok(Value::Array(Arc::clone(&arr)))
            }
            Ok(Value::Null) => {
                let status = self.status();
                match status {
                    ActorStatus::Failed(err) => Err(ActorError::ActorFailed(err)),
                    ActorStatus::Stopped => Err(ActorError::ActorStopped),
                    ActorStatus::Running => Err(ActorError::Timeout),
                }
            }
            Ok(other) => Ok(other),
            Err(e) => Err(ActorError::EvaluationError(e.message)),
        }
    }

    /// Registers an exit listener callback invoked when this actor terminates.
    /// If the actor has already terminated, the callback is dispatched immediately.
    pub fn add_exit_listener<F>(&self, f: F)
    where
        F: Fn(&str, &ActorStatus) + Send + Sync + 'static,
    {
        let mut listeners = match self.inner.exit_listeners.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let status = self.status();
        if !status.is_alive() {
            drop(listeners);
            f(&self.inner.id, &status);
            return;
        }
        listeners.push(Arc::new(f));
    }
}

impl fmt::Debug for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ActorRef({})", self.inner.id)
    }
}

impl fmt::Display for ActorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<actor:{}>", self.inner.id)
    }
}

impl PartialEq for ActorRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || self.inner.id == other.inner.id
    }
}

impl Eq for ActorRef {}

impl Hash for ActorRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.id.hash(state);
    }
}

thread_local! {
    static CURRENT_ACTOR: std::cell::RefCell<Option<Arc<ActorRef>>> = const { std::cell::RefCell::new(None) };
}

/// Retrieves the ActorRef of the currently executing actor thread, if any.
pub fn current_actor() -> Option<Arc<ActorRef>> {
    CURRENT_ACTOR.with(|cell| cell.borrow().clone())
}

/// Alias for `current_actor()`.
pub fn self_pid() -> Option<Arc<ActorRef>> {
    current_actor()
}

fn cleanup_thread_local() {
    CURRENT_ACTOR.with(|cell| *cell.borrow_mut() = None);
}

struct ActorScopeGuard {
    inner: Arc<ActorInner>,
}

impl Drop for ActorScopeGuard {
    fn drop(&mut self) {
        cleanup_thread_local();
        let mut status = match self.inner.status.read() {
            Ok(s) => s.clone(),
            Err(p) => p.into_inner().clone(),
        };
        if std::thread::panicking() && status.is_alive() {
            status = ActorStatus::Failed("Actor thread panicked unexpectedly".to_string());
            if let Ok(mut st) = self.inner.status.write() {
                *st = status.clone();
            }
        }
        let status_desc = match &status {
            ActorStatus::Failed(err) => err.clone(),
            _ => "Actor stopped".to_string(),
        };
        let span = Span::new(1, 1);
        while let Some(msg) = self.inner.mailbox.try_pop() {
            if let ActorMessage::Ask { reply_sender, .. } = msg {
                let err_envelope = Value::Array(Arc::new(RwLock::new(vec![
                    Value::String("err".into()),
                    Value::String(status_desc.clone()),
                ])));
                let _ = reply_sender.send(err_envelope, span);
            }
        }

        let listeners = match self.inner.exit_listeners.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        };
        for listener in listeners {
            listener(&self.inner.id, &status);
        }
    }
}

/// Internal handler representation across native Rust closures and Shae callables.
pub enum HandlerKind {
    NativeStateless(Box<dyn Fn(Value) -> Result<Value, String> + Send + 'static>),
    NativeStateful(Box<dyn Fn(Value, Value) -> Result<Value, String> + Send + 'static>),
    Shae {
        evaluator: Evaluator,
        handler: Value,
    },
}

impl ActorRef {
    pub(crate) fn spawn_internal(
        id: String,
        custom_name: Option<&str>,
        initial_state: Option<Value>,
        mut handler_kind: HandlerKind,
    ) -> Result<Arc<ActorRef>, ActorError> {
        let inner = Arc::new(ActorInner {
            id: id.clone(),
            mailbox: Mailbox::new(),
            status: RwLock::new(ActorStatus::Running),
            thread_handle: Mutex::new(None),
            exit_listeners: Mutex::new(Vec::new()),
        });

        let actor_ref = Arc::new(ActorRef::new(Arc::clone(&inner)));
        let thread_actor_ref = Arc::clone(&actor_ref);
        let thread_inner = Arc::clone(&inner);
        let thread_name = format!("shae-actor-{}", id);

        let handle = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                CURRENT_ACTOR.with(|cell| {
                    *cell.borrow_mut() = Some(thread_actor_ref);
                });
                let _guard = ActorScopeGuard {
                    inner: Arc::clone(&thread_inner),
                };

                let mut current_state = initial_state;
                let span = Span::new(1, 1);

                loop {
                    let msg = thread_inner.mailbox.pop();

                    if matches!(msg, ActorMessage::Stop) {
                        let mut status = match thread_inner.status.write() {
                            Ok(s) => s,
                            Err(p) => p.into_inner(),
                        };
                        *status = ActorStatus::Stopped;
                        break;
                    }

                    let (is_ask, payload, reply_sender) = match msg {
                        ActorMessage::Normal(val) => (false, val, None),
                        ActorMessage::Ask {
                            payload,
                            reply_sender,
                        } => (true, payload, Some(reply_sender)),
                        ActorMessage::Stop => unreachable!(),
                    };

                    let eval_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            match &mut handler_kind {
                                HandlerKind::NativeStateless(f) => f(payload),
                                HandlerKind::NativeStateful(f) => {
                                    let st = current_state.take().unwrap_or(Value::Null);
                                    let res = f(st, payload)?;
                                    if is_ask {
                                        if let Value::Map(ref m) = res {
                                            let guard = match m.read() {
                                                Ok(g) => g,
                                                Err(p) => p.into_inner(),
                                            };
                                            if guard.contains_key("state")
                                                && guard.contains_key("reply")
                                            {
                                                let actual_state =
                                                    guard.get("state").unwrap().clone();
                                                let reply = guard.get("reply").unwrap().clone();
                                                current_state = Some(actual_state);
                                                return Ok(reply);
                                            }
                                        }
                                    }
                                    current_state = Some(res.clone());
                                    Ok(res)
                                }
                                HandlerKind::Shae { evaluator, handler } => {
                                    match &mut current_state {
                                        None => evaluator
                                            .call_value(handler, vec![payload], span)
                                            .map_err(|e| e.to_string()),
                                        Some(st) => {
                                            let args = vec![st.clone(), payload];
                                            let step_result = evaluator
                                                .call_value(handler, args, span)
                                                .map_err(|e| e.to_string())?;
                                            if is_ask {
                                                if let Value::Map(ref m) = step_result {
                                                    let guard = match m.read() {
                                                        Ok(g) => g,
                                                        Err(p) => p.into_inner(),
                                                    };
                                                    if guard.contains_key("state")
                                                        && guard.contains_key("reply")
                                                    {
                                                        let actual_state =
                                                            guard.get("state").unwrap().clone();
                                                        let reply =
                                                            guard.get("reply").unwrap().clone();
                                                        *st = actual_state;
                                                        return Ok(reply);
                                                    }
                                                }
                                            }
                                            *st = step_result.clone();
                                            Ok(step_result)
                                        }
                                    }
                                }
                            }
                        }));

                    match eval_result {
                        Ok(Ok(return_val)) => {
                            if let Some(reply_tx) = reply_sender {
                                let envelope = Value::Array(Arc::new(RwLock::new(vec![
                                    Value::String("ok".into()),
                                    return_val.deep_clone(),
                                ])));
                                let _ = reply_tx.send(envelope, span);
                            }
                        }
                        Ok(Err(err_msg)) => {
                            {
                                let mut status = match thread_inner.status.write() {
                                    Ok(s) => s,
                                    Err(p) => p.into_inner(),
                                };
                                *status = ActorStatus::Failed(err_msg.clone());
                            }
                            if let Some(reply_tx) = reply_sender {
                                let err_envelope = Value::Array(Arc::new(RwLock::new(vec![
                                    Value::String("err".into()),
                                    Value::String(err_msg),
                                ])));
                                let _ = reply_tx.send(err_envelope, span);
                            }
                            return;
                        }
                        Err(panic_payload) => {
                            let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                                s.to_string()
                            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                                s.clone()
                            } else {
                                "Actor panicked with unformatted payload".to_string()
                            };
                            {
                                let mut status = match thread_inner.status.write() {
                                    Ok(s) => s,
                                    Err(p) => p.into_inner(),
                                };
                                *status = ActorStatus::Failed(panic_msg.clone());
                            }
                            if let Some(reply_tx) = reply_sender {
                                let err_envelope = Value::Array(Arc::new(RwLock::new(vec![
                                    Value::String("err".into()),
                                    Value::String(panic_msg),
                                ])));
                                let _ = reply_tx.send(err_envelope, span);
                            }
                            return;
                        }
                    }
                }
            })
            .map_err(|e| ActorError::SpawnError(id.clone(), e.to_string()))?;

        *inner.thread_handle.lock().unwrap() = Some(handle);

        if let Some(name) = custom_name {
            let _ = ActorRegistry::register(name, Arc::clone(&actor_ref));
        }

        Ok(actor_ref)
    }

    /// Spawns a stateless actor using a Rust closure `Fn(Value) -> Result<Value, String>`.
    pub fn spawn_fn<F>(f: F) -> Arc<ActorRef>
    where
        F: Fn(Value) -> Result<Value, String> + Send + 'static,
    {
        let id = generate_actor_id(None);
        Self::spawn_internal(id, None, None, HandlerKind::NativeStateless(Box::new(f)))
            .expect("Failed to spawn native actor")
    }

    /// Spawns a stateful actor using a Rust closure `Fn(Value, Value) -> Result<Value, String>`.
    pub fn spawn_stateful_fn<F>(initial_state: Value, f: F) -> Arc<ActorRef>
    where
        F: Fn(Value, Value) -> Result<Value, String> + Send + 'static,
    {
        let id = generate_actor_id(None);
        Self::spawn_internal(
            id,
            None,
            Some(initial_state),
            HandlerKind::NativeStateful(Box::new(f)),
        )
        .expect("Failed to spawn stateful actor")
    }

    /// Spawns a stateless actor running a Shae callable `Value`.
    pub fn spawn(handler: Value) -> Result<Arc<ActorRef>, ActorError> {
        let id = generate_actor_id(None);
        let evaluator = Evaluator::new();
        Self::spawn_internal(id, None, None, HandlerKind::Shae { evaluator, handler })
    }

    /// Spawns a stateful actor running a Shae callable `Value`.
    pub fn spawn_stateful(
        initial_state: Value,
        handler: Value,
    ) -> Result<Arc<ActorRef>, ActorError> {
        let id = generate_actor_id(None);
        let evaluator = Evaluator::new();
        Self::spawn_internal(
            id,
            None,
            Some(initial_state),
            HandlerKind::Shae { evaluator, handler },
        )
    }

    /// Spawns an actor sharing environment and module cache from a caller Evaluator.
    pub fn spawn_with_eval(
        eval: &Evaluator,
        custom_name: Option<&str>,
        initial_state: Option<Value>,
        handler: Value,
    ) -> Result<Arc<ActorRef>, ActorError> {
        let id = generate_actor_id(custom_name);
        let worker_eval = Evaluator::with_env_cache_file(
            eval.global_env.clone(),
            eval.module_cache.clone(),
            eval.current_file.clone(),
        );
        Self::spawn_internal(
            id,
            custom_name,
            initial_state,
            HandlerKind::Shae {
                evaluator: worker_eval,
                handler,
            },
        )
    }

    /// Spawns an actor and binds it immediately in the global ActorRegistry under `name`.
    pub fn spawn_named(
        name: &str,
        initial_state: Option<Value>,
        handler: Value,
    ) -> Result<Arc<ActorRef>, ActorError> {
        let evaluator = Evaluator::new();
        let actor = Self::spawn_internal(
            name.to_string(),
            None,
            initial_state,
            HandlerKind::Shae { evaluator, handler },
        )?;
        if let Err(e) = ActorRegistry::register(name, Arc::clone(&actor)) {
            actor.stop();
            let _ = actor.join();
            return Err(e);
        }
        Ok(actor)
    }
}

/// Spawns a simple stateless actor executing a native Rust closure that accepts `Value`.
pub fn spawn_native(f: Arc<dyn Fn(Value) + Send + Sync + 'static>) -> Arc<ActorRef> {
    let id = generate_actor_id(None);
    let closure = move |msg: Value| {
        f(msg);
        Ok(Value::Null)
    };
    ActorRef::spawn_internal(
        id,
        None,
        None,
        HandlerKind::NativeStateless(Box::new(closure)),
    )
    .expect("Failed to spawn native actor")
}

/// Lifecycle status of a supervisor process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorStatus {
    Running,
    Stopped,
    Failed(String),
}

impl SupervisorStatus {
    #[inline]
    pub fn is_alive(&self) -> bool {
        matches!(self, SupervisorStatus::Running)
    }

    #[inline]
    pub fn is_stopped(&self) -> bool {
        matches!(self, SupervisorStatus::Stopped)
    }

    #[inline]
    pub fn is_failed(&self) -> bool {
        matches!(self, SupervisorStatus::Failed(_))
    }

    pub fn failure_reason(&self) -> Option<&str> {
        match self {
            SupervisorStatus::Failed(r) => Some(r.as_str()),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            SupervisorStatus::Running => "running",
            SupervisorStatus::Stopped => "stopped",
            SupervisorStatus::Failed(_) => "failed",
        }
    }

    pub fn to_value(&self) -> Value {
        Value::String(self.as_str().to_string())
    }
}

impl fmt::Display for SupervisorStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Supervision restart strategy dictating how sibling processes are handled when a child crashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartStrategy {
    #[default]
    OneForOne,
    AllForOne,
}

impl RestartStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            RestartStrategy::OneForOne => "one_for_one",
            RestartStrategy::AllForOne => "all_for_one",
        }
    }

    pub fn from_str_loose(s: &str) -> Result<Self, SupervisorError> {
        let normalized = s.trim().to_lowercase().replace('-', "_");
        match normalized.as_str() {
            "one_for_one" | "oneforone" => Ok(RestartStrategy::OneForOne),
            "all_for_one" | "allforone" => Ok(RestartStrategy::AllForOne),
            other => Err(SupervisorError::InvalidConfig(format!(
                "Unknown restart strategy '{}'; expected 'one_for_one' or 'all_for_one'",
                other
            ))),
        }
    }
}

impl fmt::Display for RestartStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Child actor restart policy within a supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartPolicy {
    #[default]
    Permanent,
    Transient,
    Temporary,
}

impl RestartPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            RestartPolicy::Permanent => "permanent",
            RestartPolicy::Transient => "transient",
            RestartPolicy::Temporary => "temporary",
        }
    }

    pub fn from_str_loose(s: &str) -> Result<Self, SupervisorError> {
        let normalized = s.trim().to_lowercase().replace('-', "_");
        match normalized.as_str() {
            "permanent" => Ok(RestartPolicy::Permanent),
            "transient" => Ok(RestartPolicy::Transient),
            "temporary" => Ok(RestartPolicy::Temporary),
            other => Err(SupervisorError::InvalidConfig(format!(
                "Unknown restart policy '{}'; expected 'permanent', 'transient', or 'temporary'",
                other
            ))),
        }
    }
}

impl fmt::Display for RestartPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Strongly-typed error enum for supervisor operations.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SupervisorError {
    #[error("Supervisor is not alive")]
    SupervisorNotAlive,

    #[error("Supervisor has stopped")]
    SupervisorStopped,

    #[error("Supervisor failed: {0}")]
    SupervisorFailed(String),

    #[error("Child already exists with id: {0}")]
    ChildAlreadyExists(String),

    #[error("Child not found with id: {0}")]
    ChildNotFound(String),

    #[error("Failed to start child '{0}': {1}")]
    ChildStartError(String, String),

    #[error("Restart rate limit exceeded ({max_restarts} restarts within {within_seconds:.1}s)")]
    RestartRateLimitExceeded {
        max_restarts: usize,
        within_seconds: f64,
    },

    #[error("Invalid supervisor configuration: {0}")]
    InvalidConfig(String),
}

impl SupervisorError {
    pub fn to_runtime_error(&self, span: Span) -> RuntimeError {
        RuntimeError::new(self.to_string()).at(span)
    }
}

/// Configuration options for a supervisor tree node.
#[derive(Debug, Clone, PartialEq)]
pub struct SupervisorConfig {
    pub strategy: RestartStrategy,
    pub max_restarts: usize,
    pub within_seconds: f64,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 3,
            within_seconds: 5.0,
        }
    }
}

impl SupervisorConfig {
    pub fn new(strategy: RestartStrategy, max_restarts: usize, within_seconds: f64) -> Self {
        Self {
            strategy,
            max_restarts,
            within_seconds: if within_seconds <= 0.0 {
                5.0
            } else {
                within_seconds
            },
        }
    }

    pub fn one_for_one() -> Self {
        Self {
            strategy: RestartStrategy::OneForOne,
            ..Default::default()
        }
    }

    pub fn all_for_one() -> Self {
        Self {
            strategy: RestartStrategy::AllForOne,
            ..Default::default()
        }
    }
}

/// Factory closure type for instantiating/restarting child actors.
pub type ActorFactory = Arc<dyn Fn() -> Result<Arc<ActorRef>, ActorError> + Send + Sync + 'static>;

/// Declarative specification for a supervised child actor.
#[derive(Clone)]
pub struct ChildSpec {
    pub id: String,
    pub start: ActorFactory,
    pub restart: RestartPolicy,
}

impl fmt::Debug for ChildSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChildSpec")
            .field("id", &self.id)
            .field("restart", &self.restart)
            .finish()
    }
}

impl ChildSpec {
    pub fn new(id: impl Into<String>, start: ActorFactory) -> Self {
        Self {
            id: id.into(),
            start,
            restart: RestartPolicy::Permanent,
        }
    }

    pub fn with_restart(mut self, restart: RestartPolicy) -> Self {
        self.restart = restart;
        self
    }

    pub fn with_restart_policy(mut self, restart: RestartPolicy) -> Self {
        self.restart = restart;
        self
    }

    pub fn restart_policy(&self) -> RestartPolicy {
        self.restart
    }

    pub fn from_fn<F>(id: impl Into<String>, f: F) -> Self
    where
        F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let id_str = id.into();
        let id_for_factory = id_str.clone();
        let f = Arc::new(f);
        let factory: ActorFactory = Arc::new(move || {
            let f_clone = Arc::clone(&f);
            ActorRef::spawn_internal(
                id_for_factory.clone(),
                Some(&id_for_factory),
                None,
                HandlerKind::NativeStateless(Box::new(move |msg| f_clone(msg))),
            )
        });
        Self {
            id: id_str,
            start: factory,
            restart: RestartPolicy::Permanent,
        }
    }

    pub fn from_stateful_fn<F>(id: impl Into<String>, initial_state: Value, f: F) -> Self
    where
        F: Fn(Value, Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let id_str = id.into();
        let id_for_factory = id_str.clone();
        let f = Arc::new(f);
        let init_state = initial_state.clone();
        let factory: ActorFactory = Arc::new(move || {
            let f_clone = Arc::clone(&f);
            ActorRef::spawn_internal(
                id_for_factory.clone(),
                Some(&id_for_factory),
                Some(init_state.deep_clone()),
                HandlerKind::NativeStateful(Box::new(move |st, msg| f_clone(st, msg))),
            )
        });
        Self {
            id: id_str,
            start: factory,
            restart: RestartPolicy::Permanent,
        }
    }

    pub fn from_shae(
        id: impl Into<String>,
        initial_state: Option<Value>,
        handler: Value,
        evaluator: Option<Evaluator>,
    ) -> Self {
        let id_str = id.into();
        let id_for_factory = id_str.clone();
        let init = initial_state.clone();
        let h = handler.clone();
        let factory: ActorFactory = Arc::new(move || {
            let eval = match &evaluator {
                Some(e) => Evaluator::with_env_cache_file(
                    e.global_env.clone(),
                    e.module_cache.clone(),
                    e.current_file.clone(),
                ),
                None => Evaluator::new(),
            };
            ActorRef::spawn_internal(
                id_for_factory.clone(),
                Some(&id_for_factory),
                init.as_ref().map(|v| v.deep_clone()),
                HandlerKind::Shae {
                    evaluator: eval,
                    handler: h.deep_clone(),
                },
            )
        });
        Self {
            id: id_str,
            start: factory,
            restart: RestartPolicy::Permanent,
        }
    }
}

/// Internal tracking representation for a managed child.
pub struct SupervisedChild {
    pub spec: ChildSpec,
    pub actor: Arc<ActorRef>,
    pub restart_count: usize,
    pub explicitly_stopped: bool,
    pub explicitly_restarting: bool,
}

/// Consolidated state containing child order and child lookup map.
/// Guarded by a single RwLock to eliminate multi-lock ordering deadlocks.
#[derive(Default)]
pub struct SupervisorChildrenState {
    pub order: Vec<String>,
    pub map: HashMap<String, SupervisedChild>,
}

impl SupervisorChildrenState {
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: String, child: SupervisedChild) {
        if !self.map.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.map.insert(id, child);
    }

    pub fn remove(&mut self, id: &str) -> Option<SupervisedChild> {
        self.order.retain(|k| k != id);
        self.map.remove(id)
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }

    pub fn get(&self, id: &str) -> Option<&SupervisedChild> {
        self.map.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SupervisedChild> {
        self.map.get_mut(id)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

pub enum SupervisorEvent {
    ChildExited {
        id: String,
        actor: Arc<ActorRef>,
        status: ActorStatus,
    },
    Shutdown,
}

/// Internal thread-shared state for a supervisor.
pub struct SupervisorInner {
    pub id: String,
    pub strategy: RestartStrategy,
    pub max_restarts: usize,
    pub within_seconds: Duration,
    pub status: RwLock<SupervisorStatus>,
    pub children: RwLock<SupervisorChildrenState>,
    pub restart_history: Mutex<Vec<Instant>>,
    pub event_tx: std::sync::mpsc::Sender<SupervisorEvent>,
    pub thread_handle: Mutex<Option<JoinHandle<()>>>,
    pub supervisor_thread_id: Mutex<Option<thread::ThreadId>>,
}

impl SupervisorInner {
    pub fn cascade_stop_all(&self) {
        let targets: Vec<(String, Arc<ActorRef>)> = {
            let state = match self.children.read() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            state
                .order
                .iter()
                .rev()
                .filter_map(|id| {
                    state
                        .map
                        .get(id)
                        .map(|mc| (id.clone(), Arc::clone(&mc.actor)))
                })
                .collect()
        };

        for (_, a) in &targets {
            a.stop();
        }
        for (id, a) in &targets {
            let _ = a.join();
            a.inner.mailbox.clear();
            ActorRegistry::unregister(id);
            if a.id() != id {
                ActorRegistry::unregister(a.id());
            }
        }
    }
}

/// Public thread-safe handle to a supervisor.
#[derive(Clone)]
pub struct SupervisorRef {
    pub id: String,
    pub(crate) inner: Arc<SupervisorInner>,
}

pub type Supervisor = SupervisorRef;

impl SupervisorRef {
    pub fn new(id: impl Into<String>) -> Self {
        let id_str = id.into();
        let (tx, _rx) = std::sync::mpsc::channel();
        let inner = Arc::new(SupervisorInner {
            id: id_str.clone(),
            strategy: RestartStrategy::OneForOne,
            max_restarts: 3,
            within_seconds: Duration::from_secs(5),
            status: RwLock::new(SupervisorStatus::Stopped),
            children: RwLock::new(SupervisorChildrenState::new()),
            restart_history: Mutex::new(Vec::new()),
            event_tx: tx,
            thread_handle: Mutex::new(None),
            supervisor_thread_id: Mutex::new(None),
        });
        Self { id: id_str, inner }
    }

    pub fn start(
        config: SupervisorConfig,
        specs: Vec<ChildSpec>,
    ) -> Result<Arc<SupervisorRef>, SupervisorError> {
        let id = format!(
            "supervisor-{}",
            NEXT_ACTOR_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        Self::start_named(id, config, specs)
    }

    pub fn start_named(
        id: impl Into<String>,
        config: SupervisorConfig,
        specs: Vec<ChildSpec>,
    ) -> Result<Arc<SupervisorRef>, SupervisorError> {
        let id_str = id.into();
        let (tx, rx) = std::sync::mpsc::channel();
        let window_secs = if config.within_seconds <= 0.0 {
            5.0
        } else {
            config.within_seconds
        };

        let inner = Arc::new(SupervisorInner {
            id: id_str.clone(),
            strategy: config.strategy,
            max_restarts: config.max_restarts,
            within_seconds: Duration::from_secs_f64(window_secs),
            status: RwLock::new(SupervisorStatus::Running),
            children: RwLock::new(SupervisorChildrenState::new()),
            restart_history: Mutex::new(Vec::new()),
            event_tx: tx,
            thread_handle: Mutex::new(None),
            supervisor_thread_id: Mutex::new(None),
        });

        let mut started_actors: Vec<(String, Arc<ActorRef>)> = Vec::new();
        let rollback = |started: Vec<(String, Arc<ActorRef>)>| {
            for (_, a) in &started {
                a.stop();
            }
            for (id, a) in started {
                let _ = a.join();
                a.inner.mailbox.clear();
                ActorRegistry::unregister(&id);
                if a.id() != id {
                    ActorRegistry::unregister(a.id());
                }
            }
        };

        for spec in specs {
            let already_exists = {
                let state = match inner.children.read() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                state.contains_key(&spec.id)
            };
            if already_exists {
                rollback(started_actors);
                return Err(SupervisorError::ChildAlreadyExists(spec.id));
            }

            let actor = match (spec.start)() {
                Ok(a) => a,
                Err(e) => {
                    rollback(started_actors);
                    return Err(SupervisorError::ChildStartError(spec.id, e.to_string()));
                }
            };

            let tx_clone = inner.event_tx.clone();
            let handle_clone = Arc::clone(&actor);
            actor.add_exit_listener(move |child_id, st| {
                let _ = tx_clone.send(SupervisorEvent::ChildExited {
                    id: child_id.to_string(),
                    actor: Arc::clone(&handle_clone),
                    status: st.clone(),
                });
            });

            let _ = ActorRegistry::register(&spec.id, Arc::clone(&actor));
            started_actors.push((spec.id.clone(), Arc::clone(&actor)));

            {
                let mut state = match inner.children.write() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                state.insert(
                    spec.id.clone(),
                    SupervisedChild {
                        spec,
                        actor,
                        restart_count: 0,
                        explicitly_stopped: false,
                        explicitly_restarting: false,
                    },
                );
            }
        }

        let loop_inner = Arc::clone(&inner);
        let thread_name = format!("shae-sup-{}", id_str);
        let handle = match thread::Builder::new().name(thread_name).spawn(move || {
            *loop_inner.supervisor_thread_id.lock().unwrap() = Some(thread::current().id());
            supervisor_monitor_loop(loop_inner, rx);
        }) {
            Ok(h) => h,
            Err(e) => {
                rollback(started_actors);
                return Err(SupervisorError::InvalidConfig(format!(
                    "Failed to spawn supervisor thread: {}",
                    e
                )));
            }
        };

        *inner.thread_handle.lock().unwrap() = Some(handle);

        Ok(Arc::new(SupervisorRef { id: id_str, inner }))
    }

    #[inline]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[inline]
    pub fn status(&self) -> SupervisorStatus {
        match self.inner.status.read() {
            Ok(s) => s.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.status().is_alive()
    }

    #[inline]
    pub fn strategy(&self) -> RestartStrategy {
        self.inner.strategy
    }

    pub fn get_child(&self, id: &str) -> Option<Arc<ActorRef>> {
        let state = match self.inner.children.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.get(id).map(|mc| Arc::clone(&mc.actor))
    }

    pub fn start_child(&self, spec: ChildSpec) -> Result<Arc<ActorRef>, SupervisorError> {
        {
            let status = match self.inner.status.read() {
                Ok(s) => s.clone(),
                Err(p) => p.into_inner().clone(),
            };
            match status {
                SupervisorStatus::Running => {}
                SupervisorStatus::Stopped => return Err(SupervisorError::SupervisorStopped),
                SupervisorStatus::Failed(err) => {
                    return Err(SupervisorError::SupervisorFailed(err));
                }
            }
        }

        {
            let state = match self.inner.children.read() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if state.contains_key(&spec.id) {
                return Err(SupervisorError::ChildAlreadyExists(spec.id));
            }
        }

        let actor = (spec.start)()
            .map_err(|e| SupervisorError::ChildStartError(spec.id.clone(), e.to_string()))?;

        let tx_clone = self.inner.event_tx.clone();
        let handle_clone = Arc::clone(&actor);
        actor.add_exit_listener(move |child_id, st| {
            let _ = tx_clone.send(SupervisorEvent::ChildExited {
                id: child_id.to_string(),
                actor: Arc::clone(&handle_clone),
                status: st.clone(),
            });
        });

        let duplicate_or_stopped = {
            let mut state = match self.inner.children.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };

            let sup_alive = match self.inner.status.read() {
                Ok(s) => s.is_alive(),
                Err(p) => p.into_inner().is_alive(),
            };

            if !sup_alive {
                Some(SupervisorError::SupervisorStopped)
            } else if state.contains_key(&spec.id) {
                Some(SupervisorError::ChildAlreadyExists(spec.id.clone()))
            } else {
                state.insert(
                    spec.id.clone(),
                    SupervisedChild {
                        spec: spec.clone(),
                        actor: Arc::clone(&actor),
                        restart_count: 0,
                        explicitly_stopped: false,
                        explicitly_restarting: false,
                    },
                );
                None
            }
        };

        if let Some(err) = duplicate_or_stopped {
            actor.stop();
            let _ = actor.join();
            actor.inner.mailbox.clear();
            return Err(err);
        }

        let _ = ActorRegistry::register(&spec.id, Arc::clone(&actor));

        Ok(actor)
    }

    pub fn stop_child(&self, id: &str) -> Result<(), SupervisorError> {
        let removed = {
            let mut state = match self.inner.children.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            state.remove(id)
        };

        let mut child = removed.ok_or_else(|| SupervisorError::ChildNotFound(id.to_string()))?;
        child.explicitly_stopped = true;

        child.actor.stop();
        let _ = child.actor.join();
        child.actor.inner.mailbox.clear();
        ActorRegistry::unregister(id);
        if child.actor.id() != id {
            ActorRegistry::unregister(child.actor.id());
        }

        Ok(())
    }

    pub fn restart_child(&self, id: &str) -> Result<Arc<ActorRef>, SupervisorError> {
        {
            let status = match self.inner.status.read() {
                Ok(s) => s.clone(),
                Err(p) => p.into_inner().clone(),
            };
            if !status.is_alive() {
                return Err(SupervisorError::SupervisorNotAlive);
            }
        }

        let (spec, old_actor) = {
            let mut state = match self.inner.children.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let child = state
                .get_mut(id)
                .ok_or_else(|| SupervisorError::ChildNotFound(id.to_string()))?;
            if child.explicitly_restarting {
                return Err(SupervisorError::ChildStartError(
                    id.to_string(),
                    "Child is already restarting".to_string(),
                ));
            }
            child.explicitly_restarting = true;
            (child.spec.clone(), Arc::clone(&child.actor))
        };

        old_actor.stop();
        let _ = old_actor.join();
        old_actor.inner.mailbox.clear();

        let spawn_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.start)()));
        let new_actor = match spawn_res {
            Ok(Ok(actor)) => actor,
            Ok(Err(e)) => {
                let mut state = match self.inner.children.write() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if let Some(child) = state.get_mut(id) {
                    child.explicitly_restarting = false;
                }
                return Err(SupervisorError::ChildStartError(
                    id.to_string(),
                    e.to_string(),
                ));
            }
            Err(panic_payload) => {
                let mut state = match self.inner.children.write() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if let Some(child) = state.get_mut(id) {
                    child.explicitly_restarting = false;
                }
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Child start panicked".to_string()
                };
                return Err(SupervisorError::ChildStartError(id.to_string(), panic_msg));
            }
        };

        let tx_clone = self.inner.event_tx.clone();
        let handle_clone = Arc::clone(&new_actor);
        new_actor.add_exit_listener(move |child_id, st| {
            let _ = tx_clone.send(SupervisorEvent::ChildExited {
                id: child_id.to_string(),
                actor: Arc::clone(&handle_clone),
                status: st.clone(),
            });
        });

        let sup_alive = match self.inner.status.read() {
            Ok(s) => s.is_alive(),
            Err(p) => p.into_inner().is_alive(),
        };

        if !sup_alive {
            new_actor.stop();
            let _ = new_actor.join();
            new_actor.inner.mailbox.clear();
            let mut state = match self.inner.children.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if let Some(child) = state.get_mut(id) {
                child.explicitly_restarting = false;
            }
            return Err(SupervisorError::SupervisorNotAlive);
        }

        let updated = {
            let mut state = match self.inner.children.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if let Some(child) = state.get_mut(id) {
                if child.explicitly_restarting && Arc::ptr_eq(&child.actor, &old_actor) {
                    child.actor = Arc::clone(&new_actor);
                    child.restart_count += 1;
                    child.explicitly_restarting = false;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if !updated {
            new_actor.stop();
            let _ = new_actor.join();
            new_actor.inner.mailbox.clear();
            return Err(SupervisorError::ChildNotFound(id.to_string()));
        }

        let _ = ActorRegistry::register(&spec.id, Arc::clone(&new_actor));

        Ok(new_actor)
    }

    pub fn children(&self) -> Vec<(String, ActorStatus)> {
        let state = match self.inner.children.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut list = Vec::with_capacity(state.order.len());
        for id in &state.order {
            if let Some(mc) = state.map.get(id) {
                list.push((id.clone(), mc.actor.status()));
            }
        }
        list
    }

    pub fn len(&self) -> usize {
        match self.inner.children.read() {
            Ok(s) => s.len(),
            Err(p) => p.into_inner().len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stop(&self) -> bool {
        {
            let mut status = match self.inner.status.write() {
                Ok(s) => s,
                Err(p) => p.into_inner(),
            };
            if *status != SupervisorStatus::Running {
                return false;
            }
            *status = SupervisorStatus::Stopped;
        }

        let _ = self.inner.event_tx.send(SupervisorEvent::Shutdown);

        self.inner.cascade_stop_all();

        let current_tid = thread::current().id();
        let sup_tid = *self.inner.supervisor_thread_id.lock().unwrap();
        if Some(current_tid) != sup_tid {
            let handle = {
                let mut guard = match self.inner.thread_handle.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                guard.take()
            };
            if let Some(h) = handle {
                let _ = h.join();
            }
        }

        true
    }

    pub fn join(&self) -> Result<(), SupervisorError> {
        let current_tid = thread::current().id();
        let sup_tid = *self.inner.supervisor_thread_id.lock().unwrap();
        if Some(current_tid) == sup_tid {
            return Ok(());
        }
        let handle = {
            let mut guard = match self.inner.thread_handle.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.take()
        };
        if let Some(h) = handle {
            let _ = h.join();
        }
        Ok(())
    }

    pub fn stop_and_join(&self) -> Result<bool, SupervisorError> {
        let stopped = self.stop();
        self.join()?;
        Ok(stopped)
    }
}

impl fmt::Debug for SupervisorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SupervisorRef({})", self.id)
    }
}

impl fmt::Display for SupervisorRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<supervisor:{}>", self.id)
    }
}

impl PartialEq for SupervisorRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || self.id == other.id
    }
}

impl Eq for SupervisorRef {}

impl Hash for SupervisorRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

fn supervisor_monitor_loop(
    inner: Arc<SupervisorInner>,
    rx: std::sync::mpsc::Receiver<SupervisorEvent>,
) {
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(SupervisorEvent::ChildExited { id, actor, status }) => {
                if !inner.status.read().unwrap().is_alive() {
                    break;
                }
                handle_child_exit(&inner, &id, &actor, &status);
            }
            Ok(SupervisorEvent::Shutdown) => {
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !inner.status.read().unwrap().is_alive() {
                    break;
                }
                check_and_handle_failed_children(&inner);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
}

fn check_and_handle_failed_children(inner: &Arc<SupervisorInner>) {
    let failed: Option<(String, Arc<ActorRef>, ActorStatus)> = {
        let state = match inner.children.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.map.iter().find_map(|(id, child)| {
            let st = child.actor.status();
            if st.is_failed() && !child.explicitly_stopped && !child.explicitly_restarting {
                Some((id.clone(), Arc::clone(&child.actor), st))
            } else {
                None
            }
        })
    };
    if let Some((id, actor, status)) = failed {
        handle_child_exit(inner, &id, &actor, &status);
    }
}

fn handle_child_exit(
    inner: &Arc<SupervisorInner>,
    id: &str,
    actor: &Arc<ActorRef>,
    status: &ActorStatus,
) {
    {
        let sup_status = match inner.status.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !sup_status.is_alive() {
            return;
        }
    }

    let (spec, old_actor, policy) = {
        let state = match inner.children.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match state.get(id) {
            Some(child) => {
                if !Arc::ptr_eq(&child.actor, actor) {
                    return;
                }
                if child.explicitly_stopped || child.explicitly_restarting {
                    return;
                }
                (
                    child.spec.clone(),
                    Arc::clone(&child.actor),
                    child.spec.restart,
                )
            }
            None => return,
        }
    };

    let should_restart = match policy {
        RestartPolicy::Temporary => false,
        RestartPolicy::Transient => status.is_failed(),
        RestartPolicy::Permanent => status.is_failed() || status.is_stopped(),
    };

    if !should_restart {
        return;
    }

    let now = Instant::now();
    let rate_limit_ok = {
        let mut history = match inner.restart_history.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let cutoff = now.checked_sub(inner.within_seconds).unwrap_or(now);
        history.retain(|&t| t >= cutoff);
        if history.len() >= inner.max_restarts {
            false
        } else {
            history.push(now);
            true
        }
    };

    if !rate_limit_ok {
        let err_msg = format!(
            "Restart rate limit exceeded: {} restarts within {:.1}s",
            inner.max_restarts,
            inner.within_seconds.as_secs_f64()
        );
        {
            let mut s = match inner.status.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            *s = SupervisorStatus::Failed(err_msg);
        }
        inner.cascade_stop_all();
        return;
    }

    match inner.strategy {
        RestartStrategy::OneForOne => {
            {
                let mut state = match inner.children.write() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if let Some(child) = state.get_mut(id) {
                    if child.explicitly_stopped || child.explicitly_restarting {
                        return;
                    }
                    child.explicitly_restarting = true;
                } else {
                    return;
                }
            }

            old_actor.stop();
            let _ = old_actor.join();
            old_actor.inner.mailbox.clear();

            match (spec.start)() {
                Ok(new_actor) => {
                    let sup_alive = match inner.status.read() {
                        Ok(s) => s.is_alive(),
                        Err(p) => p.into_inner().is_alive(),
                    };
                    if !sup_alive {
                        new_actor.stop();
                        let _ = new_actor.join();
                        new_actor.inner.mailbox.clear();
                        ActorRegistry::unregister(&spec.id);
                        if new_actor.id() != spec.id {
                            ActorRegistry::unregister(new_actor.id());
                        }
                        return;
                    }

                    let tx = inner.event_tx.clone();
                    let handle = Arc::clone(&new_actor);
                    new_actor.add_exit_listener(move |exit_id, exit_st| {
                        let _ = tx.send(SupervisorEvent::ChildExited {
                            id: exit_id.to_string(),
                            actor: Arc::clone(&handle),
                            status: exit_st.clone(),
                        });
                    });
                    let mut state = match inner.children.write() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    if let Some(child) = state.get_mut(id) {
                        if child.explicitly_restarting && Arc::ptr_eq(&child.actor, &old_actor) {
                            child.actor = Arc::clone(&new_actor);
                            child.restart_count += 1;
                            child.explicitly_restarting = false;
                            let _ = ActorRegistry::register(&spec.id, Arc::clone(&new_actor));
                        } else {
                            new_actor.stop();
                            let _ = new_actor.join();
                            new_actor.inner.mailbox.clear();
                        }
                    } else {
                        new_actor.stop();
                        let _ = new_actor.join();
                        new_actor.inner.mailbox.clear();
                        ActorRegistry::unregister(&spec.id);
                        if new_actor.id() != spec.id {
                            ActorRegistry::unregister(new_actor.id());
                        }
                    }
                }
                Err(e) => {
                    {
                        let mut state = match inner.children.write() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        if let Some(child) = state.get_mut(id) {
                            child.explicitly_restarting = false;
                        }
                    }
                    let mut s = match inner.status.write() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    *s = SupervisorStatus::Failed(format!(
                        "Failed to restart child '{}': {}",
                        spec.id, e
                    ));
                    inner.cascade_stop_all();
                }
            }
        }
        RestartStrategy::AllForOne => {
            let (specs, actors) = {
                let state = match inner.children.read() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                let mut specs = Vec::new();
                let mut actors = Vec::new();
                for child_id in &state.order {
                    if let Some(mc) = state.map.get(child_id) {
                        specs.push(mc.spec.clone());
                        actors.push(Arc::clone(&mc.actor));
                    }
                }
                (specs, actors)
            };

            {
                let mut state = match inner.children.write() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                for sp in &specs {
                    if let Some(c) = state.get_mut(&sp.id) {
                        c.explicitly_restarting = true;
                    }
                }
            }

            for a in actors.iter().rev() {
                a.stop();
            }
            for a in actors.iter().rev() {
                let _ = a.join();
                a.inner.mailbox.clear();
            }

            let mut restarted: Vec<(String, Arc<ActorRef>)> = Vec::new();
            for sp in &specs {
                match (sp.start)() {
                    Ok(new_actor) => {
                        let sup_alive = match inner.status.read() {
                            Ok(s) => s.is_alive(),
                            Err(p) => p.into_inner().is_alive(),
                        };
                        if !sup_alive {
                            new_actor.stop();
                            let _ = new_actor.join();
                            new_actor.inner.mailbox.clear();
                            ActorRegistry::unregister(&sp.id);
                            if new_actor.id() != sp.id {
                                ActorRegistry::unregister(new_actor.id());
                            }
                            for (id, act) in &restarted {
                                act.stop();
                                let _ = act.join();
                                act.inner.mailbox.clear();
                                ActorRegistry::unregister(id);
                                if act.id() != id.as_str() {
                                    ActorRegistry::unregister(act.id());
                                }
                            }
                            return;
                        }

                        let tx = inner.event_tx.clone();
                        let handle = Arc::clone(&new_actor);
                        new_actor.add_exit_listener(move |exit_id, exit_st| {
                            let _ = tx.send(SupervisorEvent::ChildExited {
                                id: exit_id.to_string(),
                                actor: Arc::clone(&handle),
                                status: exit_st.clone(),
                            });
                        });
                        restarted.push((sp.id.clone(), new_actor));
                    }
                    Err(e) => {
                        for (_, act) in &restarted {
                            act.stop();
                        }
                        for (id, act) in restarted {
                            let _ = act.join();
                            act.inner.mailbox.clear();
                            ActorRegistry::unregister(&id);
                            if act.id() != id {
                                ActorRegistry::unregister(act.id());
                            }
                        }
                        {
                            let mut state = match inner.children.write() {
                                Ok(g) => g,
                                Err(p) => p.into_inner(),
                            };
                            for (_, c) in state.map.iter_mut() {
                                c.explicitly_restarting = false;
                            }
                        }
                        let mut s = match inner.status.write() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        *s = SupervisorStatus::Failed(format!(
                            "Failed to respawn child '{}' during AllForOne restart: {}",
                            sp.id, e
                        ));
                        inner.cascade_stop_all();
                        return;
                    }
                }
            }

            let sup_alive = match inner.status.read() {
                Ok(s) => s.is_alive(),
                Err(p) => p.into_inner().is_alive(),
            };
            if !sup_alive {
                for (id, act) in &restarted {
                    act.stop();
                    let _ = act.join();
                    act.inner.mailbox.clear();
                    ActorRegistry::unregister(id);
                    if act.id() != id.as_str() {
                        ActorRegistry::unregister(act.id());
                    }
                }
                return;
            }

            let mut state = match inner.children.write() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            for (child_id, new_actor) in restarted {
                if let Some(mc) = state.get_mut(&child_id) {
                    mc.actor = Arc::clone(&new_actor);
                    mc.restart_count += 1;
                    mc.explicitly_restarting = false;
                    let _ = ActorRegistry::register(&child_id, new_actor);
                } else {
                    new_actor.stop();
                    let _ = new_actor.join();
                    new_actor.inner.mailbox.clear();
                    ActorRegistry::unregister(&child_id);
                    if new_actor.id() != child_id {
                        ActorRegistry::unregister(new_actor.id());
                    }
                }
            }
        }
    }
}

/// Global, thread-safe registry mapping names to active ActorRef handles.
pub struct ActorRegistry;

static GLOBAL_REGISTRY: OnceLock<RwLock<HashMap<String, Arc<ActorRef>>>> = OnceLock::new();

fn get_registry() -> &'static RwLock<HashMap<String, Arc<ActorRef>>> {
    GLOBAL_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

impl ActorRegistry {
    /// Registers an actor under a unique name.
    /// Returns Err(ActorError::NameAlreadyRegistered) if name is already registered to an alive actor.
    pub fn register(name: impl Into<String>, actor: Arc<ActorRef>) -> Result<(), ActorError> {
        let name_str = name.into();
        let trimmed = name_str.trim();
        if trimmed.is_empty() {
            return Err(ActorError::InvalidName(
                "Actor name cannot be empty".to_string(),
            ));
        }

        if !actor.is_alive() {
            return Err(ActorError::ActorNotAlive);
        }

        let mut reg = match get_registry().write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(existing) = reg.get(trimmed) {
            if existing.is_alive() {
                return Err(ActorError::NameAlreadyRegistered(trimmed.to_string()));
            }
        }

        reg.insert(trimmed.to_string(), actor);
        Ok(())
    }

    /// Resolves an actor handle by name. Returns Some(Arc<ActorRef>) if registered and alive.
    pub fn whereis(name: &str) -> Option<Arc<ActorRef>> {
        let reg = match get_registry().read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(actor) = reg.get(name) {
            if actor.is_alive() {
                return Some(Arc::clone(actor));
            }
        }
        None
    }

    /// Unregisters an actor by name, returning the registered handle if found.
    pub fn unregister(name: &str) -> Option<Arc<ActorRef>> {
        let mut reg = match get_registry().write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        reg.remove(name)
    }

    /// Lists all currently registered and alive actor names in sorted alphabetical order.
    pub fn registered() -> Vec<String> {
        let reg = match get_registry().read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut names: Vec<String> = reg
            .iter()
            .filter(|(_, actor)| actor.is_alive())
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names
    }

    /// Clears all registrations. Intended for test harness isolation.
    pub fn clear() {
        let mut reg = match get_registry().write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        reg.clear();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::RwLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn spawn_test_actor<F>(f: F) -> Arc<ActorRef>
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        spawn_native(Arc::new(f))
    }

    // =========================================================================
    // 1. Mailbox FIFO Ordering Test
    // =========================================================================
    #[test]
    fn test_mailbox_fifo_ordering() {
        let received_chan = Channel::new(None);
        let rx_chan = Arc::clone(&received_chan);

        let actor = spawn_test_actor(move |msg: Value| {
            rx_chan.send(msg, Span::new(1, 1)).unwrap();
        });

        let total_messages = 50;
        for i in 0..total_messages {
            actor.send(Value::Int(i)).expect("Send must succeed");
        }

        // Verify all 50 messages arrive in strict sequential order
        for i in 0..total_messages {
            let received = received_chan
                .recv(Some(2000), Span::new(1, 1))
                .expect("Recv timed out");
            assert_eq!(
                received,
                Value::Int(i),
                "Message sequence out of order! Expected {}, got {:?}",
                i,
                received
            );
        }

        actor.stop();
    }

    // =========================================================================
    // 2. State Isolation: Mutating Sender Data After Send
    // =========================================================================
    #[test]
    fn test_state_isolation_mutating_sender_does_not_mutate_actor() {
        let reply_chan = Channel::new(None);
        let rx_reply = Arc::clone(&reply_chan);

        let actor = spawn_test_actor(move |msg: Value| {
            // Wait slightly so sender has ample opportunity to mutate its local array
            std::thread::sleep(Duration::from_millis(30));
            rx_reply.send(msg, Span::new(1, 1)).unwrap();
        });

        // Sender creates a shared array [1, 2]
        let original_inner = Arc::new(RwLock::new(vec![Value::Int(1), Value::Int(2)]));
        let sender_array = Value::Array(Arc::clone(&original_inner));

        // Dispatch to actor (internally triggers Value::deep_clone)
        actor.send(sender_array.clone()).expect("Send succeeded");

        // IMMEDIATELY mutate sender's local copy
        original_inner.write().unwrap().push(Value::Int(999));
        original_inner.write().unwrap()[0] = Value::Int(100);

        // Sender's local array now has [100, 2, 999]
        assert_eq!(
            *original_inner.read().unwrap(),
            vec![Value::Int(100), Value::Int(2), Value::Int(999)]
        );

        // Wait for actor to forward its received message
        let received_msg = reply_chan
            .recv(Some(2000), Span::new(1, 1))
            .expect("Recv failed");

        // Actor MUST have received the pristine original [1, 2]
        if let Value::Array(ref actor_inner) = received_msg {
            assert_eq!(
                *actor_inner.read().unwrap(),
                vec![Value::Int(1), Value::Int(2)],
                "State isolation violation: Actor received mutated sender data!"
            );
            // Verify heap pointers differ
            assert_ne!(
                Arc::as_ptr(&original_inner),
                Arc::as_ptr(actor_inner),
                "Memory leak: Sender and Actor share the same Arc pointer!"
            );
        } else {
            panic!("Expected Value::Array, got {:?}", received_msg);
        }

        actor.stop();
    }

    // =========================================================================
    // 3. State Isolation: Mutating Actor Data Does Not Mutate Sender
    // =========================================================================
    #[test]
    fn test_state_isolation_mutating_actor_does_not_mutate_sender() {
        let signal_chan = Channel::new(None);
        let rx_signal = Arc::clone(&signal_chan);

        let actor = spawn_test_actor(move |msg: Value| {
            if let Value::Array(ref arr) = msg {
                // Actor mutates its copy of the array
                arr.write()
                    .unwrap()
                    .push(Value::String("actor_mutation".into()));
            }
            rx_signal.send(Value::Bool(true), Span::new(1, 1)).unwrap();
        });

        let original_inner = Arc::new(RwLock::new(vec![Value::Int(42)]));
        let sender_array = Value::Array(Arc::clone(&original_inner));

        actor.send(sender_array.clone()).expect("Send succeeded");

        // Wait for actor to complete mutation
        signal_chan
            .recv(Some(2000), Span::new(1, 1))
            .expect("Actor did not signal");

        // Sender's array must remain pristine [42]
        assert_eq!(
            *original_inner.read().unwrap(),
            vec![Value::Int(42)],
            "State isolation violation: Actor mutated sender memory!"
        );

        actor.stop();
    }

    // =========================================================================
    // 4. Non-Blocking Send Under Heavy Load
    // =========================================================================
    #[test]
    fn test_non_blocking_send() {
        // Slow actor taking 200ms per message
        let actor = spawn_test_actor(|_msg: Value| {
            std::thread::sleep(Duration::from_millis(200));
        });

        let start = Instant::now();
        let message_count = 10;
        for i in 0..message_count {
            actor.send(Value::Int(i)).expect("Send should succeed");
        }
        let elapsed = start.elapsed();

        // Non-blocking send must return almost instantaneously (< 50ms).
        assert!(
            elapsed < Duration::from_millis(50),
            "actor.send blocked the caller thread! Total elapsed: {:?}",
            elapsed
        );

        // Mailbox length must reflect queued messages
        assert!(
            actor.mailbox_len() > 0,
            "Expected queued messages in mailbox"
        );

        actor.stop();
    }

    // =========================================================================
    // 5. Named Actor Registry Operations
    // =========================================================================
    #[test]
    fn test_named_registry_operations() {
        let name = "test_named_reg_auth_worker";
        ActorRegistry::unregister(name);

        let actor1 = spawn_test_actor(|_| {});
        let actor2 = spawn_test_actor(|_| {});

        // 1. Whereis on nonexistent returns None
        assert!(ActorRegistry::whereis(name).is_none());

        // 2. Register actor1
        ActorRegistry::register(name, Arc::clone(&actor1)).expect("Registration failed");

        // 3. Whereis returns actor1
        let resolved = ActorRegistry::whereis(name).expect("Lookup failed");
        assert_eq!(resolved.id(), actor1.id());

        // 4. Duplicate registration fails with NameAlreadyRegistered
        let err = ActorRegistry::register(name, Arc::clone(&actor2));
        assert!(err.is_err());
        match err.unwrap_err() {
            ActorError::NameAlreadyRegistered(n) => assert_eq!(n, name),
            other => panic!("Expected NameAlreadyRegistered, got {:?}", other),
        }

        // 5. Registered() enumerates this actor
        let reg_list = ActorRegistry::registered();
        assert!(
            reg_list.contains(&name.to_string()),
            "registered() must contain '{}', got {:?}",
            name,
            reg_list
        );

        // 6. Unregister this actor
        let removed = ActorRegistry::unregister(name).expect("Unregister failed");
        assert_eq!(removed.id(), actor1.id());

        // 7. Whereis returns None after unregistration
        assert!(ActorRegistry::whereis(name).is_none());
        assert!(
            !ActorRegistry::registered().contains(&name.to_string()),
            "registered() must not contain '{}' after unregistration",
            name
        );

        actor1.stop();
        actor2.stop();
        let _ = actor1.join();
        let _ = actor2.join();
    }

    // =========================================================================
    // 6. Deep Clone Cycle Detection (Self-Referential Array)
    // =========================================================================
    #[test]
    fn test_deep_clone_cyclic_array() {
        let arr = Value::Array(Arc::new(RwLock::new(Vec::new())));
        if let Value::Array(ref a) = arr {
            a.write().unwrap().push(Value::Int(1));
            a.write().unwrap().push(arr.clone()); // Cyclic reference: arr contains arr!
        }

        // Must terminate safely and not blow the stack or deadlock
        let cloned = arr.deep_clone();

        if let Value::Array(ref cloned_inner) = cloned {
            let guard = cloned_inner.read().unwrap();
            assert_eq!(guard.len(), 2);
            assert_eq!(guard[0], Value::Int(1));

            // Verify inner element points to the NEW cloned array
            if let Value::Array(ref inner_ref) = guard[1] {
                assert_eq!(
                    Arc::as_ptr(cloned_inner),
                    Arc::as_ptr(inner_ref),
                    "Cloned array self-reference should point to the cloned instance"
                );
                // Verify it does NOT point to original
                if let Value::Array(ref orig_inner) = arr {
                    assert_ne!(
                        Arc::as_ptr(cloned_inner),
                        Arc::as_ptr(orig_inner),
                        "Cloned array must have independent allocation"
                    );
                }
            } else {
                panic!("Expected cyclic Value::Array");
            }
        } else {
            panic!("Expected Value::Array");
        }
    }

    // =========================================================================
    // 7. Deep Clone Nested Compound Structures (Map inside Struct inside Array)
    // =========================================================================
    #[test]
    fn test_deep_clone_nested_compound_structures() {
        let mut inner_map = indexmap::IndexMap::new();
        inner_map.insert("counter".to_string(), Value::Int(10));

        let mut struct_fields = indexmap::IndexMap::new();
        struct_fields.insert(
            "metrics".to_string(),
            Value::Map(Arc::new(RwLock::new(inner_map))),
        );

        let struct_instance = Value::StructInstance {
            name: "WorkerStats".to_string(),
            fields: Arc::new(RwLock::new(struct_fields)),
        };

        let outer_array = Value::Array(Arc::new(RwLock::new(vec![struct_instance])));

        let cloned = outer_array.deep_clone();

        // Mutate deep nested map in original
        if let Value::Array(ref arr) = outer_array {
            if let Value::StructInstance { ref fields, .. } = arr.read().unwrap()[0] {
                if let Some(Value::Map(m)) = fields.read().unwrap().get("metrics") {
                    m.write()
                        .unwrap()
                        .insert("counter".to_string(), Value::Int(9999));
                }
            }
        }

        // Verify clone retains 10
        if let Value::Array(ref arr) = cloned {
            if let Value::StructInstance { ref fields, .. } = arr.read().unwrap()[0] {
                if let Some(Value::Map(m)) = fields.read().unwrap().get("metrics") {
                    assert_eq!(
                        m.read().unwrap().get("counter"),
                        Some(&Value::Int(10)),
                        "Nested compound structure was not deeply isolated!"
                    );
                } else {
                    panic!("Missing metrics map");
                }
            } else {
                panic!("Missing struct instance");
            }
        } else {
            panic!("Missing cloned array");
        }
    }

    // =========================================================================
    // 8. Sending Message to Stopped Actor Fails Gracefully
    // =========================================================================
    #[test]
    fn test_send_to_stopped_actor_returns_error() {
        let actor = spawn_test_actor(|_| {});
        assert!(actor.is_alive());

        actor.stop();

        // Wait for thread to exit
        for _ in 0..50 {
            if !actor.is_alive() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!actor.is_alive());

        // Send must return error
        let result = actor.send(Value::Int(42));
        assert!(result.is_err());
    }

    // =========================================================================
    // 9. Ask Pattern Timeout
    // =========================================================================
    #[test]
    fn test_ask_timeout() {
        // Actor that intentionally does NOT send a reply within timeout
        let actor = spawn_test_actor(|_msg: Value| {
            std::thread::sleep(Duration::from_millis(200));
        });

        let res = actor.ask(
            Value::String("ping".into()),
            Some(Duration::from_millis(50)),
        );
        assert!(res.is_err());
        match res.unwrap_err() {
            ActorError::Timeout => (),
            other => panic!("Expected Timeout error, got {:?}", other),
        }

        actor.stop();
    }

    // =========================================================================
    // Additional Comprehensive Tests (Explorer 2 & Thread Identity)
    // =========================================================================
    #[test]
    fn test_stateless_actor_fifo_execution() {
        let results = Arc::new(Mutex::new(Vec::new()));
        let r_clone = results.clone();

        let actor = ActorRef::spawn_fn(move |msg| {
            if let Value::Int(n) = msg {
                r_clone.lock().unwrap().push(n);
            }
            Ok(Value::Null)
        });

        for i in 1..=5 {
            actor.send(Value::Int(i)).unwrap();
        }

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(*results.lock().unwrap(), vec![1, 2, 3, 4, 5]);
        assert_eq!(actor.mailbox_len(), 0);
        assert!(actor.is_alive());
        actor.stop();
    }

    #[test]
    fn test_stateful_actor_state_transition() {
        let actor = ActorRef::spawn_stateful_fn(Value::Int(0), |state, msg| match (state, msg) {
            (Value::Int(s), Value::Int(m)) => Ok(Value::Int(s + m)),
            _ => Err("Invalid types".into()),
        });

        actor.send(Value::Int(10)).unwrap();
        actor.send(Value::Int(20)).unwrap();
        actor.send(Value::Int(30)).unwrap();

        let reply = actor
            .ask(Value::Int(0), Some(Duration::from_millis(500)))
            .unwrap();
        assert_eq!(reply, Value::Int(60));
        actor.stop();
    }

    #[test]
    fn test_ask_pattern_immediate_error_isolation() {
        let actor = ActorRef::spawn_fn(|msg| {
            if let Value::String(s) = msg {
                if s == "crash" {
                    return Err("Deliberate failure".into());
                }
            }
            Ok(Value::String("ok".into()))
        });

        let start = Instant::now();
        let res = actor.ask(Value::String("crash".into()), Some(Duration::from_secs(5)));
        assert!(start.elapsed() < Duration::from_millis(200));
        assert!(matches!(res, Err(ActorError::ActorFailed(_))));
        assert!(!actor.is_alive());
        assert!(matches!(actor.status(), ActorStatus::Failed(_)));
    }

    #[test]
    fn test_panic_isolation() {
        let actor = ActorRef::spawn_fn(|_| {
            panic!("Simulated internal panic in actor");
        });

        let res = actor.send(Value::Null);
        assert!(res.is_ok());

        std::thread::sleep(Duration::from_millis(50));
        assert!(!actor.is_alive());
        if let ActorStatus::Failed(msg) = actor.status() {
            assert!(msg.contains("Simulated internal panic"));
        } else {
            panic!("Expected Failed status");
        }
    }

    #[test]
    fn test_thread_local_actor_identity() {
        let recorded_pid = Arc::new(Mutex::new(None));
        let r_clone = recorded_pid.clone();

        let actor = spawn_test_actor(move |_| {
            let pid = current_actor();
            *r_clone.lock().unwrap() = pid.map(|a| a.id().to_string());
        });

        assert!(
            current_actor().is_none(),
            "Main thread should have no current_actor"
        );
        actor.send(Value::Int(1)).unwrap();

        std::thread::sleep(Duration::from_millis(50));
        let recorded = recorded_pid.lock().unwrap().clone();
        assert_eq!(recorded, Some(actor.id().to_string()));

        actor.stop();
    }

    // =========================================================================
    // Milestone 2 Supervision Unit Tests
    // =========================================================================

    #[test]
    fn test_supervision_one_for_one_isolated_restart() {
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 5,
            within_seconds: 5.0,
        };

        let spec1 = ChildSpec::from_stateful_fn("sup_o4o_w1", Value::Int(0), |state, msg| {
            match (state, msg) {
                (Value::Int(_), Value::String(ref s)) if s == "crash" => {
                    Err("Worker 1 deliberate crash".into())
                }
                (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                (st, _) => Ok(st),
            }
        });

        let spec2 =
            ChildSpec::from_stateful_fn("sup_o4o_w2", Value::Int(100), |state, msg| {
                match (state, msg) {
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                }
            });

        let sup = Supervisor::start(config, vec![spec1, spec2]).expect("Supervisor should start");

        let w1_initial = sup.get_child("sup_o4o_w1").expect("Worker 1 must exist");
        let w2_initial = sup.get_child("sup_o4o_w2").expect("Worker 2 must exist");

        // Increment both workers
        assert_eq!(w1_initial.ask(Value::Int(5), None).unwrap(), Value::Int(5));
        assert_eq!(
            w2_initial.ask(Value::Int(5), None).unwrap(),
            Value::Int(105)
        );

        // Crash Worker 1
        let crash_res = w1_initial.ask(
            Value::String("crash".into()),
            Some(Duration::from_millis(200)),
        );
        assert!(crash_res.is_err(), "Worker 1 must fail");
        assert!(!w1_initial.is_alive());

        // Wait for supervisor to restart Worker 1
        let mut w1_restarted = None;
        for _ in 0..100 {
            if let Some(child) = sup.get_child("sup_o4o_w1") {
                if child.is_alive()
                    && (child.id() != w1_initial.id() || child.status() == ActorStatus::Running)
                {
                    if child
                        .ask(Value::Int(0), Some(Duration::from_millis(50)))
                        .is_ok()
                    {
                        w1_restarted = Some(child);
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }

        let w1 = w1_restarted.expect("Worker 1 must have been restarted by supervisor");
        // Verify Worker 1 state is clean initial 0!
        let w1_fresh_state = w1.ask(Value::Int(0), None).unwrap();
        assert_eq!(
            w1_fresh_state,
            Value::Int(0),
            "Worker 1 must restart with fresh initial state 0"
        );

        // Verify Worker 2 state remains 105 untouched!
        let w2 = sup
            .get_child("sup_o4o_w2")
            .expect("Worker 2 must still exist");
        assert!(w2.is_alive(), "Worker 2 must remain alive continuously");
        let w2_state = w2.ask(Value::Int(0), None).unwrap();
        assert_eq!(
            w2_state,
            Value::Int(105),
            "Worker 2 state must be completely untouched"
        );

        sup.stop();
        let _ = sup.join();
    }

    #[test]
    fn test_supervision_all_for_one_sibling_restart() {
        let config = SupervisorConfig {
            strategy: RestartStrategy::AllForOne,
            max_restarts: 5,
            within_seconds: 5.0,
        };

        let spec1 = ChildSpec::from_stateful_fn("sup_a4o_w1", Value::Int(0), |state, msg| {
            match (state, msg) {
                (Value::Int(_), Value::String(ref s)) if s == "crash" => {
                    Err("Deliberate failure in w1".into())
                }
                (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                (st, _) => Ok(st),
            }
        });

        let spec2 =
            ChildSpec::from_stateful_fn("sup_a4o_w2", Value::Int(50), |state, msg| {
                match (state, msg) {
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                }
            });

        let sup = Supervisor::start(config, vec![spec1, spec2]).expect("Supervisor should start");

        let w1_init = sup.get_child("sup_a4o_w1").unwrap();
        let w2_init = sup.get_child("sup_a4o_w2").unwrap();

        // Mutate state
        assert_eq!(w1_init.ask(Value::Int(10), None).unwrap(), Value::Int(10));
        assert_eq!(w2_init.ask(Value::Int(10), None).unwrap(), Value::Int(60));

        // Crash w1
        let _ = w1_init.ask(
            Value::String("crash".into()),
            Some(Duration::from_millis(200)),
        );

        // Poll until both workers are restarted and responding to fresh state
        let mut both_restarted = false;
        for _ in 0..100 {
            if let (Some(w1), Some(w2)) = (sup.get_child("sup_a4o_w1"), sup.get_child("sup_a4o_w2"))
            {
                if w1.is_alive() && w2.is_alive() {
                    if let (Ok(s1), Ok(s2)) = (
                        w1.ask(Value::Int(0), Some(Duration::from_millis(50))),
                        w2.ask(Value::Int(0), Some(Duration::from_millis(50))),
                    ) {
                        if s1 == Value::Int(0) && s2 == Value::Int(50) {
                            both_restarted = true;
                            break;
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }

        assert!(
            both_restarted,
            "Both workers must be restarted with fresh state under AllForOne"
        );

        sup.stop();
        let _ = sup.join();
    }

    #[test]
    fn test_supervision_restart_rate_limit_threshold() {
        let config = SupervisorConfig {
            strategy: RestartStrategy::OneForOne,
            max_restarts: 3,
            within_seconds: 1.0,
        };

        let spec = ChildSpec::from_fn("sup_rl_looper", |msg| {
            if let Value::String(ref s) = msg {
                if s == "crash" {
                    return Err("crash loop item".into());
                }
            }
            Ok(Value::Null)
        });

        let sup = Supervisor::start(config, vec![spec]).expect("Supervisor starts");
        assert!(sup.is_alive());

        // Induce tight crash loop (crash 4 times rapidly)
        for _ in 0..4 {
            for _ in 0..50 {
                if let Some(child) = sup.get_child("sup_rl_looper") {
                    if child.is_alive() {
                        let _ = child.ask(
                            Value::String("crash".into()),
                            Some(Duration::from_millis(100)),
                        );
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        // Wait for rate limiter to trigger supervisor shutdown
        let mut supervisor_halted = false;
        for _ in 0..100 {
            if !sup.is_alive() {
                supervisor_halted = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }

        assert!(
            supervisor_halted,
            "Supervisor must halt when restart rate limit is exceeded"
        );
        assert!(matches!(sup.status(), SupervisorStatus::Failed(_)));

        // Verify children are also terminated
        for (_, status) in sup.children() {
            assert!(
                !status.is_alive(),
                "All children must be terminated upon supervisor failure"
            );
        }
        sup.stop();
        let _ = sup.join();
    }

    #[test]
    fn test_supervision_clean_termination_cascade() {
        let config = SupervisorConfig::default();

        let spec1 = ChildSpec::from_fn("sup_casc_a", |_| Ok(Value::Null));
        let spec2 = ChildSpec::from_fn("sup_casc_b", |_| Ok(Value::Null));
        let spec3 = ChildSpec::from_fn("sup_casc_c", |_| Ok(Value::Null));

        let sup = Supervisor::start(config, vec![spec1, spec2, spec3]).unwrap();

        let child_a = sup.get_child("sup_casc_a").unwrap();
        let child_b = sup.get_child("sup_casc_b").unwrap();
        let child_c = sup.get_child("sup_casc_c").unwrap();

        assert!(sup.is_alive());
        assert!(child_a.is_alive());
        assert!(child_b.is_alive());
        assert!(child_c.is_alive());

        // Cascade stop
        let stopped = sup.stop();
        assert!(stopped, "First call to stop() must return true");
        assert!(!sup.stop(), "Subsequent calls to stop() must return false");

        assert!(!sup.is_alive());
        assert!(!child_a.is_alive());
        assert!(!child_b.is_alive());
        assert!(!child_c.is_alive());

        assert_eq!(child_a.status(), ActorStatus::Stopped);
        assert_eq!(child_b.status(), ActorStatus::Stopped);
        assert_eq!(child_c.status(), ActorStatus::Stopped);
        let _ = sup.join();
    }

    #[test]
    fn test_supervision_dynamic_child_lifecycle() {
        let sup = Supervisor::start(SupervisorConfig::default(), Vec::new()).unwrap();
        assert!(sup.is_alive());

        // 1. get_child on non-existent returns None
        assert!(sup.get_child("sup_dyn_worker").is_none());
        assert!(sup.children().is_empty());

        // 2. start_child adds dynamic child
        let spec =
            ChildSpec::from_stateful_fn("sup_dyn_worker", Value::Int(10), |state, msg| {
                match (state, msg) {
                    (Value::Int(s), Value::Int(n)) => Ok(Value::Int(s + n)),
                    (st, _) => Ok(st),
                }
            });

        let child = sup
            .start_child(spec.clone())
            .expect("Dynamic child spawn should succeed");
        assert_eq!(child.id(), "sup_dyn_worker");
        assert!(child.is_alive());

        // Verify child is accessible and operational
        let child_ref = sup.get_child("sup_dyn_worker").expect("Must resolve child");
        assert_eq!(child_ref.ask(Value::Int(5), None).unwrap(), Value::Int(15));

        // 3. Duplicate start_child fails
        let dup_res = sup.start_child(spec);
        assert!(dup_res.is_err());
        assert!(matches!(
            dup_res.unwrap_err(),
            SupervisorError::ChildAlreadyExists(_)
        ));

        // 4. children() lists dyn_worker
        let child_list = sup.children();
        assert_eq!(child_list.len(), 1);
        assert_eq!(child_list[0].0, "sup_dyn_worker");
        assert_eq!(child_list[0].1, ActorStatus::Running);

        // 5. restart_child resets state
        let restarted = sup
            .restart_child("sup_dyn_worker")
            .expect("Manual restart should succeed");
        assert!(restarted.is_alive());
        assert_eq!(
            restarted.ask(Value::Int(0), None).unwrap(),
            Value::Int(10),
            "State must reset to 10"
        );

        // 6. stop_child terminates and removes child
        sup.stop_child("sup_dyn_worker")
            .expect("Stop child must succeed");
        assert!(sup.get_child("sup_dyn_worker").is_none());
        assert!(sup.children().is_empty());
        assert!(!restarted.is_alive());

        // 7. Duplicate stop_child or restart_child returns ChildNotFound
        assert!(matches!(
            sup.stop_child("sup_dyn_worker").unwrap_err(),
            SupervisorError::ChildNotFound(_)
        ));
        assert!(matches!(
            sup.restart_child("sup_dyn_worker").unwrap_err(),
            SupervisorError::ChildNotFound(_)
        ));

        sup.stop();
        let _ = sup.join();
    }

    #[test]
    fn test_concurrent_start_stop_children_no_deadlock() {
        let sup = Supervisor::start(SupervisorConfig::default(), Vec::new()).unwrap();
        let running = Arc::new(AtomicBool::new(true));

        // Thread 1: Mutator (start_child and stop_child in loop)
        let s1 = Arc::clone(&sup);
        let r1 = Arc::clone(&running);
        let h1 = thread::spawn(move || {
            let mut idx = 0;
            while r1.load(Ordering::Relaxed) {
                idx += 1;
                let id = format!("sup_deadlock_test_worker_{}", idx % 10);
                let spec = ChildSpec::from_fn(id.clone(), |msg| Ok(msg));
                if s1.start_child(spec).is_ok() {
                    let _ = s1.stop_child(&id);
                }
            }
        });

        // Thread 2: Inspector (children() in loop)
        let s2 = Arc::clone(&sup);
        let r2 = Arc::clone(&running);
        let h2 = thread::spawn(move || {
            while r2.load(Ordering::Relaxed) {
                let _ = s2.children();
                let _ = s2.len();
            }
        });

        thread::sleep(Duration::from_millis(500));
        running.store(false, Ordering::Relaxed);
        h1.join().unwrap();
        h2.join().unwrap();
        sup.stop();
        let _ = sup.join();
    }
}

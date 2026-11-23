use std::sync::atomic::{AtomicBool as StdAtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;

/// Lock-free atomic 64-bit signed integer.
#[derive(Debug, Clone)]
pub struct AtomicInt {
    pub inner: Arc<AtomicI64>,
}

impl AtomicInt {
    pub fn new(val: i64) -> Self {
        Self {
            inner: Arc::new(AtomicI64::new(val)),
        }
    }

    pub fn load(&self, order: Ordering) -> i64 {
        self.inner.load(order)
    }

    pub fn store(&self, val: i64, order: Ordering) {
        self.inner.store(val, order);
    }

    pub fn fetch_add(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_add(val, order)
    }

    pub fn fetch_sub(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_sub(val, order)
    }

    pub fn compare_exchange(
        &self,
        current: i64,
        new: i64,
        success: Ordering,
        failure: Ordering,
    ) -> Result<i64, i64> {
        self.inner.compare_exchange(current, new, success, failure)
    }

    pub fn swap(&self, val: i64, order: Ordering) -> i64 {
        self.inner.swap(val, order)
    }

    pub fn fetch_and(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_and(val, order)
    }

    pub fn fetch_or(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_or(val, order)
    }

    pub fn fetch_xor(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_xor(val, order)
    }

    pub fn fetch_max(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_max(val, order)
    }

    pub fn fetch_min(&self, val: i64, order: Ordering) -> i64 {
        self.inner.fetch_min(val, order)
    }
}

/// Lock-free atomic boolean flag.
#[derive(Debug, Clone)]
pub struct AtomicBool {
    pub inner: Arc<StdAtomicBool>,
}

impl AtomicBool {
    pub fn new(val: bool) -> Self {
        Self {
            inner: Arc::new(StdAtomicBool::new(val)),
        }
    }

    pub fn load(&self, order: Ordering) -> bool {
        self.inner.load(order)
    }

    pub fn store(&self, val: bool, order: Ordering) {
        self.inner.store(val, order);
    }

    pub fn compare_exchange(
        &self,
        current: bool,
        new: bool,
        success: Ordering,
        failure: Ordering,
    ) -> Result<bool, bool> {
        self.inner.compare_exchange(current, new, success, failure)
    }

    pub fn swap(&self, val: bool, order: Ordering) -> bool {
        self.inner.swap(val, order)
    }

    pub fn fetch_and(&self, val: bool, order: Ordering) -> bool {
        self.inner.fetch_and(val, order)
    }

    pub fn fetch_or(&self, val: bool, order: Ordering) -> bool {
        self.inner.fetch_or(val, order)
    }

    pub fn fetch_xor(&self, val: bool, order: Ordering) -> bool {
        self.inner.fetch_xor(val, order)
    }
}

// =========================================================================
// Memory Ordering Validation & Parsing
// =========================================================================

pub fn parse_ordering(
    val: Option<&Value>,
    default: Ordering,
    span: Span,
) -> Result<Ordering, RuntimeError> {
    match val {
        None => Ok(default),
        Some(Value::String(s)) => parse_ordering_str(s, span),
        Some(other) => Err(RuntimeError::new(format!(
            "Memory ordering must be a string, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn parse_ordering_str(s: &str, span: Span) -> Result<Ordering, RuntimeError> {
    let normalized = s.trim().to_lowercase().replace('-', "_");
    match normalized.as_str() {
        "seq_cst" | "seqcst" => Ok(Ordering::SeqCst),
        "acquire" | "acq" => Ok(Ordering::Acquire),
        "release" | "rel" => Ok(Ordering::Release),
        "relaxed" => Ok(Ordering::Relaxed),
        "acq_rel" | "acqrel" => Ok(Ordering::AcqRel),
        other => Err(RuntimeError::new(format!(
            "Unknown memory ordering '{}'. Valid orderings are 'seq_cst', 'acquire', 'release', 'relaxed', 'acq_rel'",
            other
        ))
        .at(span)),
    }
}

pub fn validate_load_ordering(order: Ordering, span: Span) -> Result<Ordering, RuntimeError> {
    match order {
        Ordering::Release => Err(RuntimeError::new(
            "Invalid memory ordering for load: 'release' ordering is not permitted for load operations".into(),
        )
        .at(span)),
        Ordering::AcqRel => Err(RuntimeError::new(
            "Invalid memory ordering for load: 'acq_rel' ordering is not permitted for load operations".into(),
        )
        .at(span)),
        valid => Ok(valid),
    }
}

pub fn validate_store_ordering(order: Ordering, span: Span) -> Result<Ordering, RuntimeError> {
    match order {
        Ordering::Acquire => Err(RuntimeError::new(
            "Invalid memory ordering for store: 'acquire' ordering is not permitted for store operations".into(),
        )
        .at(span)),
        Ordering::AcqRel => Err(RuntimeError::new(
            "Invalid memory ordering for store: 'acq_rel' ordering is not permitted for store operations".into(),
        )
        .at(span)),
        valid => Ok(valid),
    }
}

pub fn validate_cas_orderings(
    success: Ordering,
    failure: Option<Ordering>,
    span: Span,
) -> Result<(Ordering, Ordering), RuntimeError> {
    let fail = match failure {
        Some(f) => f,
        None => match success {
            Ordering::SeqCst => Ordering::SeqCst,
            Ordering::AcqRel => Ordering::Acquire,
            Ordering::Release => Ordering::Relaxed,
            Ordering::Acquire => Ordering::Acquire,
            Ordering::Relaxed => Ordering::Relaxed,
            _ => Ordering::SeqCst,
        },
    };

    if fail == Ordering::Release {
        return Err(RuntimeError::new(
            "Invalid memory ordering for compare_exchange: failure ordering cannot be 'release'".into(),
        )
        .at(span));
    }
    if fail == Ordering::AcqRel {
        return Err(RuntimeError::new(
            "Invalid memory ordering for compare_exchange: failure ordering cannot be 'acq_rel'".into(),
        )
        .at(span));
    }

    let valid = match (success, fail) {
        (Ordering::Relaxed, Ordering::Relaxed) => true,
        (Ordering::Release, Ordering::Relaxed) => true,
        (Ordering::Acquire, Ordering::Relaxed | Ordering::Acquire) => true,
        (Ordering::AcqRel, Ordering::Relaxed | Ordering::Acquire) => true,
        (Ordering::SeqCst, Ordering::Relaxed | Ordering::Acquire | Ordering::SeqCst) => true,
        _ => false,
    };

    if !valid {
        return Err(RuntimeError::new(format!(
            "Invalid memory ordering combination for compare_exchange: failure ordering ({:?}) cannot be stronger than success ordering ({:?})",
            fail, success
        ))
        .at(span));
    }

    Ok((success, fail))
}

// =========================================================================
// Method Recognition & Helpers
// =========================================================================

pub fn is_atomic_int_method(name: &str) -> bool {
    matches!(
        name,
        "load"
            | "store"
            | "fetch_add"
            | "fetchAdd"
            | "fetch_sub"
            | "fetchSub"
            | "compare_and_swap"
            | "compareAndSwap"
            | "cas"
            | "compare_exchange"
            | "compareExchange"
            | "swap"
            | "fetch_and"
            | "fetchAnd"
            | "fetch_or"
            | "fetchOr"
            | "fetch_xor"
            | "fetchXor"
            | "fetch_max"
            | "fetchMax"
            | "fetch_min"
            | "fetchMin"
    )
}

pub fn is_atomic_bool_method(name: &str) -> bool {
    matches!(
        name,
        "load"
            | "store"
            | "compare_and_swap"
            | "compareAndSwap"
            | "cas"
            | "compare_exchange"
            | "compareExchange"
            | "swap"
            | "fetch_and"
            | "fetchAnd"
            | "fetch_or"
            | "fetchOr"
            | "fetch_xor"
            | "fetchXor"
    )
}

fn value_to_i64(val: &Value, span: Span, op: &str) -> Result<i64, RuntimeError> {
    match val {
        Value::Int(i) => Ok(*i),
        Value::Float(f) | Value::Number(f) if f.fract() == 0.0 => Ok(*f as i64),
        other => Err(RuntimeError::new(format!(
            "{} expected integer, got {}",
            op,
            other.type_name()
        ))
        .at(span)),
    }
}

fn value_to_bool(val: &Value, span: Span, op: &str) -> Result<bool, RuntimeError> {
    match val {
        Value::Bool(b) => Ok(*b),
        other => Err(RuntimeError::new(format!(
            "{} expected boolean, got {}",
            op,
            other.type_name()
        ))
        .at(span)),
    }
}

// =========================================================================
// Method Dispatch Implementation
// =========================================================================

pub fn eval_atomic_int_bound_method(
    atom: &Arc<AtomicI64>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    match method {
        "load" => {
            if args.len() > 1 {
                return Err(
                    RuntimeError::new("AtomicInt.load([order]) expects 0 or 1 argument".into())
                        .at(span),
                );
            }
            let order = parse_ordering(args.first(), Ordering::SeqCst, span)?;
            let order = validate_load_ordering(order, span)?;
            Ok(Value::Int(atom.load(order)))
        }
        "store" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.store(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.store")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let order = validate_store_ordering(order, span)?;
            atom.store(val, order);
            Ok(Value::Null)
        }
        "fetch_add" | "fetchAdd" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_add(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_add")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_add(val, order);
            Ok(Value::Int(prev))
        }
        "fetch_sub" | "fetchSub" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_sub(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_sub")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_sub(val, order);
            Ok(Value::Int(prev))
        }
        "compare_and_swap" | "compareAndSwap" | "cas" | "compare_exchange" | "compareExchange" => {
            if args.len() < 2 || args.len() > 4 {
                return Err(RuntimeError::new(
                    "AtomicInt.compare_and_swap(expected, desired, [success_order], [failure_order]) expects 2 to 4 arguments".into(),
                )
                .at(span));
            }
            let expected = value_to_i64(&args[0], span, "AtomicInt.compare_and_swap")?;
            let desired = value_to_i64(&args[1], span, "AtomicInt.compare_and_swap")?;
            let succ = parse_ordering(args.get(2), Ordering::SeqCst, span)?;
            let fail = if let Some(arg3) = args.get(3) {
                Some(parse_ordering(Some(arg3), Ordering::SeqCst, span)?)
            } else {
                None
            };
            let (succ, fail) = validate_cas_orderings(succ, fail, span)?;
            match atom.compare_exchange(expected, desired, succ, fail) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(_) => Ok(Value::Bool(false)),
            }
        }
        "swap" => {
            if args.is_empty() || args.len() > 2 {
                return Err(
                    RuntimeError::new("AtomicInt.swap(val, [order]) expects 1 or 2 arguments".into())
                        .at(span),
                );
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.swap")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.swap(val, order);
            Ok(Value::Int(prev))
        }
        "fetch_and" | "fetchAnd" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_and(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_and")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_and(val, order);
            Ok(Value::Int(prev))
        }
        "fetch_or" | "fetchOr" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_or(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_or")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_or(val, order);
            Ok(Value::Int(prev))
        }
        "fetch_xor" | "fetchXor" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_xor(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_xor")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_xor(val, order);
            Ok(Value::Int(prev))
        }
        "fetch_max" | "fetchMax" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_max(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_max")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_max(val, order);
            Ok(Value::Int(prev))
        }
        "fetch_min" | "fetchMin" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicInt.fetch_min(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_i64(&args[0], span, "AtomicInt.fetch_min")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_min(val, order);
            Ok(Value::Int(prev))
        }
        other => Err(RuntimeError::new(format!(
            "AtomicInt has no property or method '{}'",
            other
        ))
        .at(span)),
    }
}

pub fn eval_atomic_bool_bound_method(
    atom: &Arc<StdAtomicBool>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    match method {
        "load" => {
            if args.len() > 1 {
                return Err(
                    RuntimeError::new("AtomicBool.load([order]) expects 0 or 1 argument".into())
                        .at(span),
                );
            }
            let order = parse_ordering(args.first(), Ordering::SeqCst, span)?;
            let order = validate_load_ordering(order, span)?;
            Ok(Value::Bool(atom.load(order)))
        }
        "store" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicBool.store(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_bool(&args[0], span, "AtomicBool.store")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let order = validate_store_ordering(order, span)?;
            atom.store(val, order);
            Ok(Value::Null)
        }
        "compare_and_swap" | "compareAndSwap" | "cas" | "compare_exchange" | "compareExchange" => {
            if args.len() < 2 || args.len() > 4 {
                return Err(RuntimeError::new(
                    "AtomicBool.compare_and_swap(expected, desired, [success_order], [failure_order]) expects 2 to 4 arguments".into(),
                )
                .at(span));
            }
            let expected = value_to_bool(&args[0], span, "AtomicBool.compare_and_swap")?;
            let desired = value_to_bool(&args[1], span, "AtomicBool.compare_and_swap")?;
            let succ = parse_ordering(args.get(2), Ordering::SeqCst, span)?;
            let fail = if let Some(arg3) = args.get(3) {
                Some(parse_ordering(Some(arg3), Ordering::SeqCst, span)?)
            } else {
                None
            };
            let (succ, fail) = validate_cas_orderings(succ, fail, span)?;
            match atom.compare_exchange(expected, desired, succ, fail) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(_) => Ok(Value::Bool(false)),
            }
        }
        "swap" => {
            if args.is_empty() || args.len() > 2 {
                return Err(
                    RuntimeError::new("AtomicBool.swap(val, [order]) expects 1 or 2 arguments".into())
                        .at(span),
                );
            }
            let val = value_to_bool(&args[0], span, "AtomicBool.swap")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.swap(val, order);
            Ok(Value::Bool(prev))
        }
        "fetch_and" | "fetchAnd" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicBool.fetch_and(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_bool(&args[0], span, "AtomicBool.fetch_and")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_and(val, order);
            Ok(Value::Bool(prev))
        }
        "fetch_or" | "fetchOr" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicBool.fetch_or(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_bool(&args[0], span, "AtomicBool.fetch_or")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_or(val, order);
            Ok(Value::Bool(prev))
        }
        "fetch_xor" | "fetchXor" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(
                    "AtomicBool.fetch_xor(val, [order]) expects 1 or 2 arguments".into(),
                )
                .at(span));
            }
            let val = value_to_bool(&args[0], span, "AtomicBool.fetch_xor")?;
            let order = parse_ordering(args.get(1), Ordering::SeqCst, span)?;
            let prev = atom.fetch_xor(val, order);
            Ok(Value::Bool(prev))
        }
        other => Err(RuntimeError::new(format!(
            "AtomicBool has no property or method '{}'",
            other
        ))
        .at(span)),
    }
}

// =========================================================================
// Builtin Constructor Functions
// =========================================================================

pub fn sync_atomic_int(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let initial = match args.len() {
        0 => 0,
        1 => value_to_i64(&args[0], span, "AtomicInt")?,
        n => {
            return Err(RuntimeError::new(format!(
                "AtomicInt expects 0 or 1 argument, got {}",
                n
            ))
            .at(span));
        }
    };
    Ok(Value::AtomicInt(Arc::new(AtomicI64::new(initial))))
}

pub fn sync_atomic_bool(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let initial = match args.len() {
        0 => false,
        1 => value_to_bool(&args[0], span, "AtomicBool")?,
        n => {
            return Err(RuntimeError::new(format!(
                "AtomicBool expects 0 or 1 argument, got {}",
                n
            ))
            .at(span));
        }
    };
    Ok(Value::AtomicBool(Arc::new(StdAtomicBool::new(initial))))
}

// =========================================================================
// Module Level Functions
// =========================================================================

pub fn sync_load(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() || args.len() > 2 {
        return Err(RuntimeError::new("sync.load(atom, [order]) expects 1 or 2 arguments".into()).at(span));
    }
    match &args[0] {
        Value::AtomicInt(a) => {
            let rest = args[1..].to_vec();
            eval_atomic_int_bound_method(a, "load", rest, span)
        }
        Value::AtomicBool(b) => {
            let rest = args[1..].to_vec();
            eval_atomic_bool_bound_method(b, "load", rest, span)
        }
        other => Err(RuntimeError::new(format!(
            "sync.load expects AtomicInt or AtomicBool, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn sync_store(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new("sync.store(atom, val, [order]) expects 2 or 3 arguments".into()).at(span));
    }
    match &args[0] {
        Value::AtomicInt(a) => {
            let rest = args[1..].to_vec();
            eval_atomic_int_bound_method(a, "store", rest, span)
        }
        Value::AtomicBool(b) => {
            let rest = args[1..].to_vec();
            eval_atomic_bool_bound_method(b, "store", rest, span)
        }
        other => Err(RuntimeError::new(format!(
            "sync.store expects AtomicInt or AtomicBool, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn sync_fetch_add(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new("sync.fetch_add(atom, val, [order]) expects 2 or 3 arguments".into()).at(span));
    }
    match &args[0] {
        Value::AtomicInt(a) => {
            let rest = args[1..].to_vec();
            eval_atomic_int_bound_method(a, "fetch_add", rest, span)
        }
        other => Err(RuntimeError::new(format!(
            "sync.fetch_add expects AtomicInt, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn sync_fetch_sub(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::new("sync.fetch_sub(atom, val, [order]) expects 2 or 3 arguments".into()).at(span));
    }
    match &args[0] {
        Value::AtomicInt(a) => {
            let rest = args[1..].to_vec();
            eval_atomic_int_bound_method(a, "fetch_sub", rest, span)
        }
        other => Err(RuntimeError::new(format!(
            "sync.fetch_sub expects AtomicInt, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

pub fn sync_cas(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(RuntimeError::new(
            "sync.cas(atom, expected, desired, [success_order], [failure_order]) expects 3 to 5 arguments".into(),
        )
        .at(span));
    }
    match &args[0] {
        Value::AtomicInt(a) => {
            let rest = args[1..].to_vec();
            eval_atomic_int_bound_method(a, "compare_and_swap", rest, span)
        }
        Value::AtomicBool(b) => {
            let rest = args[1..].to_vec();
            eval_atomic_bool_bound_method(b, "compare_and_swap", rest, span)
        }
        other => Err(RuntimeError::new(format!(
            "sync.cas expects AtomicInt or AtomicBool, got {}",
            other.type_name()
        ))
        .at(span)),
    }
}

// =========================================================================
// Module Builder: build_sync_module() -> Value
// =========================================================================

pub fn build_sync_module() -> Value {
    let mut map = indexmap::IndexMap::new();

    // Constructors
    map.insert(
        "AtomicInt".to_string(),
        Value::Builtin {
            name: "AtomicInt".to_string(),
            func: sync_atomic_int,
        },
    );
    map.insert(
        "atomic_int".to_string(),
        Value::Builtin {
            name: "atomic_int".to_string(),
            func: sync_atomic_int,
        },
    );
    map.insert(
        "AtomicBool".to_string(),
        Value::Builtin {
            name: "AtomicBool".to_string(),
            func: sync_atomic_bool,
        },
    );
    map.insert(
        "atomic_bool".to_string(),
        Value::Builtin {
            name: "atomic_bool".to_string(),
            func: sync_atomic_bool,
        },
    );

    // Module functions
    map.insert(
        "load".to_string(),
        Value::Builtin {
            name: "load".to_string(),
            func: sync_load,
        },
    );
    map.insert(
        "store".to_string(),
        Value::Builtin {
            name: "store".to_string(),
            func: sync_store,
        },
    );
    map.insert(
        "fetch_add".to_string(),
        Value::Builtin {
            name: "fetch_add".to_string(),
            func: sync_fetch_add,
        },
    );
    map.insert(
        "fetch_sub".to_string(),
        Value::Builtin {
            name: "fetch_sub".to_string(),
            func: sync_fetch_sub,
        },
    );
    map.insert(
        "cas".to_string(),
        Value::Builtin {
            name: "cas".to_string(),
            func: sync_cas,
        },
    );
    map.insert(
        "compare_and_swap".to_string(),
        Value::Builtin {
            name: "compare_and_swap".to_string(),
            func: sync_cas,
        },
    );

    // Memory ordering constants
    map.insert("SEQ_CST".to_string(), Value::String("seq_cst".to_string()));
    map.insert("ACQUIRE".to_string(), Value::String("acquire".to_string()));
    map.insert("RELEASE".to_string(), Value::String("release".to_string()));
    map.insert("RELAXED".to_string(), Value::String("relaxed".to_string()));
    map.insert("ACQ_REL".to_string(), Value::String("acq_rel".to_string()));

    map.insert("seq_cst".to_string(), Value::String("seq_cst".to_string()));
    map.insert("acquire".to_string(), Value::String("acquire".to_string()));
    map.insert("release".to_string(), Value::String("release".to_string()));
    map.insert("relaxed".to_string(), Value::String("relaxed".to_string()));
    map.insert("acq_rel".to_string(), Value::String("acq_rel".to_string()));

    Value::Map(Arc::new(std::sync::RwLock::new(map)))
}

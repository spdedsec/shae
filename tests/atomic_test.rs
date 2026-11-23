use serde_json::json;
use shae::{run, run_vm};

// =========================================================================
// 1. Constructors & Basic State
// =========================================================================

#[test]
fn test_atomic_int_constructors() {
    let script = r#"
let a1 = AtomicInt()
let a2 = AtomicInt(42)
let a3 = atomic_int(100)
[a1.load(), a2.load(), a3.load()]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([0, 42, 100]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([0, 42, 100]));
}

#[test]
fn test_atomic_bool_constructors() {
    let script = r#"
let b1 = AtomicBool()
let b2 = AtomicBool(true)
let b3 = atomic_bool(false)
[b1.load(), b2.load(), b3.load()]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([false, true, false]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([false, true, false]));
}

// =========================================================================
// 2. Load & Store Operations with Memory Orderings
// =========================================================================

#[test]
fn test_atomic_int_load_store_orderings() {
    let script = r#"
let a = AtomicInt(10)
a.store(20)
let v1 = a.load()

a.store(30, "release")
let v2 = a.load("acquire")

a.store(40, "relaxed")
let v3 = a.load("relaxed")

a.store(50, "seq_cst")
let v4 = a.load("seq_cst")

[v1, v2, v3, v4]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([20, 30, 40, 50]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([20, 30, 40, 50]));
}

#[test]
fn test_atomic_bool_load_store_orderings() {
    let script = r#"
let b = AtomicBool(false)
b.store(true)
let v1 = b.load()

b.store(false, "release")
let v2 = b.load("acquire")

b.store(true, "relaxed")
let v3 = b.load("relaxed")

[v1, v2, v3]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([true, false, true]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, false, true]));
}

// =========================================================================
// 3. Memory Ordering Validation & Error Paths
// =========================================================================

#[test]
fn test_invalid_load_orderings_throw_runtime_error() {
    // Release is invalid for load
    let script_rel = r#"
let a = AtomicInt(10)
a.load("release")
"#;
    assert!(run(script_rel).is_err(), "Release load should fail");
    assert!(run_vm(script_rel).is_err(), "VM Release load should fail");

    // AcqRel is invalid for load
    let script_acqrel = r#"
let b = AtomicBool(true)
b.load("acq_rel")
"#;
    assert!(run(script_acqrel).is_err(), "AcqRel load should fail");
    assert!(run_vm(script_acqrel).is_err(), "VM AcqRel load should fail");
}

#[test]
fn test_invalid_store_orderings_throw_runtime_error() {
    // Acquire is invalid for store
    let script_acq = r#"
let a = AtomicInt(10)
a.store(20, "acquire")
"#;
    assert!(run(script_acq).is_err(), "Acquire store should fail");
    assert!(run_vm(script_acq).is_err(), "VM Acquire store should fail");

    // AcqRel is invalid for store
    let script_acqrel = r#"
let b = AtomicBool(false)
b.store(true, "acq_rel")
"#;
    assert!(run(script_acqrel).is_err(), "AcqRel store should fail");
    assert!(run_vm(script_acqrel).is_err(), "VM AcqRel store should fail");
}

#[test]
fn test_unknown_ordering_throws_runtime_error() {
    let script = r#"
let a = AtomicInt(10)
a.load("bogus_ordering")
"#;
    assert!(run(script).is_err());
    assert!(run_vm(script).is_err());
}

// =========================================================================
// 4. Read-Modify-Write: fetch_add, fetch_sub
// =========================================================================

#[test]
fn test_atomic_int_fetch_add_sub() {
    let script = r#"
let a = AtomicInt(100)
let prev1 = a.fetch_add(25)
let cur1 = a.load()

let prev2 = a.fetch_sub(50, "acq_rel")
let cur2 = a.load("acquire")

let prev3 = a.fetch_add(-15)
let cur3 = a.load()

[prev1, cur1, prev2, cur2, prev3, cur3]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([100, 125, 125, 75, 75, 60]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([100, 125, 125, 75, 75, 60]));
}

// =========================================================================
// 5. Compare-And-Swap (CAS) & compare_exchange
// =========================================================================

#[test]
fn test_atomic_int_cas() {
    let script = r#"
let a = AtomicInt(10)

// Successful CAS: expected 10, set to 20
let ok1 = a.compare_and_swap(10, 20)
let v1 = a.load()

// Failed CAS: expected 10 (now 20), value unchanged
let ok2 = a.cas(10, 30)
let v2 = a.load()

// Successful compare_exchange with orderings: expected 20, set to 50
let ok3 = a.compare_exchange(20, 50, "seq_cst", "seq_cst")
let v3 = a.load()

[ok1, v1, ok2, v2, ok3, v3]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([true, 20, false, 20, true, 50]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, 20, false, 20, true, 50]));
}

#[test]
fn test_atomic_bool_cas() {
    let script = r#"
let b = AtomicBool(false)

// Successful CAS: expected false, set to true
let ok1 = b.compare_and_swap(false, true)
let v1 = b.load()

// Failed CAS: expected false (now true), value unchanged
let ok2 = b.cas(false, false)
let v2 = b.load()

// Successful CAS: expected true, set to false
let ok3 = b.compare_exchange(true, false)
let v3 = b.load()

[ok1, v1, ok2, v2, ok3, v3]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(ast_val.to_json(), json!([true, true, false, true, true, false]));

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_val.to_json(), json!([true, true, false, true, true, false]));
}

#[test]
fn test_invalid_cas_failure_orderings_throw_runtime_error() {
    // Failure order cannot be Release
    let script_rel_fail = r#"
let a = AtomicInt(10)
a.compare_exchange(10, 20, "seq_cst", "release")
"#;
    assert!(run(script_rel_fail).is_err());
    assert!(run_vm(script_rel_fail).is_err());

    // Failure order cannot be stronger than success order
    let script_stronger = r#"
let a = AtomicInt(10)
a.compare_exchange(10, 20, "relaxed", "seq_cst")
"#;
    assert!(run(script_stronger).is_err());
    assert!(run_vm(script_stronger).is_err());
}

// =========================================================================
// 6. Swap and Bitwise / Numeric Atomic Primitives
// =========================================================================

#[test]
fn test_atomic_int_swap_and_bitwise() {
    let script = r#"
let a = AtomicInt(0b1010)

// Swap
let prev_swap = a.swap(0b1100)
let v_swap = a.load()

// Bitwise AND: 0b1100 & 0b1010 = 0b1000 (8)
let prev_and = a.fetch_and(0b1010)
let v_and = a.load()

// Bitwise OR: 0b1000 | 0b0011 = 0b1011 (11)
let prev_or = a.fetch_or(0b0011)
let v_or = a.load()

// Bitwise XOR: 0b1011 ^ 0b0001 = 0b1010 (10)
let prev_xor = a.fetch_xor(0b0001)
let v_xor = a.load()

// fetch_max: max(10, 25) = 25
let prev_max = a.fetch_max(25)
let v_max = a.load()

// fetch_min: min(25, 5) = 5
let prev_min = a.fetch_min(5)
let v_min = a.load()

[prev_swap, v_swap, prev_and, v_and, prev_or, v_or, prev_xor, v_xor, prev_max, v_max, prev_min, v_min]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!([10, 12, 12, 8, 8, 11, 11, 10, 10, 25, 25, 5])
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        json!([10, 12, 12, 8, 8, 11, 11, 10, 10, 25, 25, 5])
    );
}

#[test]
fn test_atomic_bool_swap_and_bitwise() {
    let script = r#"
let b = AtomicBool(false)

let prev_swap = b.swap(true)
let v_swap = b.load()

let prev_and = b.fetch_and(false)
let v_and = b.load()

let prev_or = b.fetch_or(true)
let v_or = b.load()

let prev_xor = b.fetch_xor(true)
let v_xor = b.load()

[prev_swap, v_swap, prev_and, v_and, prev_or, v_or, prev_xor, v_xor]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!([false, true, true, false, false, true, true, false])
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        json!([false, true, true, false, false, true, true, false])
    );
}

// =========================================================================
// 7. std:sync Module Import & Ordering Constants
// =========================================================================

#[test]
fn test_std_sync_module_import_and_constants() {
    let script = r#"
let sync = use "std:sync"

let a = sync.AtomicInt(100)
sync.store(a, 200, sync.RELEASE)
let v = sync.load(a, sync.ACQUIRE)
let prev = sync.fetch_add(a, 50, sync.ACQ_REL)
let cas_ok = sync.cas(a, 250, 300, sync.SEQ_CST)

[
    sync.SEQ_CST,
    sync.ACQUIRE,
    sync.RELEASE,
    sync.RELAXED,
    sync.ACQ_REL,
    v,
    prev,
    cas_ok,
    a.load()
]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!(["seq_cst", "acquire", "release", "relaxed", "acq_rel", 200, 200, true, 300])
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        json!(["seq_cst", "acquire", "release", "relaxed", "acq_rel", 200, 200, true, 300])
    );
}

// =========================================================================
// 8. Equality (Arc::ptr_eq) & Truthiness
// =========================================================================

#[test]
fn test_atomic_equality_and_truthiness() {
    let script = r#"
let a1 = AtomicInt(42)
let a2 = AtomicInt(42)
let a3 = a1

let eq_diff = (a1 == a2)
let eq_same = (a1 == a3)

let zero = AtomicInt(0)
let nonzero = AtomicInt(5)
let t_zero = "falsy"
if zero {
    t_zero = "truthy"
}
let t_nonzero = "falsy"
if nonzero {
    t_nonzero = "truthy"
}

let b_false = AtomicBool(false)
let b_true = AtomicBool(true)
let t_bfalse = "falsy"
if b_false {
    t_bfalse = "truthy"
}
let t_btrue = "falsy"
if b_true {
    t_btrue = "truthy"
}

[eq_diff, eq_same, t_zero, t_nonzero, t_bfalse, t_btrue]
"#;
    let ast_val = run(script).expect("AST execution failed");
    assert_eq!(
        ast_val.to_json(),
        json!([false, true, "falsy", "truthy", "falsy", "truthy"])
    );

    let vm_val = run_vm(script).expect("VM execution failed");
    assert_eq!(
        vm_val.to_json(),
        json!([false, true, "falsy", "truthy", "falsy", "truthy"])
    );
}

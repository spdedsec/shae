use shae::value::Value;
use shae::{run, run_vm};

#[test]
fn test_ffi_sizeof() {
    let script = r#"
use { sizeof } from "std:ffi"

let s_i32 = sizeof("i32")
let s_i64 = sizeof("i64")
let s_f32 = sizeof("f32")
let s_f64 = sizeof("f64")
let s_ptr = sizeof("ptr")

assert_eq(s_i32, 4)
assert_eq(s_i64, 8)
assert_eq(s_f32, 4)
assert_eq(s_f64, 8)
assert_eq(s_ptr, 8)

"sizeof_ok"
"#;
    let res = run(script).expect("AST execution failed");
    assert_eq!(res, Value::String("sizeof_ok".into()));

    let res_vm = run_vm(script).expect("VM execution failed");
    assert_eq!(res_vm, Value::String("sizeof_ok".into()));
}

#[test]
fn test_ffi_memory_ops() {
    let script = r#"
use { alloc, free, write_i32, read_i32, write_i64, read_i64 } from "std:ffi"

let ptr = alloc(16)
assert(ptr > 0)

write_i32(ptr, 1337)
let val32 = read_i32(ptr)
assert_eq(val32, 1337)

write_i64(ptr, 9876543210)
let val64 = read_i64(ptr)
assert_eq(val64, 9876543210)

free(ptr, 16)
"mem_ok"
"#;
    let res = run(script).expect("AST execution failed");
    assert_eq!(res, Value::String("mem_ok".into()));

    let res_vm = run_vm(script).expect("VM execution failed");
    assert_eq!(res_vm, Value::String("mem_ok".into()));
}

#[test]
fn test_ffi_libc_invocation() {
    let script = r#"
use { open, load } from "std:ffi"

let libc = open("")
let abs_fn = libc.fn("abs", ["i32"], "i32")
let result = abs_fn(-42)
assert_eq(result, 42)

let strlen_fn = load(libc, "strlen", ["string"], "i64")
let len = strlen_fn("shae language")
assert_eq(len, 13)

"libc_ok"
"#;
    let res = run(script).expect("AST execution failed");
    assert_eq!(res, Value::String("libc_ok".into()));

    let res_vm = run_vm(script).expect("VM execution failed");
    assert_eq!(res_vm, Value::String("libc_ok".into()));
}

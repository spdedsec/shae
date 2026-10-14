use shae::parse_source;
use shae::stdlib::siphash_2_4;
use shae::value::Value;
use shae::vm::{InterpretResult, VM};

fn run_shae(code: &str) -> Value {
    let program = parse_source(code).expect("Parse error in test script");
    let chunk = shae::compiler::Compiler::new()
        .compile_program(&program)
        .expect("Compile error");
    let mut vm = VM::new();
    match vm.interpret(chunk) {
        InterpretResult::Ok(val) => val,
        InterpretResult::RuntimeError(e) => panic!("Runtime error: {:?}", e),
        InterpretResult::CompileError(e) => panic!("Compile error: {:?}", e),
    }
}

#[test]
fn test_siphash_2_4_reference_vector() {
    // Standard SipHash-2-4 reference test vector:
    // Key: 00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f
    // Input: 15 bytes: 00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e
    // Expected 64-bit digest: 0xa129ca61496b71e2
    let key: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    let msg: [u8; 15] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
    ];
    let hash = siphash_2_4(key, &msg);
    assert_eq!(hash, 0xa129ca61496b71e2);
}

#[test]
fn test_crypto_siphash_in_shae() {
    let script = r#"
let crypto = use "std:crypto"

let key = "0123456789abcdef"
let data = "Shae Language High-Performance Systems Scripting"

let h1 = crypto.siphash(key, data)
let h2 = crypto.siphash(key, data)
let h3 = crypto.siphash("differentkey1234", data)

let hex1 = crypto.siphash_hex(key, data)

[h1 == h2, h1 != h3, len(hex1) == 16]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Bool(true));
            assert_eq!(guard[2], Value::Bool(true));
        }
        _ => panic!("Expected Array, got {:?}", res),
    }
}

#[test]
fn test_collections_first_class_set() {
    let script = r#"
let collections = use "std:collections"

// 1. Create set from array with duplicates
let s = collections.set([10, 20, 30, 20, 10])
assert_eq(s.size(), 3)
assert_eq(s.size, 3)

// 2. Query membership
assert_eq(s.has(10), true)
assert_eq(s.has(20), true)
assert_eq(s.has(99), false)
assert_eq(s.contains(30), true)

// 3. Mutate set
let added = s.add(40)
assert_eq(added, true)
assert_eq(s.size(), 4)
assert_eq(s.has(40), true)

let added_again = s.add(40)
assert_eq(added_again, false)
assert_eq(s.size(), 4)

let deleted = s.delete(10)
assert_eq(deleted, true)
assert_eq(s.has(10), false)
assert_eq(s.size(), 3)

// 4. Convert to array
let arr = s.to_array()
assert_eq(arr, [20, 30, 40])

// 5. Set operations
let other = collections.set([30, 40, 50, 60])

let u = s.union(other)
assert_eq(u.to_array(), [20, 30, 40, 50, 60])

let inter = s.intersect(other)
assert_eq(inter.to_array(), [30, 40])

let diff = s.difference(other)
assert_eq(diff.to_array(), [20])

// 6. Clear
s.clear()
assert_eq(s.size(), 0)
assert_eq(s.to_array(), [])

"set_success"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("set_success".into()));
}

#[test]
fn test_collections_linear_time_dedup() {
    let script = r#"
let collections = use "std:collections"

// Deduplicate 2,000 numbers in O(N)
let numbers = []
let i = 0
while i < 1000 {
    numbers.push(i % 50)
    numbers.push(i % 100)
    i += 1
}

let u = collections.unique(numbers)
assert_eq(len(u), 100)

"dedup_success"
"#;
    let res = run_shae(script);
    assert_eq!(res, Value::String("dedup_success".into()));
}

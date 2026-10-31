use shae::run;
use shae::run_vm;
use shae::value::Value;

#[test]
fn test_buffer_creation_properties_and_direct_indexing_vm() {
    let script = r#"
        let buf = use "std:buffer"
        let b = buf.alloc(16)
        b[0] = 42
        b[15] = 255
        [b.len, b.offset, b[0], b[15]]
    "#;
    let res = run_vm(script).expect("VM buffer creation & indexing should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(16));
            assert_eq!(guard[1], Value::Int(0));
            assert_eq!(guard[2], Value::Int(42));
            assert_eq!(guard[3], Value::Int(255));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_buffer_creation_properties_and_direct_indexing_ast() {
    let script = r#"
        let buf = use "std:buffer"
        let b = buf.alloc(16)
        b[0] = 42
        b[15] = 255
        [b.len, b.offset, b[0], b[15]]
    "#;
    let res = run(script).expect("AST buffer creation & indexing should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(16));
            assert_eq!(guard[1], Value::Int(0));
            assert_eq!(guard[2], Value::Int(42));
            assert_eq!(guard[3], Value::Int(255));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_zero_copy_slicing_and_mutation_vm() {
    let script = r#"
        let buf = use "std:buffer"
        let parent = buf.alloc(10)
        parent[3] = 111

        let slice = parent.slice(2, 6)
        let val1 = slice[1] // parent[3] = 111

        // Mutate slice, verify parent reflects mutation
        slice[0] = 88
        let parent_val = parent[2] // should be 88

        // Mutate parent, verify slice reflects mutation
        parent[4] = 99
        let slice_val = slice[2] // should be 99

        [val1, parent_val, slice_val, slice.len, slice.offset, parent.len, parent.offset]
    "#;
    let res = run_vm(script).expect("VM zero-copy slicing should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(111));
            assert_eq!(guard[1], Value::Int(88));
            assert_eq!(guard[2], Value::Int(99));
            assert_eq!(guard[3], Value::Int(4));
            assert_eq!(guard[4], Value::Int(2));
            assert_eq!(guard[5], Value::Int(10));
            assert_eq!(guard[6], Value::Int(0));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_zero_copy_slicing_and_mutation_ast() {
    let script = r#"
        let buf = use "std:buffer"
        let parent = buf.alloc(10)
        parent[3] = 111

        let slice = parent.slice(2, 6)
        let val1 = slice[1]

        slice[0] = 88
        let parent_val = parent[2]

        parent[4] = 99
        let slice_val = slice[2]

        [val1, parent_val, slice_val, slice.len, slice.offset, parent.len, parent.offset]
    "#;
    let res = run(script).expect("AST zero-copy slicing should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(111));
            assert_eq!(guard[1], Value::Int(88));
            assert_eq!(guard[2], Value::Int(99));
            assert_eq!(guard[3], Value::Int(4));
            assert_eq!(guard[4], Value::Int(2));
            assert_eq!(guard[5], Value::Int(10));
            assert_eq!(guard[6], Value::Int(0));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_hex_utf8_conversions_and_constructors_vm() {
    let script = r#"
        let buf = use "std:buffer"
        let b1 = buf.from_hex("48656c6c6f")
        let str1 = b1.to_utf8()

        let b2 = buf.from_utf8("World")
        let hex2 = b2.to_hex()

        let b3 = buf.from([1, 2, 3, 4])
        let bytes3 = b3.to_bytes()

        let b4 = buf.concat([b1, b2])
        let str4 = b4.to_utf8()

        [str1, hex2, bytes3, str4]
    "#;
    let res = run_vm(script).expect("VM hex and utf8 conversions should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::String("Hello".into()));
            assert_eq!(guard[1], Value::String("576f726c64".into()));
            assert_eq!(guard[3], Value::String("HelloWorld".into()));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_hex_utf8_conversions_and_constructors_ast() {
    let script = r#"
        let buf = use "std:buffer"
        let b1 = buf.from_hex("48656c6c6f")
        let str1 = b1.to_utf8()

        let b2 = buf.from_utf8("World")
        let hex2 = b2.to_hex()

        let b3 = buf.from([1, 2, 3, 4])
        let bytes3 = b3.to_bytes()

        let b4 = buf.concat([b1, b2])
        let str4 = b4.to_utf8()

        [str1, hex2, bytes3, str4]
    "#;
    let res = run(script).expect("AST hex and utf8 conversions should succeed");
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::String("Hello".into()));
            assert_eq!(guard[1], Value::String("576f726c64".into()));
            assert_eq!(guard[3], Value::String("HelloWorld".into()));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_endian_reads_writes_dual_engine() {
    let script = r#"
        let buf = use "std:buffer"
        let b = buf.alloc(64)

        b.write_u8(0, 200)
        b.write_i8(1, -50)
        b.write_u16_le(2, 60000)
        b.write_u16_be(4, 60000)
        b.write_i16_le(6, -12345)
        b.write_i16_be(8, -12345)
        b.write_u32_le(10, 3000000000)
        b.write_u32_be(14, 3000000000)
        b.write_i32_le(18, -100000000)
        b.write_i32_be(22, -100000000)
        b.write_u64_le(26, 9000000000000000)
        b.write_u64_be(34, 9000000000000000)
        b.write_f32_le(42, 3.14)
        b.write_f64_be(46, 2.718281828459)

        let r_u8 = b.read_u8(0)
        let r_i8 = b.read_i8(1)
        let r_u16_le = b.read_u16_le(2)
        let r_u16_be = b.read_u16_be(4)
        let r_i16_le = b.read_i16_le(6)
        let r_i16_be = b.read_i16_be(8)
        let r_u32_le = b.read_u32_le(10)
        let r_u32_be = b.read_u32_be(14)
        let r_i32_le = b.read_i32_le(18)
        let r_i32_be = b.read_i32_be(22)
        let r_u64_le = b.read_u64_le(26)
        let r_u64_be = b.read_u64_be(34)
        let r_f32_le = b.read_f32_le(42)
        let r_f64_be = b.read_f64_be(46)

        [r_u8, r_i8, r_u16_le, r_u16_be, r_i16_le, r_i16_be, r_u32_le, r_u32_be, r_i32_le, r_i32_be, r_u64_le, r_u64_be, r_f32_le, r_f64_be]
    "#;

    let res_vm = run_vm(script).expect("VM endian reads/writes should succeed");
    let res_ast = run(script).expect("AST endian reads/writes should succeed");

    assert_eq!(res_vm, res_ast, "VM and AST results must be identical");

    match res_vm {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(200));
            assert_eq!(guard[1], Value::Int(-50));
            assert_eq!(guard[2], Value::Int(60000));
            assert_eq!(guard[3], Value::Int(60000));
            assert_eq!(guard[4], Value::Int(-12345));
            assert_eq!(guard[5], Value::Int(-12345));
            assert_eq!(guard[6], Value::Int(3000000000));
            assert_eq!(guard[7], Value::Int(3000000000));
            assert_eq!(guard[8], Value::Int(-100000000));
            assert_eq!(guard[9], Value::Int(-100000000));
            assert_eq!(guard[10], Value::Int(9000000000000000));
            assert_eq!(guard[11], Value::Int(9000000000000000));
            match &guard[12] {
                Value::Float(f) => assert!((f - 3.14).abs() < 0.01),
                other => panic!("Expected float for f32, got {:?}", other),
            }
            match &guard[13] {
                Value::Float(f) => assert!((f - 2.718281828459).abs() < 0.0001),
                other => panic!("Expected float for f64, got {:?}", other),
            }
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_struct_packing_unpacking_calcsize_dual_engine() {
    let script = r#"
        let buf = use "std:buffer"

        let sz1 = buf.calcsize("<I2h4s")
        let sz2 = buf.calcsize(">2d4B")

        let packed = buf.pack("<I2h4s", 4294967295, -1200, 3200, "Rust")
        let unpacked = buf.unpack("<I2h4s", packed)

        let dest = buf.alloc(20)
        let written = buf.pack_into("<I2h4s", dest, 4, 100000, 10, 20, "Shae")
        let unpacked_into = buf.unpack("<I2h4s", dest, 4)

        [sz1, sz2, unpacked[0], unpacked[1], unpacked[2], unpacked[3], written, unpacked_into[0], unpacked_into[3]]
    "#;

    let res_vm = run_vm(script).expect("VM struct packing should succeed");
    let res_ast = run(script).expect("AST struct packing should succeed");

    assert_eq!(res_vm, res_ast, "VM and AST struct pack/unpack results must match");

    match res_vm {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Int(12));
            assert_eq!(guard[1], Value::Int(20));
            assert_eq!(guard[2], Value::Int(4294967295));
            assert_eq!(guard[3], Value::Int(-1200));
            assert_eq!(guard[4], Value::Int(3200));
            assert_eq!(guard[5], Value::String("Rust".into()));
            assert_eq!(guard[6], Value::Int(12));
            assert_eq!(guard[7], Value::Int(100000));
            assert_eq!(guard[8], Value::String("Shae".into()));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

#[test]
fn test_buffer_equality_with_array_dual_engine() {
    let script = r#"
        let buf = use "std:buffer"
        let b = buf.from([10, 20, 30, 40])
        let arr = [10, 20, 30, 40]
        let arr_diff = [10, 20, 30, 41]

        [b == arr, arr == b, b == arr_diff, b == b.copy()]
    "#;
    let res_vm = run_vm(script).expect("VM buffer equality should succeed");
    let res_ast = run(script).expect("AST buffer equality should succeed");

    assert_eq!(res_vm, res_ast);

    match res_vm {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            assert_eq!(guard[0], Value::Bool(true));
            assert_eq!(guard[1], Value::Bool(true));
            assert_eq!(guard[2], Value::Bool(false));
            assert_eq!(guard[3], Value::Bool(true));
        }
        other => panic!("Expected array, got {:?}", other),
    }
}

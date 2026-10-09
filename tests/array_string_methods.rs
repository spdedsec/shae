use shae::run;
use shae::value::Value;

#[test]
fn test_array_find_some_every() {
    let script = r#"
        let nums = [1, 2, 3, 4, 5, 6]
        let found = nums.find(fn(x) { x > 3 && x % 2 == 0 })
        let not_found = nums.find(fn(x) { x > 100 })
        let has_even = nums.some(fn(x) { x % 2 == 0 })
        let has_neg = nums.some(fn(x) { x < 0 })
        let all_pos = nums.every(fn(x) { x > 0 })
        let all_even = nums.every(fn(x) { x % 2 == 0 })

        [found, not_found, has_even, has_neg, all_pos, all_even]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(4));
            assert_eq!(b[1], Value::Null);
            assert_eq!(b[2], Value::Bool(true));
            assert_eq!(b[3], Value::Bool(false));
            assert_eq!(b[4], Value::Bool(true));
            assert_eq!(b[5], Value::Bool(false));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_array_flat_join_reverse_slice() {
    let script = r#"
        let nested = [1, [2, 3], [4, [5]], 6]
        let flattened = nested.flat()

        let fruits = ["apple", "banana", "cherry"]
        let joined_default = fruits.join()
        let joined_dash = fruits.join(" - ")

        let to_rev = [1, 2, 3, 4]
        to_rev.reverse()

        let items = [10, 20, 30, 40, 50]
        let s1 = items.slice(1, 4)
        let s2 = items.slice(-2)

        [flattened.len, joined_default, joined_dash, to_rev, s1, s2]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Int(6)); // 1, 2, 3, 4, [5], 6
            assert_eq!(b[1], Value::String("apple,banana,cherry".into()));
            assert_eq!(b[2], Value::String("apple - banana - cherry".into()));
            match &b[3] {
                Value::Array(ra) => {
                    let rb = ra.read().unwrap();
                    assert_eq!(rb[0], Value::Int(4));
                    assert_eq!(rb[1], Value::Int(3));
                    assert_eq!(rb[2], Value::Int(2));
                    assert_eq!(rb[3], Value::Int(1));
                }
                _ => panic!("Expected reversed array"),
            }
            match &b[4] {
                Value::Array(s1) => {
                    let sb = s1.read().unwrap();
                    assert_eq!(sb[0], Value::Int(20));
                    assert_eq!(sb[1], Value::Int(30));
                    assert_eq!(sb[2], Value::Int(40));
                }
                _ => panic!("Expected slice"),
            }
            match &b[5] {
                Value::Array(s2) => {
                    let sb = s2.read().unwrap();
                    assert_eq!(sb[0], Value::Int(40));
                    assert_eq!(sb[1], Value::Int(50));
                }
                _ => panic!("Expected slice"),
            }
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_string_starts_ends_contains() {
    let script = r#"
        let text = "hello shae language"
        let sw1 = text.starts_with("hello")
        let sw2 = text.starts_with("shae")
        let ew1 = text.ends_with("language")
        let ew2 = text.ends_with("shae")
        let c1 = text.contains("shae")
        let c2 = text.contains("rust")

        [sw1, sw2, ew1, ew2, c1, c2]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::Bool(true));
            assert_eq!(b[1], Value::Bool(false));
            assert_eq!(b[2], Value::Bool(true));
            assert_eq!(b[3], Value::Bool(false));
            assert_eq!(b[4], Value::Bool(true));
            assert_eq!(b[5], Value::Bool(false));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_string_pad_start_lines_chars() {
    let script = r#"
        let padded = "42".pad_start(5, "0")
        let multiline = "line1\nline2\nline3"
        let l = multiline.lines()
        let c = "shae".chars()

        [padded, l, c]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("00042".into()));
            match &b[1] {
                Value::Array(lines_arr) => {
                    let lb = lines_arr.read().unwrap();
                    assert_eq!(lb.len(), 3);
                    assert_eq!(lb[0], Value::String("line1".into()));
                    assert_eq!(lb[1], Value::String("line2".into()));
                    assert_eq!(lb[2], Value::String("line3".into()));
                }
                _ => panic!("Expected lines array"),
            }
            match &b[2] {
                Value::Array(chars_arr) => {
                    let cb = chars_arr.read().unwrap();
                    assert_eq!(cb.len(), 4);
                    assert_eq!(cb[0], Value::String("s".into()));
                    assert_eq!(cb[1], Value::String("h".into()));
                    assert_eq!(cb[2], Value::String("a".into()));
                    assert_eq!(cb[3], Value::String("e".into()));
                }
                _ => panic!("Expected chars array"),
            }
        }
        _ => panic!("Expected array"),
    }
}

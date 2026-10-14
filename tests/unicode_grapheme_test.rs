use shae::parse_source;
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
        InterpretResult::CompileError => panic!("Compile error"),
    }
}

#[test]
fn test_unicode_uax29_graphemes_and_runes() {
    let script = r#"
let u = use "std:unicode"
// Family ZWJ sequence (man + ZWJ + woman + ZWJ + girl + ZWJ + boy)
let family = "👨‍👩‍👧‍👦"
// Thumbs up + medium skin tone modifier
let thumbs = "👍🏽"
// Combining accent: e + combining acute accent
let e_acute = u.from_codepoints([101, 769])
// Flag sequence: US
let flag = "🇺🇸"

// ASCII string
let ascii = "hello"

[
    family.grapheme_len() == 1,
    len(family.codepoints()) == 7,
    family.byte_len() == 25,

    thumbs.grapheme_len() == 1,
    len(thumbs.codepoints()) == 2,

    e_acute.grapheme_len() == 1,
    len(e_acute.codepoints()) == 2,

    flag.grapheme_len() == 1,
    len(flag.codepoints()) == 2,

    ascii.is_ascii() == true,
    family.is_ascii() == false
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Failed condition at index {}", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

#[test]
fn test_unicode_slice_graphemes_preserves_clusters() {
    let script = r#"
// String with mixed ASCII, multi-cluster emojis, and combined accents
let text = "Hi 👨‍👩‍👧‍👦, welcome to 🇺🇸! Great 👍🏽 job!"

// Slicing by grapheme indices without splitting emojis
let part1 = text.slice_graphemes(0, 4)
let emoji_only = text.slice_graphemes(3, 4)
let flag_only = text.slice_graphemes(17, 18)
let tail = text.slice_graphemes(-5)

[
    part1 == "Hi 👨‍👩‍👧‍👦",
    emoji_only == "👨‍👩‍👧‍👦",
    flag_only == "🇺🇸",
    tail == " job!"
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Condition at index {} failed", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

#[test]
fn test_std_unicode_module() {
    let script = r#"
let u = use "std:unicode"

let s = "Rust🦀Shae"
let g = u.graphemes(s)
let gl = u.grapheme_len(s)
let bl = u.byte_len(s)

let cps = u.codepoints("Shae")
let reconstructed = u.from_codepoints(cps)

let cat_letter = u.category("A")
let cat_number = u.category("9")
let cat_punct = u.category("!")

[
    gl == 9,
    bl == 12,
    reconstructed == "Shae",
    cat_letter == "Letter",
    cat_number == "Number",
    cat_punct == "Punctuation",
    u.is_ascii("Shae") == true,
    u.is_ascii("Shae🦀") == false
]
"#;
    let res = run_shae(script);
    match res {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            for (idx, val) in guard.iter().enumerate() {
                assert_eq!(*val, Value::Bool(true), "Failed condition at index {}", idx);
            }
        }
        _ => panic!("Expected array, got {:?}", res),
    }
}

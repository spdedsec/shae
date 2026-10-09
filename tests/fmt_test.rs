use shae::fmt::format_source;
use shae::run;

#[test]
fn test_format_basic_let_and_arithmetic() {
    let unformatted = "let   a=10+5*2\nlet b = (a-2) / 3\n";
    let formatted = format_source(unformatted).expect("Formatting should succeed");
    assert_eq!(formatted, "let a = 10 + 5 * 2\nlet b = (a - 2) / 3\n");
}

#[test]
fn test_format_functions_and_indentation() {
    let unformatted = r#"fn add(x,y){
let sum=x+y
return sum
}"#;
    let formatted = format_source(unformatted).expect("Formatting should succeed");
    let expected = "fn add(x, y) {\n    let sum = x + y\n    return sum\n}\n";
    assert_eq!(formatted, expected);
}

#[test]
fn test_format_if_else_chain() {
    let unformatted = r#"if x>10{
print("high")
}else if x>0{
print("low")
}else{
print("zero or negative")
}"#;
    let formatted = format_source(unformatted).expect("Formatting should succeed");
    let expected = "if x > 10 {\n    print(\"high\")\n} else if x > 0 {\n    print(\"low\")\n} else {\n    print(\"zero or negative\")\n}\n";
    assert_eq!(formatted, expected);
}

#[test]
fn test_format_loops() {
    let unformatted = r#"let i = 0
while i < 3 {
print(i)
i = i + 1
}
for item in [1, 2, 3] {
print(item)
}
"#;
    let formatted = format_source(unformatted).expect("Formatting should succeed");
    assert!(formatted.contains("while i < 3 {\n    print(i)\n    i = i + 1\n}"));
    assert!(formatted.contains("for item in [1, 2, 3] {\n    print(item)\n}"));
}

#[test]
fn test_format_struct_and_enum() {
    let unformatted = r#"struct Point { x, y }
enum Status {
Pending,
Done(code, msg)
}
"#;
    let formatted = format_source(unformatted).expect("Formatting should succeed");
    assert!(formatted.contains("struct Point { x, y }"));
    assert!(formatted.contains("enum Status {\n    Pending,\n    Done(code, msg)\n}"));
}

#[test]
fn test_format_destructuring_and_match() {
    let unformatted = r#"let [head, ..tail] = [1, 2, 3]
let { name, role } = user
let status = match score {
90..=100 => "A",
x if x >= 80 => "B",
_ => "C"
}
"#;
    let formatted = format_source(unformatted).expect("Formatting should succeed");
    assert!(formatted.contains("let [head, ..tail] = [1, 2, 3]"));
    assert!(formatted.contains("let { name, role } = user"));
    assert!(formatted.contains(
        "match score {\n    90..=100 => \"A\",\n    x if x >= 80 => \"B\",\n    _ => \"C\"\n}"
    ));
}

#[test]
fn test_format_idempotency_and_semantic_preservation() {
    let script = r#"
fn fib(n) {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

let [a, b, ..rest] = [10, 20, 30, 40]
let result = fib(7) + a + b + rest[0]
result
"#;
    let formatted = format_source(script).expect("First format should succeed");
    let formatted_again = format_source(&formatted).expect("Second format should succeed");
    // Idempotency: Formatting an already formatted string must produce identical output
    assert_eq!(formatted, formatted_again);

    // Semantic preservation: Evaluating both original and formatted produces identical value
    let val_orig = run(script).expect("Original script should run");
    let val_fmt = run(&formatted).expect("Formatted script should run");
    assert_eq!(val_orig, val_fmt);
}

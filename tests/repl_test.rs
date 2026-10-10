use shae::repl::{is_input_complete, ReplState};
use shae::value::Value;

#[test]
fn test_repl_vm_state_persistence() {
    let mut repl = ReplState::new(true);

    // 1. Declare variable
    let res1 = repl.eval_line("let x = 40").expect("let should succeed");
    assert_eq!(res1, Some(Value::Null));

    // 2. Declare function using global/top-level variable
    let res2 = repl.eval_line("fn add(a, b) { a + b }").expect("fn should succeed");
    assert_eq!(res2, Some(Value::Null));

    // 3. Call function using previously declared variable and function
    let res3 = repl.eval_line("add(x, 2)").expect("call should succeed");
    assert_eq!(res3, Some(Value::Int(42)));

    // 4. Inspect user globals
    let globals = repl.user_globals();
    let names: Vec<&str> = globals.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"x"));
    assert!(names.contains(&"add"));

    // 5. Reassign x
    repl.eval_line("x = 100").expect("assign should succeed");
    let res4 = repl.eval_line("add(x, 5)").expect("call after reassign");
    assert_eq!(res4, Some(Value::Int(105)));

    // 6. Reset REPL
    repl.reset();
    let globals_after_reset = repl.user_globals();
    assert!(globals_after_reset.is_empty());
}

#[test]
fn test_repl_ast_mode() {
    let mut repl = ReplState::new(false);
    assert!(!repl.use_vm);

    repl.eval_line("let msg = \"  Hello Shae  \"").expect("eval let in AST");
    let res = repl.eval_line("msg.trim()").expect("eval expr in AST");
    assert_eq!(res, Some(Value::String("Hello Shae".to_string())));
}

#[test]
fn test_repl_input_completeness() {
    // Single line complete
    assert!(is_input_complete("let x = 10"));
    assert!(is_input_complete("1 + 2"));

    // Unclosed braces
    assert!(!is_input_complete("fn test() {"));
    assert!(!is_input_complete("if x > 10 { print(x)"));
    assert!(is_input_complete("fn test() { 123 }"));

    // Unclosed parentheses & brackets
    assert!(!is_input_complete("foo(1, 2,"));
    assert!(is_input_complete("foo(1, 2)"));
    assert!(!is_input_complete("[1, 2, 3,"));
    assert!(is_input_complete("[1, 2, 3]"));

    // Braces inside string literal should not count
    assert!(is_input_complete("let s = \"{not a brace}\""));
    assert!(!is_input_complete("let s = \"unclosed string"));
}

#[test]
fn test_repl_multiline_block_evaluation() {
    let mut repl = ReplState::new(true);
    let block = r#"
fn fib(n) {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}
"#;
    assert!(is_input_complete(block));
    repl.eval_line(block).expect("multi-line function definition should compile");

    let res = repl.eval_line("fib(10)").expect("fib(10) should evaluate");
    assert_eq!(res, Some(Value::Int(55)));
}

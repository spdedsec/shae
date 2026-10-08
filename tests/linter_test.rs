use shae::linter::{DiagnosticSeverity, Linter};
use shae::parse_source;

#[test]
fn test_linter_unused_variables() {
    let script = r#"
fn test_func(used_param, unused_param, _intentional) {
    let used_local = 10
    let unused_local = 20
    let _ignored = 30
    return used_param + used_local
}
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let mut linter = Linter::new();
    let diags = linter.lint_program(&program);

    let unused_warnings: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning && d.message.contains("never"))
        .collect();

    assert_eq!(unused_warnings.len(), 2);
    assert!(unused_warnings.iter().any(|d| d.message.contains("unused_param")));
    assert!(unused_warnings.iter().any(|d| d.message.contains("unused_local")));
    // Verify underscore-prefixed variables were NOT warned
    assert!(!unused_warnings.iter().any(|d| d.message.contains("_intentional")));
    assert!(!unused_warnings.iter().any(|d| d.message.contains("_ignored")));
}

#[test]
fn test_linter_unreachable_code() {
    let script = r#"
fn compute(x) {
    if x > 0 {
        return x * 2
        let dead1 = 100
        print(dead1)
    }
    return 0
    let dead2 = 200
}
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let mut linter = Linter::new();
    let diags = linter.lint_program(&program);

    let unreachable_warnings: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning && d.message.contains("Unreachable code"))
        .collect();

    assert_eq!(unreachable_warnings.len(), 3);
}

#[test]
fn test_linter_variable_shadowing() {
    let script = r#"
let x = 10
fn inner() {
    let x = 20
    let len = 30
    return x + len
}
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let mut linter = Linter::new();
    let diags = linter.lint_program(&program);

    let shadowing_warnings: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning && d.message.contains("shadows"))
        .collect();

    assert_eq!(shadowing_warnings.len(), 2);
    assert!(shadowing_warnings.iter().any(|d| d.message.contains("outer variable")));
    assert!(shadowing_warnings.iter().any(|d| d.message.contains("built-in function")));
}

#[test]
fn test_linter_clean_code_has_no_diagnostics() {
    let script = r#"
fn fib(n) {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

let result = fib(10)
print(result)
"#;
    let program = parse_source(script).expect("Parsing should succeed");
    let mut linter = Linter::new();
    let diags = linter.lint_program(&program);

    assert!(diags.is_empty(), "Expected 0 diagnostics for clean code, got: {:?}", diags);
}

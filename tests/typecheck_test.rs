use shae::lexer;
use shae::parser;
use shae::typecheck::TypeChecker;

fn check_source(source: &str) -> Vec<shae::linter::Diagnostic> {
    let tokens = lexer::tokenize(source).expect("Tokenization should succeed");
    let program = parser::parse(tokens).expect("Parsing should succeed");
    let mut tc = TypeChecker::new();
    tc.check_program(&program).to_vec()
}

#[test]
fn test_valid_typed_functions_and_variables() {
    let src = r#"
        let count: int = 42
        let name: string = "Shae"
        let is_fast: bool = true
        let ratio: float = 3.14

        fn add(a: int, b: int) -> int {
            return a + b
        }

        fn greet(user: string) -> string {
            return "Hello, " + user
        }

        let result: int = add(count, 10)
    "#;
    let diags = check_source(src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for valid typed code, got: {:?}",
        diags
    );
}

#[test]
fn test_type_mismatch_variable_assignment() {
    let src = r#"
        let count: int = "forty-two"
    "#;
    let diags = check_source(src);
    assert!(
        diags.iter().any(|d| d.message.contains("Type mismatch")),
        "Expected type mismatch diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn test_type_mismatch_function_return() {
    let src = r#"
        fn get_id() -> int {
            return "id-12345"
        }
    "#;
    let diags = check_source(src);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Return type mismatch")),
        "Expected return type mismatch diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn test_type_mismatch_function_arguments() {
    let src = r#"
        fn square(n: int) -> int {
            return n * n
        }

        let res = square("three")
    "#;
    let diags = check_source(src);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Argument 1 type mismatch")),
        "Expected argument type mismatch, got: {:?}",
        diags
    );
}

#[test]
fn test_function_arity_mismatch() {
    let src = r#"
        fn add(a: int, b: int) -> int {
            return a + b
        }

        let res = add(1)
    "#;
    let diags = check_source(src);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("expects 2 argument(s)")),
        "Expected arity mismatch, got: {:?}",
        diags
    );
}

#[test]
fn test_struct_field_types_and_property_access() {
    let valid_src = r#"
        struct User {
            name: string,
            age: int,
            active: bool
        }

        let u = User {
            name: "Alice",
            age: 30,
            active: true
        }

        let user_name: string = u.name
        let user_age: int = u.age
    "#;
    let diags = check_source(valid_src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for valid struct usage, got: {:?}",
        diags
    );

    let invalid_field_src = r#"
        struct User {
            name: string,
            age: int
        }

        let u = User {
            name: "Bob",
            age: "twenty"
        }
    "#;
    let diags2 = check_source(invalid_field_src);
    assert!(
        diags2.iter().any(|d| d
            .message
            .contains("Field 'age' on struct 'User' expects 'int'")),
        "Expected field type mismatch, got: {:?}",
        diags2
    );

    let invalid_prop_src = r#"
        struct Point {
            x: int,
            y: int
        }

        let p = Point { x: 1, y: 2 }
        let z = p.z
    "#;
    let diags3 = check_source(invalid_prop_src);
    assert!(
        diags3.iter().any(|d| d
            .message
            .contains("Property 'z' does not exist on struct 'Point'")),
        "Expected property does not exist diagnostic, got: {:?}",
        diags3
    );
}

#[test]
fn test_union_and_nullable_types() {
    let src = r#"
        let val: string | int = "text"
        val = 123

        let opt: string? = null
        opt = "present"
    "#;
    let diags = check_source(src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for union and nullable types, got: {:?}",
        diags
    );
}

#[test]
fn test_arrays_and_maps_types() {
    let src = r#"
        let numbers: [int] = [1, 2, 3, 4]
        let lookup: map<string, int> = { "alpha": 1, "beta": 2 }
    "#;
    let diags = check_source(src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for array and map annotations, got: {:?}",
        diags
    );
}

#[test]
fn test_untyped_code_backward_compatibility() {
    let src = r#"
        let x = 10
        let y = "hello"
        fn compute(a, b) {
            return a + b
        }
        let z = compute(x, 20)
    "#;
    let diags = check_source(src);
    assert_eq!(
        diags.len(),
        0,
        "Untyped code should pass type checking with 0 errors, got: {:?}",
        diags
    );
}

#[test]
fn test_typed_code_runs_on_ast_and_vm() {
    use shae::eval::Evaluator;
    use shae::vm::VM;

    let src = r#"
        struct Calc { factor: int }

        fn compute(c: Calc, x: int) -> int {
            return c.factor * x
        }

        let c = Calc { factor: 5 }
        let res: int = compute(c, 10)
        res
    "#;

    // 1. Check typechecker passes
    let diags = check_source(src);
    assert_eq!(diags.len(), 0);

    // 2. Run on AST Evaluator
    let tokens = lexer::tokenize(src).unwrap();
    let program = parser::parse(tokens).unwrap();
    let mut eval = Evaluator::new();
    let ast_res = eval
        .eval_program(&program)
        .expect("AST evaluation should pass");
    assert_eq!(ast_res, shae::value::Value::Int(50));

    // 3. Run on Bytecode VM
    let mut vm = VM::new();
    let comp = shae::compiler::Compiler::new();
    let chunk = comp.compile_program(&program).unwrap();
    let vm_res = vm.interpret(chunk);
    assert_eq!(
        vm_res,
        shae::vm::InterpretResult::Ok(shae::value::Value::Int(50))
    );
}

#[test]
fn test_formatter_preserves_type_annotations() {
    let src = "fn calculate(x: int, y: int) -> int {\n  return x + y\n}\n";
    let tokens = lexer::tokenize(src).unwrap();
    let program = parser::parse(tokens).unwrap();
    let fmt = shae::fmt::Formatter::new();
    let formatted = fmt.format_program(&program);
    assert!(
        formatted.contains("x: int") && formatted.contains("-> int"),
        "Formatted output must preserve type annotations: {}",
        formatted
    );
}

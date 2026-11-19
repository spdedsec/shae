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

#[test]
fn test_null_type_annotation() {
    // Valid : null bindings and return types
    let valid_src = r#"
        let empty: null = null
        let opt: string | null = null
        fn return_null() -> null {
            return null
        }
        let res: null = return_null()
    "#;
    let diags = check_source(valid_src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for valid null annotations, got: {:?}",
        diags
    );

    // Invalid assignment to null
    let invalid_src = r#"
        let empty: null = 42
    "#;
    let diags2 = check_source(invalid_src);
    assert!(
        diags2.iter().any(|d| d.message.contains("Type mismatch")),
        "Expected type mismatch for non-null assigned to null, got: {:?}",
        diags2
    );

    // Invalid return from null-declared function
    let invalid_ret_src = r#"
        fn bad_null() -> null {
            return "not_null"
        }
    "#;
    let diags3 = check_source(invalid_ret_src);
    assert!(
        diags3
            .iter()
            .any(|d| d.message.contains("Return type mismatch")),
        "Expected return type mismatch for non-null return, got: {:?}",
        diags3
    );
}

#[test]
fn test_implicit_return_type_validation() {
    // Valid implicit return
    let valid_src = r#"
        fn add(a: int, b: int) -> int {
            a + b
        }
        let res: int = add(5, 10)
    "#;
    let diags = check_source(valid_src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for valid implicit return, got: {:?}",
        diags
    );

    // Invalid implicit return in FnDef
    let invalid_fn_src = r#"
        fn compute() -> int {
            "invalid_return_type"
        }
    "#;
    let diags2 = check_source(invalid_fn_src);
    assert!(
        diags2.iter().any(|d| d
            .message
            .contains("Return type mismatch: expected 'int', found 'string'")),
        "Expected return type mismatch for invalid implicit return, got: {:?}",
        diags2
    );

    // Invalid implicit return in Lambda
    let invalid_lambda_src = r#"
        let f = fn(x: int) -> int {
            "not_an_int"
        }
    "#;
    let diags3 = check_source(invalid_lambda_src);
    assert!(
        diags3.iter().any(|d| d
            .message
            .contains("Return type mismatch: expected 'int', found 'string'")),
        "Expected return type mismatch in lambda, got: {:?}",
        diags3
    );

    // Valid implicit return in Lambda
    let valid_lambda_src = r#"
        let square = fn(x: int) -> int {
            x * x
        }
        let res: int = square(6)
    "#;
    let diags4 = check_source(valid_lambda_src);
    assert_eq!(
        diags4.len(),
        0,
        "Expected 0 diagnostics for valid lambda implicit return, got: {:?}",
        diags4
    );
}

#[test]
fn test_variadic_and_optional_builtins() {
    // Variadic print, println, dbg with 0, 1, and many arguments
    let variadic_src = r#"
        print()
        print("one")
        print("one", 2, true, [1, 2], { "k": "v" })
        println()
        println("status:", 200, "OK")
        dbg("inspecting", 42)
    "#;
    let diags = check_source(variadic_src);
    assert_eq!(
        diags.len(),
        0,
        "Expected 0 diagnostics for variadic print/println/dbg, got: {:?}",
        diags
    );

    // Range with 1, 2, and 3 arguments
    let range_valid_src = r#"
        let r1 = range(10)
        let r2 = range(1, 10)
        let r3 = range(0, 10, 2)
    "#;
    let diags_range = check_source(range_valid_src);
    assert_eq!(
        diags_range.len(),
        0,
        "Expected 0 diagnostics for valid range calls, got: {:?}",
        diags_range
    );

    // Range with invalid arity
    let range_invalid_src = r#"
        let r0 = range()
    "#;
    let diags_r0 = check_source(range_invalid_src);
    assert!(
        diags_r0
            .iter()
            .any(|d| d.message.contains("expects 1 to 3 argument(s)")),
        "Expected arity mismatch for range(), got: {:?}",
        diags_r0
    );

    // Assert and assert_eq with 1/2 and 2/3 args
    let assert_valid_src = r#"
        assert(true)
        assert(true, "Condition must be true")
        assert_eq(1, 1)
        assert_eq("a", "a", "Strings must match")
    "#;
    let diags_assert = check_source(assert_valid_src);
    assert_eq!(
        diags_assert.len(),
        0,
        "Expected 0 diagnostics for valid assert/assert_eq calls, got: {:?}",
        diags_assert
    );

    // Assert with wrong argument type
    let assert_bad_type = r#"
        assert("not_a_bool")
    "#;
    let diags_assert_bad = check_source(assert_bad_type);
    assert!(
        diags_assert_bad
            .iter()
            .any(|d| d.message.contains("Argument 1 type mismatch")),
        "Expected type mismatch for assert non-bool argument, got: {:?}",
        diags_assert_bad
    );
}

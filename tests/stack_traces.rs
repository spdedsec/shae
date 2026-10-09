use shae::run;

#[test]
fn test_stack_trace() {
    let script = r#"
        fn a() { b() }
        fn b() { c() }
        fn c() { d() }
        a()
    "#;
    let err = run(script).unwrap_err();
    let err_str = shae::render_error(&err, script);

    println!("ERR STR:\n{}", err_str);
    assert!(err_str.contains("Stack Trace:"));
    assert!(err_str.contains("at c"));
    assert!(err_str.contains("at b"));
    assert!(err_str.contains("at a"));
}

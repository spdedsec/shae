use shae::run;
use shae::value::Value;

#[test]
fn test_match_arm_guards() {
    let script = r#"
        fn classify(n) {
            match n {
                x if x > 100 => "huge",
                x if x > 10 => "medium",
                x if x > 0 => "small",
                _ => "non-positive"
            }
        }

        [classify(500), classify(50), classify(5), classify(-3)]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("huge".into()));
            assert_eq!(b[1], Value::String("medium".into()));
            assert_eq!(b[2], Value::String("small".into()));
            assert_eq!(b[3], Value::String("non-positive".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_range_patterns() {
    let script = r#"
        fn category(age) {
            match age {
                0..=2 => "toddler",
                3..13 => "child",
                13..=19 => "teen",
                20..=64 => "adult",
                65..=150 => "senior",
                _ => "invalid"
            }
        }

        [category(2), category(3), category(12), category(13), category(19), category(20), category(70)]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("toddler".into()));
            assert_eq!(b[1], Value::String("child".into()));
            assert_eq!(b[2], Value::String("child".into()));
            assert_eq!(b[3], Value::String("teen".into()));
            assert_eq!(b[4], Value::String("teen".into()));
            assert_eq!(b[5], Value::String("adult".into()));
            assert_eq!(b[6], Value::String("senior".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_range_patterns_with_guards() {
    let script = r#"
        fn evaluate(score) {
            match score {
                1..=100 if score % 10 == 0 => "decade",
                1..=100 => "normal",
                _ => "out of bounds"
            }
        }

        [evaluate(50), evaluate(47), evaluate(150)]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("decade".into()));
            assert_eq!(b[1], Value::String("normal".into()));
            assert_eq!(b[2], Value::String("out of bounds".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_negative_range_patterns() {
    let script = r#"
        fn temp_zone(t) {
            match t {
                -50..=0 => "freezing",
                1..=20 => "mild",
                21..=50 => "hot",
                _ => "extreme"
            }
        }

        [temp_zone(-10), temp_zone(0), temp_zone(15), temp_zone(35)]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("freezing".into()));
            assert_eq!(b[1], Value::String("freezing".into()));
            assert_eq!(b[2], Value::String("mild".into()));
            assert_eq!(b[3], Value::String("hot".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_wildcard_in_variant_bindings() {
    let script = r#"
        enum Result {
            Ok(value),
            Err(code, msg)
        }

        let r1 = Result.Ok(42)
        let r2 = Result.Err(404, "Not Found")
        let r3 = Result.Err(500, "Internal Error")

        fn describe(r) {
            match r {
                Result.Ok(_) => "has value",
                Result.Err(404, _) => "not found",
                Result.Err(_, msg) => "error: " + msg
            }
        }

        [describe(r1), describe(r2), describe(r3)]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("has value".into()));
            assert_eq!(b[1], Value::String("not found".into()));
            assert_eq!(b[2], Value::String("error: Internal Error".into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_variant_guards_and_bindings() {
    let script = r#"
        enum Task {
            Priority(level, title),
            Routine(title)
        }

        let t1 = Task.Priority(10, "Emergency Fix")
        let t2 = Task.Priority(2, "Refactor Docs")
        let t3 = Task.Routine("Drink Water")

        fn handle_task(t) {
            match t {
                Task.Priority(lvl, title) if lvl >= 5 => "URGENT: " + title,
                Task.Priority(lvl, title) => "Normal: " + title,
                Task.Routine(title) => "Routine: " + title
            }
        }

        [handle_task(t1), handle_task(t2), handle_task(t3)]
    "#;
    let res = run(script).expect("Execution should succeed");
    match res {
        Value::Array(arr) => {
            let b = arr.read().unwrap();
            assert_eq!(b[0], Value::String("URGENT: Emergency Fix".into()));
            assert_eq!(b[1], Value::String("Normal: Refactor Docs".into()));
            assert_eq!(b[2], Value::String("Routine: Drink Water".into()));
        }
        _ => panic!("Expected array"),
    }
}

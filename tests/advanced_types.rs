use serde_json::json;
use shae::run;

#[test]
fn test_structs() {
    let script = r#"
        struct User { name, age, is_admin }
        
        let u = User {
            name: "Alice",
            age: 25,
            is_admin: true
        }
        
        u.age = 26
        u.age
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(26));
}

#[test]
fn test_enums_and_pattern_matching() {
    let script = r#"
        enum Status {
            Pending(),
            Active(days),
            Banned(reason, days)
        }
        
        let s1 = Status.Active(42)
        let s2 = Status.Banned("spam", 99)
        let s3 = Status.Pending()
        
        fn check_status(s) {
            match s {
                Status.Active(d) => "active for " + str(d) + " days",
                Status.Banned(r, d) => "banned for " + str(r),
                Status.Pending() => "waiting",
                _ => "unknown"
            }
        }
        
        check_status(s1)
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!("active for 42 days"));
}

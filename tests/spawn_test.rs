use serde_json::json;
use shae::run;
use std::fs;
use std::thread;
use std::time::Duration;

#[test]
fn test_spawn() {
    let _ = fs::remove_file("spawn_test.txt");
    let script = r#"
        fn worker() {
            write("spawn_test.txt", "done")
        }
        spawn(worker)
        "init"
    "#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!("init"));

    // Wait for the spawned thread to execute
    let mut content = String::new();
    for _ in 0..100 {
        if let Ok(c) = fs::read_to_string("spawn_test.txt")
            && !c.is_empty()
        {
            content = c;
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }
    assert_eq!(content, "done");
    let _ = fs::remove_file("spawn_test.txt");
}

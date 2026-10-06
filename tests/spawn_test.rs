use shae::run;
use serde_json::json;
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
    thread::sleep(Duration::from_millis(50));
    let content = fs::read_to_string("spawn_test.txt").unwrap_or_default();
    assert_eq!(content, "done");
    let _ = fs::remove_file("spawn_test.txt");
}

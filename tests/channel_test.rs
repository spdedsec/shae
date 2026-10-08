use serde_json::json;
use shae::run;

#[test]
fn test_channel_basic_send_recv_methods() {
    let script = r#"
let ch = channel()
ch.send(42)
ch.send("shae")
let a = ch.recv()
let b = ch.recv()
[a, b, ch.len()]
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!([42, "shae", 0]));
}

#[test]
fn test_channel_builtin_functions() {
    let script = r#"
let ch = channel()
send(ch, 100)
send(ch, 200)
let v1 = recv(ch)
let v2 = recv(ch)
[v1, v2]
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!([100, 200]));
}

#[test]
fn test_channel_concurrency_producer_consumer() {
    let script = r#"
let ch = channel()

let t1 = spawn(fn() {
    ch.send(10)
    ch.send(20)
    ch.send(30)
})

let sum = ch.recv() + ch.recv() + ch.recv()
join(t1)
sum
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(60));
}

#[test]
fn test_bounded_channel_capacity_and_blocking() {
    let script = r#"
let ch = channel(2)
let cap = ch.capacity()

ch.send("item1")
ch.send("item2")
let len_before = ch.len()

let t = spawn(fn() {
    // This send will wait until the consumer receives an item
    ch.send("item3")
})

let first = ch.recv()
let second = ch.recv()
let third = ch.recv()
join(t)

[cap, len_before, first, second, third]
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(
        val.to_json(),
        json!([2, 2, "item1", "item2", "item3"])
    );
}

#[test]
fn test_channel_try_recv() {
    let script = r#"
let ch = channel()
let empty_val = ch.tryRecv()

ch.send(999)
let filled_val = ch.tryRecv()
let after_val = ch.tryRecv()

[empty_val, filled_val, after_val]
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!([null, 999, null]));
}

#[test]
fn test_channel_recv_timeout() {
    let script = r#"
let ch = channel()
let timed_out = ch.recv(15) // Wait 15ms on empty channel

ch.send("quick")
let got_quick = ch.recv(100)

[timed_out, got_quick]
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!([null, "quick"]));
}

#[test]
fn test_channel_close_and_drain() {
    let script = r#"
let ch = channel()
ch.send("first")
ch.send("second")
ch.close()

let r1 = ch.recv()
let r2 = ch.recv()
let r3 = ch.recv() // Closed and drained: returns null

[r1, r2, r3]
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(["first", "second", null]));
}

#[test]
fn test_channel_send_on_closed_fails() {
    let script = r#"
let ch = channel()
ch.close()
ch.send(123)
"#;
    let res = run(script);
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Cannot send on a closed channel"));
}

#[test]
fn test_multi_producer_multi_consumer() {
    let script = r#"
let jobs = channel(10)
let results = channel(10)

// Spawn 2 workers
let w1 = spawn(fn() {
    let j = jobs.recv()
    results.send(j * 10)
})
let w2 = spawn(fn() {
    let j = jobs.recv()
    results.send(j * 10)
})

jobs.send(3)
jobs.send(7)

let res1 = results.recv()
let res2 = results.recv()
join(w1)
join(w2)

res1 + res2
"#;
    let val = run(script).expect("Execution failed");
    assert_eq!(val.to_json(), json!(100));
}

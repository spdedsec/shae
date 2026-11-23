use serde_json::json;
use shae::{run, run_vm};

#[test]
fn test_unbuffered_rendezvous_direct_handover() {
    let script = r#"
let ch = chan(0)

let t = spawn(fn() {
    ch <- 42
})

let val = <-ch
join(t)
val
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!(42));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!(42));
}

#[test]
fn test_channel_operator_syntax_send_and_recv() {
    let script = r#"
let ch = chan(2)
ch <- 10
ch <- 20
let a = <-ch
let b = <-ch
[a, b]
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!([10, 20]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!([10, 20]));
}

#[test]
fn test_buffered_channel_fifo_ordering() {
    let script = r#"
let ch = chan(5)
ch <- "alpha"
ch <- "beta"
ch <- "gamma"
let x = <-ch
let y = <-ch
let z = <-ch
[x, y, z, ch.len()]
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!(["alpha", "beta", "gamma", 0]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!(["alpha", "beta", "gamma", 0]));
}

#[test]
fn test_select_nonblocking_default() {
    let script = r#"
let ch1 = chan(0)
let ch2 = chan(0)
let branch = "none"

select {
    case m = <-ch1 => {
        branch = "ch1: " + m
    }
    case m = <-ch2 => {
        branch = "ch2: " + m
    }
    default => {
        branch = "default"
    }
}

branch
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("default"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("default"));
}

#[test]
fn test_select_multi_channel_blocking() {
    let script = r#"
let ch1 = chan(0)
let ch2 = chan(0)

let t = spawn(fn() {
    // Send to ch2
    ch2 <- "message from ch2"
})

let result = ""
select {
    case msg = <-ch1 => {
        result = "got1: " + msg
    }
    case msg = <-ch2 => {
        result = "got2: " + msg
    }
}
join(t)
result
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("got2: message from ch2"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("got2: message from ch2"));
}

#[test]
fn test_select_send_case() {
    let script = r#"
let ch = chan(1)
let branch = ""

select {
    case ch <- "outbound" => {
        branch = "sent"
    }
    default => {
        branch = "default"
    }
}

let received = <-ch
[branch, received]
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!(["sent", "outbound"]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!(["sent", "outbound"]));
}

#[test]
fn test_select_timer_timeout() {
    let script = r#"
let ch = chan(0)
let outcome = ""

select {
    case m = <-ch => {
        outcome = "channel: " + m
    }
    case <-timer(25) => {
        outcome = "timeout"
    }
}

outcome
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("timeout"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("timeout"));
}

#[test]
fn test_select_fairness_distribution() {
    let script = r#"
let ch1 = chan(200)
let ch2 = chan(200)

let i = 0
while i < 100 {
    ch1 <- 1
    ch2 <- 2
    i = i + 1
}

let count1 = 0
let count2 = 0
let n = 0

while n < 100 {
    select {
        case <-ch1 => {
            count1 = count1 + 1
        }
        case <-ch2 => {
            count2 = count2 + 1
        }
    }
    n = n + 1
}

// Both channels should be selected a substantial fraction of the time
let both_selected = count1 > 20 && count2 > 20
both_selected
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!(true));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!(true));
}

#[test]
fn test_select_expression_syntax() {
    let script = r#"
let ch = chan(1)
ch <- "fast"

let val = select {
    case m = <-ch => m
    default => "slow"
}

val
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("fast"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("fast"));
}

#[test]
fn test_std_time_after_in_select() {
    let script = r#"
use { after } from "std:time"

let ch = chan(0)
let result = ""

select {
    case msg = <-ch => {
        result = "msg"
    }
    case <-after(20) => {
        result = "timer triggered"
    }
}

result
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("timer triggered"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("timer triggered"));
}

#[test]
fn test_select_websocket_event_channel() {
    let script = r#"
let ws = use "std:ws"

let server = ws.listen("127.0.0.1:0")
let port = server.port

let t = spawn(fn() {
    let conn = server.accept(3000)
    if conn != null {
        conn.send("hello websocket select")
    }
})

let client = ws.connect("ws://127.0.0.1:" + str(port))
let client_chan = ws.channel(client)

let outcome = ""
select {
    case event = <-client_chan => {
        outcome = event
    }
    case <-timer(2000) => {
        outcome = "timeout"
    }
}

join(t)
client.close()
server.close()
outcome
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("hello websocket select"));
}

#[test]
fn test_unbuffered_rendezvous_synchronization() {
    let script = r#"
let ch = chan(0)
let order = []

let t1 = spawn(fn() {
    ch <- "rendezvous_data"
    order.push("sender_finished")
})

let t2 = spawn(fn() {
    let data = <-ch
    order.push("receiver_got_" + data)
})

join(t1)
join(t2)
order
"#;
    let ast_res = run(script).expect("AST execution failed");
    let val_json = ast_res.to_json();
    assert!(val_json.as_array().unwrap().contains(&json!("sender_finished")));
    assert!(val_json.as_array().unwrap().contains(&json!("receiver_got_rendezvous_data")));

    let vm_res = run_vm(script).expect("VM execution failed");
    let vm_json = vm_res.to_json();
    assert!(vm_json.as_array().unwrap().contains(&json!("sender_finished")));
    assert!(vm_json.as_array().unwrap().contains(&json!("receiver_got_rendezvous_data")));
}

#[test]
fn test_select_closed_channel_immediate_recv() {
    let script = r#"
let ch = chan(0)
ch.close()

let outcome = ""
select {
    case m = <-ch => {
        if m == null {
            outcome = "closed_null"
        } else {
            outcome = "got_val"
        }
    }
    default => {
        outcome = "default"
    }
}
outcome
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("closed_null"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("closed_null"));
}

#[test]
fn test_unbuffered_try_recv_returns_null() {
    let script = r#"
let ch = chan(0)
let v1 = ch.tryRecv()
[v1, ch.len(), ch.capacity()]
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!([null, 0, 0]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!([null, 0, 0]));
}

#[test]
fn test_multiple_timers_select_earliest_wins() {
    let script = r#"
let winner = ""
select {
    case <-timer(15) => {
        winner = "fast"
    }
    case <-timer(500) => {
        winner = "slow"
    }
}
winner
"#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("fast"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("fast"));
}


#[test]
fn test_concurrent_ping_pong_select_deadlock_free() {
    let script = r#"
        let ch1 = chan(0)
        let ch2 = chan(0)
        let t = spawn(fn() {
            let i = 0
            while i < 100 {
                ch1 <- i
                let reply = <-ch2
                i = i + 1
            }
        })
        let count = 0
        while count < 100 {
            select {
                case val = <-ch1 => {
                    count = count + 1
                    ch2 <- val
                }
            }
        }
        join(t)
        assert(count == 100)
    "#;
    let res = run(script);
    assert!(res.is_ok(), "AST ping pong select failed: {:?}", res);
    let res_vm = run_vm(script);
    assert!(res_vm.is_ok(), "VM ping pong select failed: {:?}", res_vm);
}

#[test]
fn test_select_channel_closed_while_send_waiting() {
    let script = r#"
        let ch = chan(0)
        let t = spawn(fn() {
            <-timer(10)
            ch.close()
        })
        let branch = "none"
        select {
            case ch <- 99 => {
                branch = "sent"
            }
            case <-timer(60) => {
                branch = "timeout"
            }
        }
        join(t)
        branch
    "#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!("timeout"));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!("timeout"));
}

#[test]
fn test_select_no_ghost_data_in_buffered_channel() {
    let script = r#"
        let ch_buf = chan(1)
        ch_buf <- "existing"

        let ch_other = chan(1)
        ch_other <- "other_ready"

        let branch = ""
        select {
            case ch_buf <- "ghost" => {
                branch = "ghost_sent"
            }
            case v = <-ch_other => {
                branch = "recv_other"
            }
        }

        let first = <-ch_buf
        let remaining_len = ch_buf.len()
        [branch, first, remaining_len]
    "#;
    let ast_res = run(script).expect("AST execution failed");
    assert_eq!(ast_res.to_json(), json!(["recv_other", "existing", 0]));

    let vm_res = run_vm(script).expect("VM execution failed");
    assert_eq!(vm_res.to_json(), json!(["recv_other", "existing", 0]));
}

#[test]
fn test_select_send_typed_channel_mismatch_fails() {
    let script = r#"
        let ch: chan<Int> = chan(1)
        select {
            case ch <- "wrong_type" => {
                "sent"
            }
            default => {
                "default"
            }
        }
    "#;
    let ast_res = run(script);
    assert!(ast_res.is_err(), "Expected type error in AST select send");
    let vm_res = run_vm(script);
    assert!(vm_res.is_err(), "Expected type error in VM select send");
}

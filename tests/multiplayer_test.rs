use shae::run;
use shae::value::Value;

#[test]
fn test_multiplayer_arena_integration() {
    let script = r#"
        let cluster = use "std:actor:cluster"
        let physics = use "std:physics"

        let node_host = cluster.node("test_host", "127.0.0.1:0", { "cookie": "secret123" })
        let node_client = cluster.node("test_client", "127.0.0.1:0", { "cookie": "secret123" })

        let ok = node_host.connect(node_client.address())

        let game = node_host.spawn("arena", { "p1": 50, "p2": 50 }, fn(state, msg) {
            if msg["action"] == "move_p1" {
                return {
                    "state": { "p1": state["p1"] + msg["amount"], "p2": state["p2"] },
                    "reply": "p1_moved"
                }
            } else if msg["action"] == "get" {
                return { "state": state, "reply": state }
            }
            return state
        })

        let remote = node_client.actor("arena", "test_host")
        let rep1 = remote.call({ "action": "move_p1", "amount": 15 }, 2000)
        let state = remote.call({ "action": "get" }, 2000)

        // Physics world integration check
        let world = physics.world_new(0.0)
        let b = physics.body({ "pos_x": 10.0, "pos_y": 10.0, "vel_x": 5.0, "vel_y": 0.0 })
        physics.world_add_body(world, b)
        physics.world_step(world, 1.0)
        let bodies = physics.world_get_bodies(world)
        let ball = bodies[0]

        [ok, rep1 == "p1_moved", state["p1"] == 65, ball["pos_x"] > 10.0]
    "#;

    let res = run(script).expect("Multiplayer arena script should run successfully");
    if let Value::Array(arr) = res {
        let g = arr.read().unwrap();
        assert_eq!(g[0], Value::Bool(true), "Cluster connect should succeed");
        assert_eq!(g[1], Value::Bool(true), "Remote call reply should match");
        assert_eq!(g[2], Value::Bool(true), "Actor state should be updated to 65");
        assert_eq!(g[3], Value::Bool(true), "Physics body should step forward");
    } else {
        panic!("Expected array result");
    }
}

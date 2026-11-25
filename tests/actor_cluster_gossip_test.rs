use shae::run_vm;

#[test]
fn test_gossip_lifecycle_and_members_in_shae() {
    let script = r#"
        use { create, add_member, is_alive, is_suspect, alive_members } from "std:actor:gossip";

        let g = create("node_alpha", "127.0.0.1:9001");
        assert(is_alive(g, "node_alpha"), "local node must be alive");
        assert(!is_suspect(g, "node_alpha"), "local node must not be suspect");

        add_member(g, "node_beta", "127.0.0.1:9002");
        assert(is_alive(g, "node_beta"), "added member must be alive");

        let members = alive_members(g);
        assert(members.len == 2, "must have 2 alive members");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "gossip lifecycle script failed: {:?}", res.err());
}

#[test]
fn test_gossip_failure_detection_and_timeout_in_shae() {
    let script = r#"
        use { create, add_member, probe, is_alive, is_suspect, suspect_members, check_timeouts, dead_members } from "std:actor:gossip";

        let g = create("node_1", "127.0.0.1:9101");
        add_member(g, "node_2", "127.0.0.1:9102");

        // Simulate probe failure
        probe(g, "node_2", false);
        assert(!is_alive(g, "node_2"), "node_2 must not be alive");
        assert(is_suspect(g, "node_2"), "node_2 must be suspect");

        let suspects = suspect_members(g);
        assert(suspects.len == 1, "must have 1 suspect member");
        assert(suspects[0].id == "node_2", "suspect must be node_2");

        // Check timeouts with 0ms timeout (immediate expiration)
        check_timeouts(g, 0);
        let dead = dead_members(g);
        assert(dead.len == 1, "must have 1 dead member");
        assert(dead[0].id == "node_2", "dead member must be node_2");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "gossip failure detection script failed: {:?}", res.err());
}

#[test]
fn test_gossip_rumor_dissemination_and_self_refutation_in_shae() {
    let script = r#"
        use { create, apply_rumor, is_alive, is_suspect, drain_rumors } from "std:actor:gossip";

        let g = create("node_local", "127.0.0.1:9201");

        // Remote rumor falsely suspects node_local at incarnation 0
        let updated = apply_rumor(g, "node_local", "127.0.0.1:9201", "suspect", 0);
        assert(updated, "self-refutation must report state update");

        // Node local must have refuted suspicion immediately
        assert(is_alive(g, "node_local"), "node_local must remain alive after refuting");
        assert(!is_suspect(g, "node_local"), "node_local must not be suspect");

        // Drain rumors: should contain new Alive rumor with incarnation 1
        let rumors = drain_rumors(g, 10);
        assert(rumors.len == 1, "must have 1 emitted rumor");
        assert(rumors[0].state == "alive", "emitted rumor must be alive");
        assert(rumors[0].incarnation == 1, "emitted rumor incarnation must be 1");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "gossip self refutation script failed: {:?}", res.err());
}

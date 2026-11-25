use shae::run_vm;

#[test]
fn test_consistent_hash_ring_in_shae() {
    let script = r#"
        use { ring, ring_add, ring_remove, ring_get_node, ring_get_replicas } from "std:actor:mesh";

        let r = ring(50, ["node_1", "node_2", "node_3"]);

        let target_node = ring_get_node(r, "user_account_982");
        assert(target_node != null, "Must find responsible node on ring");
        assert(target_node == "node_1" || target_node == "node_2" || target_node == "node_3", "Must be one of the cluster nodes");

        let replicas = ring_get_replicas(r, "order_item_44", 2);
        assert(replicas.len == 2, "Must return 2 distinct replica nodes");
        assert(replicas[0] != replicas[1], "Replica nodes must be distinct");

        // Remove node_1 and verify ring automatically rebalances
        ring_remove(r, "node_1");
        let new_target = ring_get_node(r, "user_account_982");
        assert(new_target == "node_2" || new_target == "node_3", "Must remap to remaining nodes");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "consistent hash ring script failed: {:?}", res.err());
}

#[test]
fn test_zero_downtime_actor_migration_in_shae() {
    let script = r#"
        use { start_migration, route_message, complete_migration, get_actor_location } from "std:actor:mesh";

        // Begin zero-downtime migration of actor 'actor_wallet_88' from node_alpha to node_beta
        start_migration("actor_wallet_88", "node_alpha", "node_beta", { balance: 500 });

        // While migrating, incoming messages are held in buffer
        let r1 = route_message("actor_wallet_88", { op: "deposit", amount: 50 });
        let r2 = route_message("actor_wallet_88", { op: "transfer", amount: 100 });
        assert(r1.status == "buffered", "Message 1 must be buffered during migration");
        assert(r2.status == "buffered", "Message 2 must be buffered during migration");

        // Target node completes migration and flushes buffered messages
        let flushed = complete_migration("actor_wallet_88");
        assert(flushed.len == 2, "Must flush 2 buffered messages");
        assert(flushed[0].amount == 50, "First message preserved");
        assert(flushed[1].amount == 100, "Second message preserved");

        // Post-migration messages route directly to node_beta
        let r3 = route_message("actor_wallet_88", { op: "balance" });
        assert(r3.status == "routed", "Subsequent message must route directly");
        assert(r3.destination == "node_beta", "Destination must be node_beta");

        let loc = get_actor_location("actor_wallet_88");
        assert(loc == "node_beta", "Actor location must be node_beta");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "zero downtime actor migration script failed: {:?}", res.err());
}

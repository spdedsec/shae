use shae::run_vm;

#[test]
fn test_raft_cluster_election_quorum_in_shae() {
    let script = r#"
        use { create, start_election, handle_request_vote, receive_vote, get_state } from "std:raft";

        let n1 = create("raft_1", ["raft_2", "raft_3"]);
        let n2 = create("raft_2", ["raft_1", "raft_3"]);
        let n3 = create("raft_3", ["raft_1", "raft_2"]);

        // n1 starts election
        let elect = start_election(n1);
        assert(elect.term == 1, "election term must be 1");
        assert(elect.requests.len == 2, "must generate 2 vote requests");

        // n2 votes for n1
        let req2 = elect.requests[0];
        let reply2 = handle_request_vote(n2, req2.term, req2.candidate_id, req2.last_log_index, req2.last_log_term);
        assert(reply2.vote_granted, "n2 must grant vote to n1");

        // n1 receives vote from n2 (2 out of 3 is majority quorum)
        let is_leader = receive_vote(n1, "raft_2", reply2.term, reply2.vote_granted);
        assert(is_leader, "n1 must win election with quorum");

        let s1 = get_state(n1);
        assert(s1.role == "leader", "n1 role must be leader");
        assert(s1.is_leader, "n1 is_leader must be true");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "raft election script failed: {:?}", res.err());
}

#[test]
fn test_raft_replicated_state_machine_and_consensus_in_shae() {
    let script = r#"
        use { create, start_election, receive_vote, propose, handle_append_entries, record_replication, get_state, get_kv } from "std:raft";

        let leader = create("leader_node", ["follower_node"]);
        let follower = create("follower_node", ["leader_node"]);

        // Elect leader_node
        let elect = start_election(leader);
        let won = receive_vote(leader, "follower_node", elect.term, true);
        assert(won, "leader must win election");

        // Propose command on leader
        let log_idx = propose(leader, "SET system:status active");
        assert(log_idx == 1, "first proposed entry index must be 1");

        // Follower receives AppendEntries
        let append_res = handle_append_entries(
            follower,
            1,
            "leader_node",
            0,
            0,
            [{ index: 1, term: 1, command: "SET system:status active" }],
            0
        );
        assert(append_res.success, "follower must append entry successfully");
        assert(append_res.match_index == 1, "follower match_index must be 1");

        // Leader records follower replication -> majority (2/2) commits entry!
        let committed = record_replication(leader, "follower_node", append_res.match_index);
        assert(committed, "entry must reach consensus and commit");

        let s_leader = get_state(leader);
        assert(s_leader.commit_index == 1, "leader commit_index must advance to 1");

        // State machine must have executed SET system:status active
        let val = get_kv(leader, "system:status");
        assert(val == "active", "KV state machine must store 'active'");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "raft replication script failed: {:?}", res.err());
}

#[test]
fn test_raft_follower_state_machine_catchup_in_shae() {
    let script = r#"
        use { create, handle_append_entries, get_state, get_kv } from "std:raft";

        let follower = create("worker_node", ["leader_node"]);

        // Heartbeat with leader_commit = 1
        let res = handle_append_entries(
            follower,
            2,
            "leader_node",
            0,
            0,
            [{ index: 1, term: 2, command: "SET database:cluster_mode primary" }],
            1
        );

        assert(res.success, "follower must accept entries and commit");
        let s = get_state(follower);
        assert(s.commit_index == 1, "follower commit_index must catch up to leader");

        let db_val = get_kv(follower, "database:cluster_mode");
        assert(db_val == "primary", "follower state machine must apply committed log");
    "#;

    let res = run_vm(script);
    assert!(res.is_ok(), "raft catchup script failed: {:?}", res.err());
}

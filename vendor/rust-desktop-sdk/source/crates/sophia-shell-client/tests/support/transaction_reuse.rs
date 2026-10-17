use super::*;

fn reusable(error: Option<u32>) -> (Result<ShellConnection, ShellClientError>, Peer) {
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let profile = bar(0);
    let thread = std::thread::spawn(move || Peer::reusable_handshake(listener, profile, error));
    let connection = ShellConnection::connect_files(&path, options(&profile));
    let peer = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (connection, peer)
}

#[test]
fn retained_writer_rearms_after_ack_and_refusal_opens_a_fresh_writer() {
    let (connection, mut peer) = reusable(None);
    let mut connection = connection.unwrap();
    assert_eq!(peer.walk_count("transaction-reuse"), 1);
    peer.reset_logs();
    for id in 1..=3 {
        let ticket = enqueue_demand(&mut connection, id);
        commit(&mut connection, &mut peer, ticket);
    }
    assert_eq!(peer.walk_count("transaction"), 0);
    assert_eq!(peer.walk_count("transaction-reuse"), 0);
    let refused = enqueue_demand(&mut connection, 4);
    let held = submit(&mut connection, &mut peer);
    peer.answer_error(held.tag, EINVAL);
    assert_eq!(
        settle(&mut connection, &mut peer, refused),
        (Custody::Refused(EINVAL), None)
    );
    let next = enqueue_demand(&mut connection, 5);
    commit(&mut connection, &mut peer, next);
    assert_eq!(peer.walk_count("transaction-reuse"), 1);
    assert_eq!(peer.walk_count("transaction"), 0);
}

#[test]
fn only_missing_reusable_node_selects_legacy() {
    for error in [5, 13, 116] {
        let (connection, peer) = reusable(Some(error));
        assert!(connection.is_err());
        assert_eq!(
            peer.walk_count("transaction"),
            1,
            "only negotiation uses legacy"
        );
        assert_eq!(peer.walk_count("transaction-reuse"), 1);
    }
}

#![cfg(feature = "comparator-progress")]
use sophia_runtime::comparator_counters::{self, progress};

#[test]
fn progress_records_are_bounded_and_cannot_masquerade_as_cost_samples() {
    progress::add(progress::Counter::PeerWait, 3);
    progress::add(progress::Counter::RelayIdle, 5);
    progress::add(progress::Counter::RequestWalk, 7);
    let mut bytes = [0u8; 2048];
    let mut sink = std::io::Cursor::new(bytes.as_mut_slice());
    comparator_counters::write_snapshot(
        &mut sink,
        "peer",
        "frame_end",
        7,
        comparator_counters::snapshot(),
    )
    .unwrap();
    let len = sink.position() as usize;
    let text = std::str::from_utf8(&bytes[..len]).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("mode=progress"));
    assert!(lines[1].contains("sophia_comparator_progress schema=2 mode=progress"));
    assert!(lines[1].contains("peer_wait=3"));
    assert!(lines[1].contains("relay_idle=5"));
    assert!(lines[1].contains("request_walk=7"));
    assert!(lines.iter().all(|line| line.len() < 1024));
    let mut tiny = std::io::Cursor::new([0u8; 1]);
    assert!(comparator_counters::write_progress(&mut tiny, "peer", 7).is_err());
}

#[test]
fn largest_counter_values_fit_one_atomic_pipe_record() {
    let count = u64::MAX.to_string();
    let fields: usize = progress::NAMES
        .iter()
        .map(|name| name.len() + count.len() + 2)
        .sum();
    assert!(fields + 256 < 2048);
    assert!(2048 < 4096, "Linux PIPE_BUF");
}

#![cfg(feature = "comparator-progress")]
use sophia_runtime::comparator_counters::{self, progress};

#[test]
fn progress_records_are_bounded_and_cannot_masquerade_as_cost_samples() {
    progress::add(progress::Counter::PeerWait, 3);
    progress::add(progress::Counter::RelayIdle, 5);
    progress::add(progress::Counter::RequestWalk, 7);
    let mut bytes = [0u8; 2 * comparator_counters::PROGRESS_RECORD_BYTES];
    let mut sink = std::io::Cursor::new(bytes.as_mut_slice());
    comparator_counters::write_snapshot(
        &mut sink,
        "peer",
        "frame_end",
        7,
        comparator_counters::snapshot().unwrap(),
    )
    .unwrap();
    let len = sink.position() as usize;
    let text = std::str::from_utf8(&bytes[..len]).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].starts_with("sophia_comparator_counters schema=2 mode=progress process=peer ")
    );
    assert!(
        !lines[0].contains("faults"),
        "progress records keep their old fields"
    );
    assert_eq!(lines[0].split_whitespace().count(), 14);
    assert!(
        lines[1].starts_with("sophia_comparator_progress schema=4 mode=progress process=peer ")
    );
    // Schema 4 is the 30 base counters, then exactly the 17 service-ledger
    // names, in order, after the seven identity and interval fields.
    let names: Vec<_> = lines[1]
        .split_whitespace()
        .skip(8)
        .map(|field| field.split_once('=').unwrap().0)
        .collect();
    let expected: Vec<_> = progress::NAMES
        .iter()
        .chain(&progress::service_ledger::NAMES)
        .copied()
        .collect();
    assert_eq!(progress::NAMES.len(), 30);
    assert_eq!(progress::service_ledger::NAMES.len(), 17);
    assert_eq!(names, expected);
    assert!(lines[1].contains("peer_wait=3"));
    assert!(lines[1].contains("relay_idle=5"));
    assert!(lines[1].contains("request_walk=7"));
    assert!(
        lines
            .iter()
            .all(|line| line.len() < comparator_counters::PROGRESS_RECORD_BYTES)
    );
    let mut tiny = std::io::Cursor::new([0u8; 1]);
    assert!(comparator_counters::write_progress(&mut tiny, "peer", 7).is_err());
}

#[test]
fn largest_counter_values_fit_one_atomic_pipe_record() {
    let count = u64::MAX.to_string();
    let fields: usize = progress::NAMES
        .iter()
        .chain(&progress::service_ledger::NAMES)
        .map(|name| name.len() + count.len() + 2)
        .sum();
    // Identity and interval fields at their widest, plus the newline.
    let header = format!(
        "sophia_comparator_progress schema=4 mode=progress process=session pid={} sequence={count} before_usec={count} after_usec={count}\n",
        u32::MAX
    );
    // The record buffer is Linux PIPE_BUF, so one write stays atomic.
    assert_eq!(comparator_counters::PROGRESS_RECORD_BYTES, 4096);
    assert!(header.len() + fields <= comparator_counters::PROGRESS_RECORD_BYTES);
}

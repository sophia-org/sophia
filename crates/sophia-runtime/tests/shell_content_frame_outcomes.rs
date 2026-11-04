#![cfg(feature = "experimental-native-frame-transaction")]

#[path = "support/frame_stream.rs"]
#[expect(
    dead_code,
    reason = "outcome tests use a subset of the lifecycle fixture"
)]
mod support;

use sophia_protocol::ShellContentRecord;
use sophia_runtime::ContentCandidateError as Error;
use support::*;

#[test]
fn prepared_keeps_stream_identity_and_cannot_escape_through_the_legacy_reader() {
    let mut f = Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    f.frame(1, 1, None).unwrap();
    let _display = f.submit(1);
    let prepared = f.candidates().pending_event().cloned().unwrap();
    assert!(matches!(&prepared.record, ShellContentRecord::CandidateOutcome(v) if v.kind == 1));
    let metadata = prepared
        .frame_credit
        .expect("Prepared needs its SFT identity");
    assert_eq!(metadata.key, key(1));
    assert_eq!(metadata.expires_msec, None);
    assert_eq!(f.candidates().take_event(), None);

    let occupancy = f.pool.accounting().response_records;
    let now = f.now;
    let (resources, candidates) = f.pool.active_pair_mut().unwrap();
    assert_eq!(
        candidates.transfer_frame_event_at(resources, now, |_| Err(Error::Budget)),
        Err(Error::Budget)
    );
    assert_eq!(candidates.pending_event(), Some(&prepared));
    assert_eq!(f.pool.accounting().response_records, occupancy);
    assert_eq!(f.drain_outcomes(), vec![prepared]);
    assert_eq!(f.pool.accounting().response_records, occupancy - 1);

    let now = f.tick();
    assert_eq!(
        f.candidates().begin(tx(20), begin(2, 1), now),
        Err(Error::Budget)
    );
    f.present(1);
    let terminal = f.drain_outcomes();
    assert!(terminal[0].frame_credit.unwrap().expires_msec.is_some());
    let now = f.tick();
    assert_eq!(f.candidates().begin(tx(21), begin(3, 1), now), Ok(()));
}

#[test]
fn closing_does_not_strip_a_queued_prepared_of_its_stream_identity() {
    let mut f = Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    f.frame(1, 1, None).unwrap();
    let _display = f.submit(1);
    let now = f.tick();
    f.candidates()
        .close_frame_stream(tx(50), key(1), now)
        .unwrap();
    f.candidates().renderer_failed_at(output(), 1, now).unwrap();
    assert!(f.candidates().take_event().is_none());
    let prepared = f.drain_outcomes();
    assert_eq!(prepared.len(), 1, "close notice preserves FIFO order");
    assert_eq!(prepared[0].frame_credit.unwrap().key, key(1));
    assert_eq!(prepared[0].frame_credit.unwrap().expires_msec, None);
    f.candidates().take_frame_stream_event().unwrap();
    let terminal = f.drain_outcomes();
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0].frame_credit.unwrap().expires_msec, None);
    f.open(2).unwrap();
}

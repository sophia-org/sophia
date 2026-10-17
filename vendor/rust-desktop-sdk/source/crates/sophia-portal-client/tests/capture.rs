use sophia_portal_client::{
    capture::{Custody, Error, Pending},
    protocol::{CaptureRequest, CaptureState},
};

fn pending() -> Pending {
    Pending::new(
        CaptureRequest {
            serial: 9,
            output: 7,
        },
        0,
        100,
    )
    .unwrap()
}
fn accepted() -> Pending {
    let mut p = pending();
    p.issued(0).unwrap();
    p.write_reply(Ok(24), 1).unwrap();
    p
}
fn status(serial: u64, output: u64, state: u64) -> [u8; 32] {
    let mut bytes = [0; 32];
    for (chunk, field) in bytes.chunks_exact_mut(8).zip([1, serial, output, state]) {
        chunk.copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

#[test]
fn exact_write_pending_snapshots_and_all_terminal_outcomes() {
    for (wire, outcome) in [
        (3, CaptureState::Ready),
        (4, CaptureState::Denied),
        (5, CaptureState::Unavailable),
        (6, CaptureState::Pressure),
        (7, CaptureState::Cancelled),
    ] {
        let mut p = pending();
        assert_eq!(
            p.bytes(),
            &[
                1, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        assert_eq!(p.start_status(0), Err(Error::Busy));
        p.issued(0).unwrap();
        assert_eq!(p.issued(0), Err(Error::Busy));
        p.write_reply(Ok(24), 1).unwrap();
        assert_eq!(p.custody(), Custody::Accepted);
        for (now, state) in [(1, 1), (2, 2)] {
            assert_eq!(p.next_wake(), Some(now));
            assert_eq!(p.start_status(now), Ok(100));
            assert_eq!(p.start_status(now), Err(Error::Busy));
            p.status(&status(9, 7, state), now).unwrap();
            assert_eq!(p.outcome(), None);
            assert_eq!(p.start_status(now), Err(Error::Busy));
        }
        p.start_status(3).unwrap();
        p.status(&status(9, 7, wire), 3).unwrap();
        assert_eq!(p.custody(), Custody::Settled);
        assert_eq!(p.outcome(), Some(outcome));
        assert_eq!(p.next_wake(), None);
        p.advance(1000).unwrap();
        p.close();
        assert_eq!(p.custody(), Custody::Settled);
        assert_eq!(p.outcome(), Some(outcome));
    }
}

#[test]
fn refusal_short_write_loss_and_local_cancel_never_replay() {
    for errno in [11, 116, 13, 22] {
        let mut p = pending();
        p.issued(0).unwrap();
        assert_eq!(p.write_reply(Err(errno), 1), Err(Error::Refused(errno)));
        assert_eq!(p.custody(), Custody::Refused);
        p.close();
        assert_eq!(p.issued(2), Err(Error::Refused(errno)));
    }
    for count in [0, 1, 23, 25, u32::MAX] {
        let mut p = pending();
        p.issued(0).unwrap();
        assert_eq!(p.write_reply(Ok(count), 1), Err(Error::Protocol));
        assert_eq!(p.custody(), Custody::Unknown);
        assert_eq!(p.issued(2), Err(Error::Protocol));
    }
    let mut p = pending();
    p.close();
    assert_eq!(p.custody(), Custody::DroppedUnsent);
    let mut p = pending();
    p.issued(0).unwrap();
    p.close();
    assert_eq!(p.custody(), Custody::Unknown);
    let mut p = accepted();
    p.close();
    assert_eq!(p.custody(), Custody::Unknown);
}

#[test]
fn status_identity_deadline_and_clock_are_not_reset_by_progress() {
    for bytes in [
        status(10, 7, 3),
        status(9, 8, 3),
        status(0, 0, 0),
        status(9, 7, 8),
    ] {
        let mut p = accepted();
        p.start_status(1).unwrap();
        assert_eq!(p.status(&bytes, 1), Err(Error::Protocol));
        assert_eq!(p.custody(), Custody::Unknown);
        assert_eq!(p.outcome(), None);
    }
    let mut p = accepted();
    p.start_status(1).unwrap();
    assert_eq!(p.status(&status(9, 7, 3)[..31], 1), Err(Error::Protocol));
    let mut p = accepted();
    assert_eq!(p.advance(0), Err(Error::ClockRegression));
    assert_eq!(p.error(), None);
    for now in 1..100 {
        assert_eq!(p.start_status(now), Ok(100));
        assert_eq!(
            p.status(&status(9, 7, 2), now - 1),
            Err(Error::ClockRegression)
        );
        p.status(&status(9, 7, 2), now).unwrap();
    }
    assert_eq!(p.start_status(100), Err(Error::Deadline));
    assert_eq!(p.custody(), Custody::Unknown);
    let mut p = pending();
    assert_eq!(p.issued(100), Err(Error::Deadline));
    assert_eq!(p.custody(), Custody::DroppedUnsent);
    let r = CaptureRequest {
        serial: 1,
        output: 1,
    };
    assert!(Pending::new(r, 0, 10000).is_ok());
    assert!(Pending::new(r, 0, 10001).is_err());
    assert!(Pending::new(r, u64::MAX - 1, u64::MAX).is_ok());
    assert!(Pending::new(r, 1, 1).is_err());
    assert!(Pending::new(CaptureRequest { serial: 0, ..r }, 0, 10).is_err());
}

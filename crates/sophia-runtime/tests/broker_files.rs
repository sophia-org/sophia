use sophia_9p::{Errno, export::ReadOutcome};
use sophia_protocol::broker_files::BrokerFileLimits;
use sophia_runtime::broker_files::BrokerFileCustody;
use std::time::{Duration, Instant};
fn candidate(range: u16) -> ([u8; 24], [u8; 40]) {
    let mut control = [0; 24];
    control[0] = 7;
    control[8] = 1;
    control[16] = 40;
    let mut bytes = [0; 40];
    bytes[0] = 40;
    bytes[4] = 1;
    bytes[7] = 1;
    bytes[8] = 7;
    bytes[16] = 1;
    bytes[32..34].copy_from_slice(&range.to_le_bytes());
    bytes[34..36].copy_from_slice(&range.to_le_bytes());
    (control, bytes)
}
fn ack(seq: u8) -> [u8; 16] {
    let mut b = [0; 16];
    b[0] = 7;
    b[8] = seq;
    b
}
#[test]
fn handshake_retry_and_ack_preserve_exact_custody() {
    let now = Instant::now();
    let mut c = BrokerFileCustody::new(7, BrokerFileLimits::default()).unwrap();
    let (control, bytes) = candidate(2);
    c.negotiate(&control, &bytes, now).unwrap();
    assert!(c.negotiated());
    assert_eq!(c.retained(), Some(bytes.as_slice()));
    c.negotiate(&control, &bytes, now).unwrap();
    let ReadOutcome::Ready(events) = c.read(0, 4096, now).unwrap() else {
        panic!()
    };
    assert_eq!(events.len(), 104);
    assert_eq!(
        (events[6], events[24], events[54], events[72]),
        (18, 1, 16, 2)
    );
    c.acknowledge(&ack(1), now).unwrap();
    assert!(c.retained().is_none());
    assert_eq!(c.negotiate(&control, &bytes, now), Err(Errno(114)));
    assert!(matches!(c.read(0, 1, now), Err(Errno::ESTALE)));
    c.acknowledge(&ack(2), now).unwrap();
    c.expire(now + Duration::from_secs(3));
    assert!(!c.closed());
}
#[test]
fn partial_ack_does_not_extend_refusal_drain() {
    let now = Instant::now();
    let mut c = BrokerFileCustody::new(7, BrokerFileLimits::default()).unwrap();
    let (control, bytes) = candidate(9);
    c.negotiate(&control, &bytes, now).unwrap();
    assert!(!c.negotiated());
    c.acknowledge(&ack(1), now + Duration::from_millis(1500))
        .unwrap();
    assert!(matches!(
        c.read(48, 40, now + Duration::from_millis(1900)),
        Ok(ReadOutcome::Ready(_))
    ));
    c.expire(now + Duration::from_millis(2000));
    assert!(c.closed());
    assert!(matches!(
        c.read(48, 40, now + Duration::from_millis(2000)),
        Err(Errno::ESTALE)
    ));
}
#[test]
fn terminal_ack_closes_refusal_and_wrong_epoch_never_publishes() {
    let now = Instant::now();
    let mut c = BrokerFileCustody::new(7, BrokerFileLimits::default()).unwrap();
    let (mut control, bytes) = candidate(9);
    control[0] = 8;
    assert_eq!(c.negotiate(&control, &bytes, now), Err(Errno::ESTALE));
    assert!(c.retained().is_none());
    assert!(matches!(c.read(0, 1, now), Ok(ReadOutcome::Pending)));
    control[0] = 7;
    c.negotiate(&control, &bytes, now).unwrap();
    c.acknowledge(&ack(2), now).unwrap();
    assert!(c.closed());
}

#[test]
fn response_preparation_is_atomic_and_retry_never_repeats_effects() {
    use sophia_protocol::{MetadataDisclosure, SurfaceId, broker_files::BrokerFileRequest};
    use std::cell::Cell;
    let now = Instant::now();
    let mut c = BrokerFileCustody::new(7, BrokerFileLimits::default()).unwrap();
    let (hello, bytes) = candidate(2);
    c.negotiate(&hello, &bytes, now).unwrap();
    c.acknowledge(&ack(2), now).unwrap();
    let request = BrokerFileRequest::SetDisclosure {
        surface: SurfaceId::new(3, 4),
        disclosure: MetadataDisclosure::ClassOnly,
    };
    c.publish(9, request, now).unwrap();
    assert_eq!(c.last_request(), 9);
    assert_eq!(c.publish(10, request, now), Err(Errno::EAGAIN));
    let mut control = [0; 24];
    control[0] = 7;
    control[8] = 2;
    control[16..20].copy_from_slice(&448u32.to_le_bytes());
    let mut response = [0; 448];
    response[..4].copy_from_slice(&448u32.to_le_bytes());
    response[4] = 1;
    response[6] = 1;
    response[7] = 1;
    response[8] = 7;
    response[16] = 2;
    response[32] = 9;
    response[40] = 5;
    response[42] = 2;
    response[48] = 1;
    response[56] = 3;
    response[60] = 4;
    response[64] = 2;
    response[66] = 1;
    response[80] = 8;
    response[248] = 2;
    response[256] = 3;
    response[260] = 4;
    response[266] = 1;
    response[268] = 2;
    response[280] = 8;
    response[288] = 12;
    response[296] = 1;
    response[304] = 8;
    let effects = Cell::new(0);
    assert_eq!(
        c.respond(
            &control,
            &response,
            now,
            |_| Err::<(), _>(Errno::EAGAIN),
            |_| effects.set(99)
        ),
        Err(Errno::EAGAIN)
    );
    assert_eq!(effects.get(), 0);
    assert!(c.retained().is_none());
    assert!(matches!(c.read(304, 1, now), Ok(ReadOutcome::Pending)));
    let mut bad = response;
    bad[288] = 0;
    assert_eq!(
        c.respond(
            &control,
            &bad,
            now,
            |_| {
                effects.set(99);
                Ok(())
            },
            |_| ()
        ),
        Err(Errno::EINVAL)
    );
    assert_eq!(effects.get(), 0);
    c.respond(
        &control,
        &response,
        now,
        |set| Ok(set.rows().count()),
        |count| effects.set(count),
    )
    .unwrap();
    assert_eq!(effects.get(), 2);
    assert_eq!(c.retained(), Some(response.as_slice()));
    c.respond(
        &control,
        &response,
        now,
        |_| panic!("retry prepared effects"),
        |_: ()| panic!("retry committed effects"),
    )
    .unwrap();
    assert_eq!(effects.get(), 2);
    assert_eq!(c.publish(10, request, now), Err(Errno::EAGAIN));
    c.acknowledge(&ack(4), now).unwrap();
    c.publish(10, request, now).unwrap();
    assert_eq!(
        c.respond(&control, &response, now, |_| Ok(()), |_| ()),
        Err(Errno(114))
    );
}
#[test]
fn request_deadline_survives_ack_and_owner_watermark_survives_epoch() {
    use sophia_protocol::{SurfaceId, broker_files::BrokerFileRequest};
    let now = Instant::now();
    let mut c = BrokerFileCustody::new_after(7, BrokerFileLimits::default(), 8).unwrap();
    let (hello, bytes) = candidate(2);
    c.negotiate(&hello, &bytes, now).unwrap();
    c.acknowledge(&ack(2), now).unwrap();
    let request = BrokerFileRequest::SurfaceRemoved {
        surface: SurfaceId::new(3, 4),
    };
    assert_eq!(c.publish(8, request, now), Err(Errno(114)));
    c.publish(9, request, now).unwrap();
    c.acknowledge(&ack(3), now).unwrap();
    c.expire(now + Duration::from_millis(4999));
    assert!(!c.closed());
    c.expire(now + Duration::from_millis(5000));
    assert!(c.closed());
    assert_eq!(c.last_request(), 9);
}

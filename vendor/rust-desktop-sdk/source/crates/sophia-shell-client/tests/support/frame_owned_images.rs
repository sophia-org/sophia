use super::*;
use std::os::unix::fs::FileExt;

fn fixture(name: &str) -> Fixture {
    let mut f = Fixture::new(name);
    f.upload.width = 2;
    f.upload.height = 1;
    f
}

fn source_word(f: &Fixture) -> u64 {
    let source = std::fs::File::from(f._source.try_clone().unwrap());
    let mut word = [0; 8];
    source.read_exact_at(&mut word, 0).unwrap();
    u64::from_le_bytes(word)
}

#[test]
fn worker_fill_is_admitted_once_after_capacity_without_rerendering() {
    let mut f = fixture("frame-owned-worker");
    let mut loan = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    let (started, wait_started) = std::sync::mpsc::sync_channel(0);
    let (resume, wait_resume) = std::sync::mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        loan.pixels_mut().unwrap()[..4].fill(31);
        started.send(()).unwrap();
        wait_resume.recv().unwrap();
        loan.pixels_mut().unwrap()[4..].fill(73);
        loan
    });
    wait_started.recv().unwrap();
    f.c.poll_io().unwrap();
    assert!(matches!(
        f.c.image_acquire_ring_fill(8, Instant::now()),
        Err(ImageAdapterError::Busy)
    ));
    assert_eq!(source_word(&f), 0);
    resume.send(()).unwrap();
    let mut loan = worker.join().unwrap();
    let before = f.record.clone();
    f.c.set_queue_limits(QueueLimits {
        records: 1,
        bytes: 8192,
        control_records: 0,
        control_bytes: 0,
    })
    .unwrap();
    let held = f.c.reserve_capacity(1, 8192).unwrap();
    assert_eq!(
        f.stream
            .submit_ring_fill(&mut f.c, &mut f.record, &mut loan, f.upload, Instant::now()),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(f.record, before);
    assert_eq!(
        loan.pixels_mut().unwrap(),
        &[31, 31, 31, 31, 73, 73, 73, 73]
    );
    assert!(f.stream.has_credit());
    assert_eq!(source_word(&f), 0);
    f.c.cancel_reservation(&held).unwrap();
    let (admission, offer) = f
        .stream
        .submit_ring_fill(&mut f.c, &mut f.record, &mut loan, f.upload, Instant::now())
        .unwrap();
    assert!(loan.pixels_mut().is_err());
    assert_eq!(f.record, before);
    assert_eq!(offer, 1);
    assert_eq!(source_word(&f), 2);
    assert!(!f.stream.has_credit());
    f.custody(admission.first);
    let ShellFrameRecord::Commit(sent) = decode_shell_frame_record(
        f.p.transactions.last().unwrap(),
        ShellFileClass::Candidate,
        13,
        8192,
    )
    .unwrap() else {
        panic!("commit")
    };
    assert_eq!(sent.image.unwrap().offer_id, offer);
    assert_eq!(sent.image.unwrap().slot_generation, 2);
    assert_eq!(f.p.control_writes.len(), 1, "no separate legacy Offer");
    let mut bytes = [0; 8];
    std::fs::File::from(f._source.try_clone().unwrap())
        .read_exact_at(&mut bytes, OWNED_RING_HEADER_BYTES)
        .unwrap();
    assert_eq!(bytes, [31, 31, 31, 31, 73, 73, 73, 73]);
    f.deliver(f.outcome(1)).unwrap();
    assert!(!f.stream.has_credit());
    f.deliver(f.outcome(2)).unwrap();
    assert!(f.stream.has_credit());
    assert_eq!(
        f.c.image_ring_offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    let status = f.p.event(
        ShellFileKind::ResourceStatus,
        &status_body(offer, f.upload.resource, f.upload.resource_generation, 2),
    );
    f.p.push(status);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(offer).unwrap().status.is_some()
    });
    assert_eq!(
        f.c.image_ring_offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    let release = ring_released(
        f.p.take_sequence(),
        offer,
        2,
        f.upload.resource,
        f.upload.resource_generation,
        0,
    );
    f.p.push(release);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(offer).unwrap().state == RingOfferState::Settled
    });
    f.c.image_forget(offer).unwrap();
    let mut next = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    f.c.image_abort_ring_fill(&mut next).unwrap();
}

#[test]
fn foreign_or_wrong_size_loan_is_retained_and_never_published() {
    let mut f = fixture("frame-owned-origin");
    let mut other = fixture("frame-owned-foreign");
    let mut loan = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    loan.pixels_mut().unwrap().fill(19);
    assert!(
        other
            .stream
            .submit_ring_fill(
                &mut other.c,
                &mut other.record,
                &mut loan,
                other.upload,
                Instant::now()
            )
            .is_err()
    );
    assert_eq!(source_word(&f), 0);
    assert_eq!(source_word(&other), 0);
    assert!(other.stream.has_credit());
    let mut invalid = f.upload;
    invalid.width += 1;
    assert!(
        f.stream
            .submit_ring_fill(&mut f.c, &mut f.record, &mut loan, invalid, Instant::now())
            .is_err()
    );
    assert_eq!(loan.pixels_mut().unwrap(), &[19; 8]);
    assert!(f.stream.has_credit());
    assert_eq!(source_word(&f), 0);
    f.c.image_abort_ring_fill(&mut loan).unwrap();
}

#[test]
fn exact_frame_refusal_settles_an_owned_source_but_contradictory_admission_does_not() {
    for admitted in [false, true] {
        let mut f = fixture("frame-owned-refusal");
        let mut loan = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
        let (admission, offer) = f
            .stream
            .submit_ring_fill(&mut f.c, &mut f.record, &mut loan, f.upload, Instant::now())
            .unwrap();
        f.custody(admission.first);
        if admitted {
            f.deliver(f.outcome(1)).unwrap();
        }
        let result = f.deliver(f.refused(1));
        assert_eq!(result.is_err(), admitted);
        assert_eq!(
            f.c.image_ring_offer(offer).unwrap().state,
            if admitted {
                RingOfferState::Unknown
            } else {
                RingOfferState::Refused
            }
        );
        assert_eq!(source_word(&f), 2, "published identity stays spent");
    }
}

#[test]
fn legacy_owned_commit_cannot_bypass_frame_admission() {
    let mut f = fixture("frame-owned-legacy");
    let mut loan = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    loan.pixels_mut().unwrap().fill(42);
    assert!(matches!(
        f.c.image_commit_ring_fill(&mut loan, f.upload, Instant::now()),
        Err(ImageAdapterError::NotEnabled)
    ));
    assert_eq!(loan.pixels_mut().unwrap(), &[42; 8]);
    assert!(f.stream.has_credit());
    f.c.image_abort_ring_fill(&mut loan).unwrap();
    assert_eq!(f.p.control_writes.len(), 1, "registration only");
}

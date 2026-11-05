use super::*;

fn fixture(name: &str) -> Fixture {
    let mut f = Fixture::new(name);
    f.upload.width = 2;
    f.upload.height = 1;
    f
}

fn close(f: &mut Fixture) {
    let ticket = f
        .stream
        .close(&mut f.c, TransactionId::from_raw(30))
        .unwrap()
        .first;
    f.custody(ticket);
    f.deliver(ShellFrameRecord::Closed {
        transaction: TransactionId::from_raw(30),
        key: f.key,
        reason: 11,
    })
    .unwrap();
}

fn status(f: &mut Fixture, offer: u64, upload: RingUpload) {
    let bytes = f.p.event(
        ShellFileKind::ResourceStatus,
        &status_body(offer, upload.resource, upload.resource_generation, 2),
    );
    f.p.push(bytes);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(offer).unwrap().status.is_some()
    });
}

fn release(f: &mut Fixture, offer: u64, upload: RingUpload, slot: u32) {
    let bytes = RingEvent {
        kind: RingEventKind::SlotReleased,
        identity: RING_IDENTITY,
        sequence: f.p.take_sequence(),
        offer_id: offer,
        slot,
        slot_generation: 2,
        resource: upload.resource,
        resource_generation: upload.resource_generation,
        reason: 0,
        changed_generation: false,
    }
    .encode();
    f.p.push(bytes);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(offer).unwrap().state == RingOfferState::Settled
    });
}

#[test]
fn fallback_waits_for_both_frame_drain_and_exact_source_release() {
    let mut f = fixture("frame-fallback-drain");
    let (admission, first) = f.submit();
    f.custody(admission.first);
    f.deliver(f.outcome(1)).unwrap();
    let mut fill = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    fill.pixels_mut().unwrap().fill(41);
    let mut upload = f.upload;
    upload.resource += 1;
    assert!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, upload, Instant::now())
            .is_err()
    );
    close(&mut f);
    assert!(
        !f.stream.frame_work_drained(),
        "Closed is not an outstanding frame's terminal"
    );
    assert!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, upload, Instant::now())
            .is_err()
    );
    let mut terminal = f.outcome(2);
    let ShellFrameRecord::Outcome {
        credit_expires_msec,
        ..
    } = &mut terminal
    else {
        unreachable!()
    };
    *credit_expires_msec = 0;
    f.deliver(terminal).unwrap();
    assert!(f.stream.frame_work_drained());
    assert_eq!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, upload, Instant::now()),
        Err(ShellClientError::Image(ImageAdapterError::Busy))
    );
    let prior = f.upload;
    status(&mut f, first, prior);
    assert_eq!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, upload, Instant::now()),
        Err(ShellClientError::Image(ImageAdapterError::Busy))
    );
    assert_eq!(fill.pixels_mut().unwrap(), &[41; 8]);
    release(&mut f, first, prior, 0);
    let offer = f
        .stream
        .commit_legacy_ring_fill(&mut f.c, &mut fill, upload, Instant::now())
        .unwrap();
    assert_eq!(offer, first + 1);
    assert!(fill.pixels_mut().is_err());
    drive(&mut f.c, &mut f.p, |_, p| p.control_writes.len() == 2);
    assert_eq!(
        f.p.control_writes.last().unwrap(),
        &RingOffer {
            identity: RING_IDENTITY,
            offer_id: offer,
            slot: 1,
            slot_generation: 2,
            resource: upload.resource,
            resource_generation: upload.resource_generation,
            allocation: upload.allocation,
            allocation_generation: upload.allocation_generation,
            output: upload.output,
            output_generation: upload.output_generation,
            topology_generation: upload.topology_generation,
            width: upload.width,
            height: upload.height,
        }
        .encode()
    );
    assert_eq!(f.c.welcome().selected_revision, 13);
    assert!(!f.stream.has_credit());
    status(&mut f, offer, upload);
    release(&mut f, offer, upload, 1);
    assert_eq!(
        f.c.image_ring_offer(first).unwrap().state,
        RingOfferState::Settled
    );
}

#[test]
fn source_release_does_not_replace_stream_and_frame_drain() {
    let mut f = fixture("frame-fallback-source-first");
    let (admission, offer) = f.submit();
    f.custody(admission.first);
    f.deliver(f.outcome(1)).unwrap();
    let upload = f.upload;
    status(&mut f, offer, upload);
    release(&mut f, offer, upload, 0);
    let mut fill = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    fill.pixels_mut().unwrap().fill(41);
    let mut next = upload;
    next.resource += 1;
    assert!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, next, Instant::now())
            .is_err()
    );
    close(&mut f);
    assert!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, next, Instant::now())
            .is_err()
    );
    assert_eq!(fill.pixels_mut().unwrap(), &[41; 8]);
    let mut terminal = f.outcome(2);
    let ShellFrameRecord::Outcome {
        credit_expires_msec,
        ..
    } = &mut terminal
    else {
        unreachable!()
    };
    *credit_expires_msec = 0;
    f.deliver(terminal).unwrap();
    assert!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, next, Instant::now())
            .is_ok()
    );
}

#[test]
fn fallback_rejects_wrong_binding_empty_or_unknown_owner_and_closed_transport() {
    let mut f = fixture("frame-fallback-binding");
    close(&mut f);
    let mut fill = f.c.image_acquire_ring_fill(8, Instant::now()).unwrap();
    fill.pixels_mut().unwrap().fill(29);
    for field in 0..6 {
        let mut upload = f.upload;
        match field {
            0 => upload.output += 1,
            1 => upload.output_generation += 1,
            2 => upload.allocation += 1,
            3 => upload.allocation_generation += 1,
            4 => upload.topology_generation += 1,
            5 => upload.width += 1,
            _ => unreachable!(),
        }
        assert!(
            f.stream
                .commit_legacy_ring_fill(&mut f.c, &mut fill, upload, Instant::now())
                .is_err()
        );
        assert_eq!(fill.pixels_mut().unwrap(), &[29; 8]);
    }
    let mut empty = FrameStream::new();
    assert!(
        empty
            .commit_legacy_ring_fill(&mut f.c, &mut fill, f.upload, Instant::now())
            .is_err()
    );
    empty.disconnect();
    assert!(
        empty
            .commit_legacy_ring_fill(&mut f.c, &mut fill, f.upload, Instant::now())
            .is_err()
    );
    assert_eq!(f.p.control_writes.len(), 1);
    f.c.close();
    assert_eq!(
        f.stream
            .commit_legacy_ring_fill(&mut f.c, &mut fill, f.upload, Instant::now()),
        Err(ShellClientError::Image(ImageAdapterError::Closed))
    );
    assert_eq!(fill.pixels_mut().unwrap(), &[29; 8]);
    f.c.image_abort_ring_fill(&mut fill).unwrap();
}

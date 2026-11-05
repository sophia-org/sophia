use super::*;

#[test]
fn worker_loan_preserves_bytes_and_services_another_offer() {
    let mut producer = ready_slots(2);
    let (first, _) = producer.upload(upload(), &[1; 8], 3).unwrap();
    let mut loan = producer.acquire_owned_slot(8, 4).unwrap();
    let (go, wait) = std::sync::mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        wait.recv().unwrap();
        loan.pixels_mut()
            .unwrap()
            .copy_from_slice(&[3, 5, 7, 11, 13, 17, 19, 23]);
        loan
    });
    assert_eq!(
        producer.upload(upload(), &[0; 8], 5),
        Err(ProducerError::Busy)
    );
    assert!(matches!(producer.acquire_slot(5), Err(ProducerError::Busy)));
    assert!(matches!(
        producer.acquire_owned_slot(8, 5),
        Err(ProducerError::Busy)
    ));
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 6)
        .unwrap();
    producer.ring_event(released(first, 2), 7).unwrap();
    assert_eq!(
        producer.offer(first).unwrap().state,
        RingOfferState::Settled
    );
    go.send(()).unwrap();
    let mut loan = worker.join().unwrap();
    let (second, bytes) = producer
        .commit_owned_slot(
            &mut loan,
            RingUpload {
                resource: 12,
                ..upload()
            },
            8,
        )
        .unwrap();
    assert_eq!(second, 2);
    assert_eq!(bytes.len(), 152);
    assert!(loan.pixels_mut().is_err());
    let extent = producer.registration.slot_extent() as usize;
    let header = OWNED_RING_HEADER_BYTES as usize;
    let map = producer.ring.as_mut().unwrap().bytes_mut();
    assert_eq!(&map[extent..extent + 8], &2u64.to_le_bytes());
    assert_eq!(
        &map[extent + header..extent + header + 8],
        &[3, 5, 7, 11, 13, 17, 19, 23]
    );
    assert!(map[extent + header + 8..2 * extent].iter().all(|v| *v == 0));
    assert_eq!(&map[header..header + 8], &[1; 8]);
    assert_eq!(
        producer.offer(second).unwrap().state,
        RingOfferState::Pending
    );
}

#[test]
fn abort_and_failed_commit_do_not_spend_or_publish() {
    let mut producer = ready();
    let mut loan = producer.acquire_owned_slot(8, 3).unwrap();
    loan.pixels_mut().unwrap().fill(99);
    assert_eq!(
        producer.commit_owned_slot(
            &mut loan,
            RingUpload {
                width: 3,
                ..upload()
            },
            4
        ),
        Err(ProducerError::Argument)
    );
    assert_eq!(loan.pixels_mut().unwrap(), &[99; 8]);
    assert_eq!(producer.next_offer, 1);
    assert_eq!(producer.slots[0].generation, 0);
    assert!(producer.offers().next().is_none());
    producer.abort_owned_slot(&mut loan).unwrap();
    assert!(loan.pixels_mut().is_err());
    assert!(producer.abort_owned_slot(&mut loan).is_err());
    let map = producer.ring.as_mut().unwrap().bytes_mut();
    assert!(
        map[..OWNED_RING_HEADER_BYTES as usize]
            .iter()
            .all(|v| *v == 0)
    );
    drop(loan);
    let (offer, _) = producer.upload(upload(), &[1; 8], 5).unwrap();
    assert_eq!(offer, 1);
    assert_eq!(producer.slots[0].generation, 2);
}

#[test]
fn retry_and_exact_release_preserve_the_existing_slot_contract() {
    let mut producer = ready();
    let mut loan = producer.acquire_owned_slot(8, 3).unwrap();
    loan.pixels_mut().unwrap().fill(7);
    assert_eq!(
        producer.commit_owned_slot(&mut loan, upload(), 2),
        Err(ProducerError::Argument)
    );
    let (offer, _) = producer.commit_owned_slot(&mut loan, upload(), 4).unwrap();
    assert_eq!(offer, 1);
    assert!(producer.commit_owned_slot(&mut loan, upload(), 4).is_err());
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 5)
        .unwrap();
    assert!(matches!(
        producer.acquire_owned_slot(8, 6),
        Err(ProducerError::Busy)
    ));
    producer.ring_event(released(offer, 2), 7).unwrap();
    let mut next = producer.acquire_owned_slot(8, 8).unwrap();
    let (second, _) = producer.commit_owned_slot(&mut next, upload(), 9).unwrap();
    assert_eq!(second, 2);
    assert_eq!(producer.slots[0].generation, 4);
}

#[test]
fn foreign_loan_is_refused_even_with_identical_wire_identities() {
    let mut original = ready();
    let mut other = ready();
    let mut loan = original.acquire_owned_slot(8, 3).unwrap();
    let mut theirs = other.acquire_owned_slot(8, 3).unwrap();
    assert_eq!(
        other.commit_owned_slot(&mut loan, upload(), 4),
        Err(ProducerError::Refused)
    );
    assert_eq!(
        other.abort_owned_slot(&mut loan),
        Err(ProducerError::Refused)
    );
    assert_eq!(loan.pixels_mut().unwrap().len(), 8);
    original.abort_owned_slot(&mut loan).unwrap();
    other.abort_owned_slot(&mut theirs).unwrap();
    assert!(original.ready() && other.ready());
}

#[test]
fn every_close_path_refuses_late_commit_but_allows_custody_return() {
    for close in [
        RingProducer::close,
        RingProducer::journal_lost,
        RingProducer::channel_closed,
    ] {
        let mut producer = ready();
        let mut loan = producer.acquire_owned_slot(8, 3).unwrap();
        close(&mut producer);
        loan.pixels_mut().unwrap().fill(2);
        assert_eq!(
            producer.commit_owned_slot(&mut loan, upload(), 4),
            Err(ProducerError::Closed)
        );
        producer.abort_owned_slot(&mut loan).unwrap();
        assert!(producer.closed());
        assert!(!producer.ready());
        assert_eq!(producer.next_offer, 1);
        assert_eq!(producer.slots[0].generation, 0);
    }
}

#[test]
fn abandonment_closes_immediately_and_marks_unsettled_offers_unknown() {
    let mut producer = ready_slots(2);
    let (offer, _) = producer.upload(upload(), &[0; 8], 3).unwrap();
    let loan = producer.acquire_owned_slot(8, 4).unwrap();
    let worker = std::thread::spawn(move || {
        let _loan = loan;
        panic!("render failure");
    });
    assert!(worker.join().is_err());
    assert!(producer.closed());
    assert!(!producer.ready());
    assert_eq!(producer.cause(), CloseCause::Fault);
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
    assert_eq!(
        producer.offers().next().unwrap().state,
        RingOfferState::Unknown
    );
    assert!(matches!(
        producer.acquire_slot(5),
        Err(ProducerError::Closed)
    ));
    assert!(matches!(
        producer.acquire_owned_slot(8, 5),
        Err(ProducerError::Closed)
    ));
    assert_eq!(
        producer.upload(upload(), &[0; 8], 5),
        Err(ProducerError::Closed)
    );
    producer.expire(6);
    assert_eq!(producer.offers[0].state, Some(RingOfferState::Unknown));
}

#[test]
fn producer_drop_does_not_reclaim_the_workers_payload() {
    let mut producer = ready();
    let mut loan = producer.acquire_owned_slot(8, 3).unwrap();
    drop(producer);
    let returned = std::thread::spawn(move || {
        loan.pixels_mut().unwrap().fill(42);
        assert_eq!(loan.pixels_mut().unwrap(), &[42; 8]);
        loan
    })
    .join()
    .unwrap();
    drop(returned);
}

#[test]
fn invalid_lengths_and_failed_channel_send_preserve_custody() {
    let mut producer = ready();
    for len in [0, 1, 7, 4100] {
        assert!(producer.acquire_owned_slot(len, 3).is_err());
        assert!(producer.ready());
        assert!(producer.owned.is_none());
    }
    let loan = producer.acquire_owned_slot(8, 4).unwrap();
    let (send, recv) = std::sync::mpsc::sync_channel(1);
    drop(recv);
    let mut returned = send.send(loan).err().unwrap().0;
    producer.abort_owned_slot(&mut returned).unwrap();
    assert!(producer.ready());
    assert_eq!(producer.next_offer, 1);
}

#[test]
fn owned_payload_is_the_original_mapping_not_a_staging_buffer() {
    let mut producer = ready();
    let header = OWNED_RING_HEADER_BYTES as usize;
    let original = producer.ring.as_mut().unwrap().bytes_mut()[header..].as_mut_ptr();
    let mut loan = producer.acquire_owned_slot(8, 3).unwrap();
    assert!(producer.ring.is_none());
    assert_eq!(loan.pixels_mut().unwrap().as_mut_ptr(), original);
    producer.commit_owned_slot(&mut loan, upload(), 4).unwrap();
    assert_eq!(
        producer.ring.as_mut().unwrap().bytes_mut()[header..].as_mut_ptr(),
        original
    );
}

#[test]
fn an_old_consumed_handle_cannot_claim_the_next_fill() {
    let mut producer = ready();
    let mut old = producer.acquire_owned_slot(8, 3).unwrap();
    producer.abort_owned_slot(&mut old).unwrap();
    let mut next = producer.acquire_owned_slot(8, 4).unwrap();
    assert_eq!(
        producer.commit_owned_slot(&mut old, upload(), 5),
        Err(ProducerError::Refused)
    );
    assert_eq!(
        producer.abort_owned_slot(&mut old),
        Err(ProducerError::Refused)
    );
    drop(old);
    assert!(producer.ready());
    next.pixels_mut().unwrap().fill(47);
    assert_eq!(
        producer
            .commit_owned_slot(&mut next, upload(), 6)
            .unwrap()
            .0,
        1
    );
}

#[test]
fn full_offer_ledger_returns_the_completed_loan_for_retry() {
    let mut producer = ready();
    for n in 0..4 {
        let (offer, _) = producer.upload(upload(), &[1; 8], 3 + n * 3).unwrap();
        producer
            .resource_status(11, 1, ResourceStatusKind::Accepted, 4 + n * 3)
            .unwrap();
        producer
            .ring_event(released(offer, 2 * (n + 1)), 5 + n * 3)
            .unwrap();
    }
    let mut loan = producer.acquire_owned_slot(8, 15).unwrap();
    loan.pixels_mut().unwrap().fill(9);
    assert_eq!(
        producer.commit_owned_slot(&mut loan, upload(), 16),
        Err(ProducerError::Busy)
    );
    assert_eq!(loan.pixels_mut().unwrap(), &[9; 8]);
    assert_eq!(producer.next_offer, 5);
    assert_eq!(producer.slots[0].generation, 8);
    producer.forget(1).unwrap();
    assert_eq!(
        producer
            .commit_owned_slot(&mut loan, upload(), 17)
            .unwrap()
            .0,
        5
    );
    assert_eq!(producer.slots[0].generation, 10);
}

use super::*;

#[path = "frame_owned_images.rs"]
mod owned;

#[path = "frame_fallback.rs"]
mod fallback;

struct Fixture {
    c: ShellConnection,
    p: Peer,
    stream: FrameStream,
    key: ShellFrameStreamKey,
    record: ShellFrameRecord,
    upload: RingUpload,
    dir: PathBuf,
    _socket: OwnedFd,
    _source: OwnedFd,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new(name: &str) -> Self {
        let mut f = Self::registered(name);
        f.open();
        f
    }
    fn registered(name: &str) -> Self {
        let (mut c, mut p, dir, listener) = connect_role_image(
            name,
            4096,
            Profile {
                role: "bar",
                revision: 13,
                capabilities: BAR | OWNED | SHELL_FRAME_CAPABILITY,
                limits_published: true,
            },
            false,
        );
        let socket = drive_to_channel_ready(&mut c, &mut p, &listener);
        assert_eq!(
            RingRegistration::decode(&p.control_writes[0])
                .unwrap()
                .slots,
            2
        );
        let (source, _) = drive_to_ring_ready(&mut c, &mut p, &socket);
        let line = include_str!("../../../../spec/golden/sophia-frame-transactions-v1.tsv")
            .lines()
            .find(|l| l.starts_with("commit-copy\t"))
            .unwrap();
        let bytes: Vec<_> = line
            .split('\t')
            .nth(2)
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect();
        let ShellFrameRecord::Commit(mut value) =
            decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap()
        else {
            panic!("image golden")
        };
        value.transaction = TransactionId::from_raw(21);
        value.candidate.grant = ContentGrant {
            connection_epoch: EPOCH,
            content_grant_epoch: file_wire_peer::CONTENT_GRANT_EPOCH,
        };
        value.candidate.candidate_generation = 1;
        value.retire = None;
        let key = ShellFrameStreamKey {
            grant: value.candidate.grant,
            output: value.candidate.output,
            allocation: value.candidate.surfaces[0].allocation,
            facts_generation: value.candidate.facts_generation,
            stream_id: value.candidate.pacing_permit,
        };
        let image = value.image.take().unwrap();
        let upload = RingUpload {
            resource: image.resource,
            resource_generation: image.resource_generation,
            allocation: key.allocation.id,
            allocation_generation: key.allocation.generation,
            output: key.output.id,
            output_generation: key.output.generation,
            topology_generation: key.facts_generation,
            width: image.width,
            height: image.height,
        };
        Self {
            c,
            p,
            stream: FrameStream::new(),
            key,
            record: ShellFrameRecord::Commit(value),
            upload,
            dir,
            _socket: socket,
            _source: source,
        }
    }
    fn open(&mut self) {
        let ticket = self
            .stream
            .open(&mut self.c, TransactionId::from_raw(10), self.key)
            .unwrap()
            .first;
        self.custody(ticket);
        self.deliver(ShellFrameRecord::Opened {
            transaction: TransactionId::from_raw(10),
            key: self.key,
            ttl_millis: 100,
        })
        .unwrap();
    }
    fn custody(&mut self, ticket: Ticket) {
        drive(&mut self.c, &mut self.p, |_, p| !p.submits.is_empty());
        let held = self.p.submits.pop_front().unwrap();
        let submitted = decode_shell_file_submit(&held.data).unwrap();
        let kind = decode_shell_file_record_at_revision(
            self.p.transactions.last().unwrap(),
            ShellFileClass::Candidate,
            13,
        )
        .unwrap()
        .header
        .kind;
        self.p
            .answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
        let bytes = encode_shell_file_submitted_at_revision(
            ShellFileHeader {
                kind: ShellFileKind::Submitted,
                connection_epoch: EPOCH,
                submission_id: 0,
                sequence: self.p.take_sequence(),
            },
            ShellFileSubmitted {
                submission_id: submitted.submission_id,
                candidate_kind: kind,
            },
            13,
        )
        .unwrap();
        self.p.push(bytes);
        drive(&mut self.c, &mut self.p, |c, _| {
            c.custody(ticket) == Some(Custody::Submitted)
        });
    }
    fn deliver(&mut self, record: ShellFrameRecord) -> Result<(), ShellClientError> {
        let sequence = self.p.take_sequence();
        let bytes = encode_shell_frame_record(
            ShellFileHeader {
                kind: record.kind(),
                connection_epoch: EPOCH,
                submission_id: 0,
                sequence,
            },
            &record,
            13,
        )
        .unwrap();
        self.p.push(bytes);
        drive(&mut self.c, &mut self.p, |c, _| {
            c.journal_progress().handled >= sequence
        });
        self.stream.observe(&self.c.take_frame().unwrap().unwrap())
    }
    fn submit(&mut self) -> (Admission, u64) {
        self.stream
            .submit_ring(
                &mut self.c,
                &mut self.record,
                self.upload,
                Instant::now(),
                |pixels| pixels.fill(7),
            )
            .unwrap()
    }
    fn refused(&self, generation: u64) -> ShellFrameRecord {
        ShellFrameRecord::Refused {
            transaction: TransactionId::from_raw(21),
            grant: self.key.grant,
            output: self.key.output,
            stream_id: self.key.stream_id,
            candidate_generation: generation,
            request_kind: ShellFileKind::FrameCommit,
            reason: 1,
        }
    }
    fn outcome(&self, kind: u16) -> ShellFrameRecord {
        ShellFrameRecord::Outcome {
            transaction: TransactionId::from_raw(21),
            outcome: ContentCandidateOutcome {
                grant: self.key.grant,
                candidate_generation: 1,
                output: self.key.output,
                kind,
                reason: 0,
                presentation_epoch: u64::from(kind == 2),
                work_area_generation: 0,
                wm_commit_generation: 0,
            },
            stream_id: self.key.stream_id,
            allocation: self.key.allocation,
            facts_generation: self.key.facts_generation,
            credit_expires_msec: if kind == 2 { 100 } else { 0 },
        }
    }
}

#[test]
fn queue_refusal_and_fill_unwind_leave_source_and_credit_unspent() {
    let mut f = Fixture::new("frame-rollback");
    f.c.set_queue_limits(QueueLimits {
        records: 1,
        bytes: 8192,
        control_records: 0,
        control_bytes: 0,
    })
    .unwrap();
    let reservation = f.c.reserve_capacity(1, 8192).unwrap();
    assert_eq!(
        f.stream.submit_ring(
            &mut f.c,
            &mut f.record,
            f.upload,
            Instant::now(),
            |_| panic!("must not fill a refused frame")
        ),
        Err(ShellClientError::QueueSaturated)
    );
    f.c.cancel_reservation(&reservation).unwrap();
    let before = f.record.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.stream
                .submit_ring(&mut f.c, &mut f.record, f.upload, Instant::now(), |_| {
                    panic!("cancel fill")
                })
        }))
        .is_err()
    );
    assert_eq!(f.record, before);
    assert!(f.stream.has_credit());
    assert!(f.c.image_ring_offer(1).is_none());
    let (admission, offer) = f.submit();
    assert_eq!(offer, 1);
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
    assert_eq!(sent.image.unwrap().offer_id, 1);
    assert_eq!(sent.image.unwrap().slot_generation, 2);
    assert_eq!(
        f.p.control_writes.len(),
        1,
        "only registration, never a separate Offer"
    );
    assert!(!f.stream.has_credit());
    assert_eq!(
        f.c.image_upload_ring(f.upload, &[0; 8], Instant::now()),
        Err(ImageAdapterError::NotEnabled)
    );
}

#[test]
fn exact_frame_refusal_releases_source_without_reusing_its_identity() {
    let mut f = Fixture::new("frame-refused");
    let (admission, offer) = f.submit();
    f.custody(admission.first);
    f.deliver(f.refused(1)).unwrap();
    assert!(f.stream.has_credit());
    assert_eq!(
        f.c.image_ring_offer(offer).unwrap().state,
        RingOfferState::Refused
    );
    f.c.image_forget(offer).unwrap();
    if let ShellFrameRecord::Commit(v) = &mut f.record {
        v.transaction = TransactionId::from_raw(22);
        v.candidate.candidate_generation = 2;
    }
    let (_, next) = f.submit();
    assert_eq!(next, 2);
    let mut word = [0; 8];
    use std::os::unix::fs::FileExt;
    std::fs::File::from(f._source.try_clone().unwrap())
        .read_exact_at(&mut word, 0)
        .unwrap();
    assert_eq!(u64::from_le_bytes(word), 4);
}

#[test]
fn prepared_and_terminal_never_release_source_pixels() {
    let mut f = Fixture::new("frame-source-barrier");
    let (admission, offer) = f.submit();
    f.custody(admission.first);
    f.deliver(f.outcome(1)).unwrap();
    assert!(!f.stream.has_credit());
    f.deliver(f.outcome(2)).unwrap();
    assert!(f.stream.has_credit());
    assert_eq!(
        f.c.image_ring_offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    let bytes = f.p.event(
        ShellFileKind::ResourceStatus,
        &status_body(offer, f.upload.resource, f.upload.resource_generation, 2),
    );
    f.p.push(bytes);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(offer).unwrap().status.is_some()
    });
    assert_eq!(
        f.c.image_ring_offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    let bytes = ring_released(
        f.p.take_sequence(),
        offer,
        2,
        f.upload.resource,
        f.upload.resource_generation,
        0,
    );
    f.p.push(bytes);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(offer).unwrap().state == RingOfferState::Settled
    });
}

#[test]
fn contradictory_refusal_cannot_make_source_reusable() {
    for proof in 0..3 {
        let status_first = proof == 1;
        let mut f = Fixture::new(match proof {
            0 => "frame-wrong-refusal",
            1 => "frame-status-refusal",
            _ => "frame-prepared-refusal",
        });
        let (admission, offer) = f.submit();
        f.custody(admission.first);
        if status_first {
            let bytes = f.p.event(
                ShellFileKind::ResourceStatus,
                &status_body(offer, f.upload.resource, f.upload.resource_generation, 2),
            );
            f.p.push(bytes);
            drive(&mut f.c, &mut f.p, |c, _| {
                c.image_ring_offer(offer).unwrap().status.is_some()
            });
        }
        if proof == 2 {
            f.deliver(f.outcome(1)).unwrap();
        }
        let result = f.deliver(f.refused(if proof == 0 { 2 } else { 1 }));
        if !status_first {
            assert!(result.is_err());
        }
        assert_eq!(
            f.c.image_ring_offer(offer).unwrap().state,
            RingOfferState::Unknown
        );
        assert_eq!(
            f.c.image_adapter().unwrap().channel(),
            Some(sophia_image_producer::ChannelState::Closed)
        );
    }
}

#[test]
fn wire_refusal_frees_but_disconnect_keeps_unproven_source_unknown() {
    for closed in [false, true] {
        let mut f = Fixture::new(if closed {
            "frame-disconnect"
        } else {
            "frame-wire-refusal"
        });
        let (admission, offer) = f.submit();
        drive(&mut f.c, &mut f.p, |_, p| !p.submits.is_empty());
        let submit = f.p.submits.pop_front().unwrap();
        if closed {
            f.p.close();
            for _ in 0..20 {
                let _ = f.c.poll_io();
                if f.c.image_ring_offer(offer).unwrap().state == RingOfferState::Unknown {
                    break;
                }
            }
            assert_eq!(
                f.c.image_ring_offer(offer).unwrap().state,
                RingOfferState::Unknown
            );
        } else {
            f.p.answer_error(submit.tag, 22);
            drive(&mut f.c, &mut f.p, |c, _| {
                c.custody(admission.first) == Some(Custody::Refused(22))
            });
            assert_eq!(
                f.c.image_ring_offer(offer).unwrap().state,
                RingOfferState::Refused
            );
        }
        assert!(
            !f.stream.has_credit(),
            "transport refusal is not a semantic frame credit"
        );
    }
}

#[test]
fn both_slots_stay_busy_until_their_own_source_release() {
    let mut f = Fixture::new("frame-two-slots");
    let first_resource = f.upload;
    for generation in [1, 2] {
        if generation == 2 {
            f.upload.resource += 1;
            if let ShellFrameRecord::Commit(v) = &mut f.record {
                v.transaction = TransactionId::from_raw(22);
                v.candidate.candidate_generation = 2;
                v.candidate.placements[0].resource.id = f.upload.resource;
            }
        }
        let (admission, offer) = f.submit();
        assert_eq!(offer, generation);
        assert_eq!(
            f.c.image_ring_offer(offer).unwrap().slot,
            (generation - 1) as u32
        );
        f.custody(admission.first);
        for kind in [1, 2] {
            let mut event = f.outcome(kind);
            if let ShellFrameRecord::Outcome {
                transaction,
                outcome,
                ..
            } = &mut event
            {
                *transaction = TransactionId::from_raw(20 + generation);
                outcome.candidate_generation = generation;
            }
            f.deliver(event).unwrap();
        }
    }
    f.upload.resource += 1;
    if let ShellFrameRecord::Commit(v) = &mut f.record {
        v.transaction = TransactionId::from_raw(23);
        v.candidate.candidate_generation = 3;
        v.candidate.placements[0].resource.id = f.upload.resource;
    }
    assert_eq!(
        f.stream.submit_ring(
            &mut f.c,
            &mut f.record,
            f.upload,
            Instant::now(),
            |_| panic!("both sources owned")
        ),
        Err(ShellClientError::Image(ImageAdapterError::Busy))
    );
    assert!(f.stream.has_credit());
    let status = f.p.event(
        ShellFileKind::ResourceStatus,
        &status_body(
            1,
            first_resource.resource,
            first_resource.resource_generation,
            2,
        ),
    );
    f.p.push(status);
    let release = ring_released(
        f.p.take_sequence(),
        1,
        2,
        first_resource.resource,
        first_resource.resource_generation,
        0,
    );
    f.p.push(release);
    drive(&mut f.c, &mut f.p, |c, _| {
        c.image_ring_offer(1).unwrap().state == RingOfferState::Settled
    });
    f.c.image_forget(1).unwrap();
    let (_, offer) = f.submit();
    assert_eq!(offer, 3, "busy refusal did not spend the offer identity");
    assert_eq!(f.c.image_ring_offer(offer).unwrap().slot, 0);
    assert_eq!(
        f.c.image_ring_offer(2).unwrap().state,
        RingOfferState::Pending
    );
}

//! Test-only real-export join. Supplied negotiation is not authenticated admission.
use super::*;
use sophia_control_ring_probe::{
    capture::{Captured, EXTENT, HEADER, Identity, ReadPlan, Source},
    custody::{Client, Outcome, Receipt, Slot},
};
use sophia_linux_peer::ProducerRing;
use std::fs::File;

fn lean_export() -> ShellFiles {
    let mut files = export();
    files.capabilities |= SHELL_FRAME_LEAN_CAPABILITY | SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
    files
}
fn open_writer(files: &mut ShellFiles) -> u64 {
    match files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap() {
        Handle::ReusableTransaction(id) => id,
        _ => panic!("reusable writer expected"),
    }
}
fn commit(id: u64, ack: u64) -> Vec<u8> {
    let mut bytes = golden("commit-copy-retire");
    bytes[16..24].copy_from_slice(&id.to_le_bytes());
    bytes[24..32].copy_from_slice(&ack.to_le_bytes());
    bytes
}
fn control(files: &mut ShellFiles, bytes: &[u8]) -> Result<u32, Errno> {
    files.write(&Node::FrameSubmit, &mut Handle::Plain, 0, bytes)
}
struct Carrier {
    producer: ProducerRing,
    source: Source,
    captured: Captured,
    slot: Slot,
    writer: u64,
}
impl Carrier {
    fn new(files: &mut ShellFiles) -> Self {
        Self::for_writer(open_writer(files))
    }
    fn for_writer(writer: u64) -> Self {
        let producer = ProducerRing::create(EXTENT).unwrap();
        let source = Source::admit(File::from(
            producer.source_fd().try_clone_to_owned().unwrap(),
        ))
        .unwrap();
        Self {
            producer,
            source,
            captured: Captured::buffer(),
            slot: Slot::default(),
            writer,
        }
    }
    fn submit(&mut self, files: &mut ShellFiles, bytes: &[u8]) -> Receipt {
        let header = decode_shell_file_record_at_revision(
            bytes,
            ShellFileClass::Candidate,
            SHELL_FRAME_REVISION,
        )
        .unwrap()
        .header;
        let identity = Identity {
            sequence: self.slot.last_sequence() + 1,
            epoch: header.connection_epoch,
            writer: self.writer,
            submission: header.submission_id,
            ack: header.sequence,
            bytes: bytes.len() as u64,
        };
        self.producer.bytes_mut()[..HEADER].copy_from_slice(&identity.header());
        self.producer.bytes_mut()[HEADER..HEADER + bytes.len()].copy_from_slice(bytes);
        self.slot
            .capture_published(
                identity.sequence,
                &self.source,
                &mut self.captured,
                ReadPlan::HeaderThenPayload,
            )
            .unwrap();
        self.slot
            .process(&self.captured, |record| {
                // The carrier binds to the existing writer, not a cached negotiation flag.
                files.read_reusable_transaction(identity.writer, 0, 0)?;
                files.write(&Node::FrameSubmit, &mut Handle::Plain, 0, record)
            })
            .unwrap();
        self.slot.publish(true).unwrap()
    }
}
fn equal(control: &ShellFiles, candidate: &ShellFiles) {
    assert_eq!(journal_bytes(control), journal_bytes(candidate));
    assert_eq!(control.submission_watermark, candidate.submission_watermark);
    assert_eq!(control.inbound.len(), candidate.inbound.len());
    assert_eq!(
        control.staging.as_ref().map(|s| &s.bytes),
        candidate.staging.as_ref().map(|s| &s.bytes)
    );
    assert_eq!(
        control.accepted.as_ref().map(|s| &s.bytes),
        candidate.accepted.as_ref().map(|s| &s.bytes)
    );
    assert_eq!(
        control.lean_frames_in_flight,
        candidate.lean_frames_in_flight
    );
}
fn agrees(expected: Result<u32, Errno>, receipt: Receipt, bytes: &[u8]) {
    let expected = match expected {
        Ok(count) => {
            assert_eq!(count as usize, bytes.len());
            Outcome::Accepted {
                applied_ack: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
            }
        }
        Err(error) => Outcome::Refused(error),
    };
    assert_eq!(receipt.outcome, expected);
}

#[test]
fn private_snapshot_reaches_host_decoder_without_a_second_admission_owner() {
    let mut a = lean_export();
    let mut b = lean_export();
    open_writer(&mut a);
    let mut carrier = Carrier::new(&mut b);
    let pointer = carrier.captured.bytes.as_ptr();
    for id in [20, 21] {
        let bytes = commit(id, 0);
        agrees(
            control(&mut a, &bytes),
            carrier.submit(&mut b, &bytes),
            &bytes,
        );
        equal(&a, &b);
        assert_eq!(carrier.captured.bytes.as_ptr(), pointer);
        assert!(a.accepted.is_none() && b.accepted.is_none());
    }
    assert_eq!(a.take_inbound(), b.take_inbound());
}

#[test]
fn real_backpressure_changed_retry_and_success_have_identical_effects() {
    let mut a = lean_export();
    let mut b = lean_export();
    open_writer(&mut a);
    let mut carrier = Carrier::new(&mut b);
    let bytes = commit(20, 0);
    let value = decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap();
    for files in [&mut a, &mut b] {
        files
            .inbound
            .extend((0..INBOUND_RECORDS).map(|_| Inbound::Frame(Box::new(value.clone()))));
    }
    for _ in 0..2 {
        let expected = control(&mut a, &bytes);
        assert_eq!(expected, Err(Errno::EAGAIN));
        agrees(expected, carrier.submit(&mut b, &bytes), &bytes);
        equal(&a, &b);
    }
    let mut changed = bytes.clone();
    changed[32..40].copy_from_slice(&99u64.to_le_bytes());
    let expected = control(&mut a, &changed);
    assert_eq!(expected, Err(Errno::EINVAL));
    agrees(expected, carrier.submit(&mut b, &changed), &changed);
    equal(&a, &b);
    a.inbound.clear();
    b.inbound.clear();
    agrees(
        control(&mut a, &bytes),
        carrier.submit(&mut b, &bytes),
        &bytes,
    );
    equal(&a, &b);
    assert_eq!(a.take_inbound(), b.take_inbound());
}

#[test]
fn real_duplicate_stages_and_poisons_writer_and_client_fails_unknown() {
    let mut a = lean_export();
    let mut b = lean_export();
    open_writer(&mut a);
    let mut carrier = Carrier::new(&mut b);
    let bytes = commit(20, 0);
    agrees(
        control(&mut a, &bytes),
        carrier.submit(&mut b, &bytes),
        &bytes,
    );
    let expected = control(&mut a, &bytes);
    assert_eq!(expected, Err(EALREADY));
    let receipt = carrier.submit(&mut b, &bytes);
    agrees(expected, receipt, &bytes);
    equal(&a, &b);
    assert!(b.staging.is_some());
    let mut client = Client::default();
    client.begin(receipt.identity).unwrap();
    publish_client(&mut client);
    client.observe(receipt).unwrap();
    assert!(client.closed && client.unknown);
    let next = commit(21, 0);
    assert_eq!(control(&mut a, &next), Err(EBUSY));
    assert_eq!(
        carrier.submit(&mut b, &next).outcome,
        Outcome::Refused(EBUSY)
    );
    equal(&a, &b);
}

#[test]
fn real_expiry_and_writer_clunk_refuse_then_same_attach_can_reopen() {
    for expire in [false, true] {
        let mut a = lean_export();
        let mut b = lean_export();
        let a_writer = open_writer(&mut a);
        let mut carrier = Carrier::new(&mut b);
        let bytes = commit(20, 0);
        for (files, writer) in [(&mut a, a_writer), (&mut b, carrier.writer)] {
            if expire {
                let mut staging = Staging::new(writer, STAGING);
                staging
                    .write(
                        0,
                        &bytes,
                        Instant::now() - STAGING.assembly - Duration::from_secs(1),
                    )
                    .unwrap();
                files.staging = Some(staging);
            } else {
                files.release(
                    Node::TransactionReuse,
                    Some(Handle::ReusableTransaction(writer)),
                );
            }
        }
        assert_eq!(control(&mut a, &bytes), Err(Errno::ESTALE));
        assert_eq!(
            carrier.submit(&mut b, &bytes).outcome,
            Outcome::Refused(Errno::ESTALE)
        );
        equal(&a, &b);
        // Expire drops the writer as well as staging; a fresh writer is required.
        open_writer(&mut a);
        carrier.writer = open_writer(&mut b);
        agrees(
            control(&mut a, &bytes),
            carrier.submit(&mut b, &bytes),
            &bytes,
        );
        equal(&a, &b);
    }
}

#[test]
fn cq_spends_no_journal_capacity_and_preserves_lean_publication_hold() {
    let mut a = lean_export();
    let mut b = lean_export();
    open_writer(&mut a);
    let mut carrier = Carrier::new(&mut b);
    let bytes = commit(20, 0);
    let before = journal_bytes(&a);
    agrees(
        control(&mut a, &bytes),
        carrier.submit(&mut b, &bytes),
        &bytes,
    );
    assert_eq!(journal_bytes(&b), before);
    for files in [&mut a, &mut b] {
        files
            .with_frame_commit(|input| {
                input.take();
                Ok(crate::ContentFrameAdmission::Accepted { copy_job: None })
            })
            .unwrap();
        assert!(!files.publish_image_records());
        files
            .append_event(ShellFileKind::CandidateOutcome, &[0; 40], false)
            .unwrap();
        assert!(!files.publish_image_records());
    }
    equal(&a, &b);
    for files in [&mut a, &mut b] {
        files
            .append_event(ShellFileKind::FrameOutcome, &[0; 116], false)
            .unwrap();
        assert!(files.publish_image_records());
    }
    equal(&a, &b);
}

#[test]
fn real_ack_effect_survives_backpressure_but_receipt_confirms_only_success() {
    let mut a = lean_export();
    let mut b = lean_export();
    let setup = |files: &mut ShellFiles| {
        let (writer, submit) = stage(files, &golden("open"));
        files.submit_transaction(&submit, false).unwrap();
        let sequence = files.accepted.as_ref().unwrap().sequence;
        let Handle::ReusableTransaction(id) = writer else {
            panic!("writer");
        };
        (id, sequence)
    };
    let (_, ack) = setup(&mut a);
    let (writer, other_ack) = setup(&mut b);
    assert_eq!(ack, other_ack);
    let mut carrier = Carrier::for_writer(writer);
    let bytes = commit(20, ack);
    let value = decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap();
    for files in [&mut a, &mut b] {
        while files.inbound.len() < INBOUND_RECORDS {
            files
                .inbound
                .push_back(Inbound::Frame(Box::new(value.clone())));
        }
    }
    let expected = control(&mut a, &bytes);
    assert_eq!(expected, Err(Errno::EAGAIN));
    let receipt = carrier.submit(&mut b, &bytes);
    agrees(expected, receipt, &bytes);
    equal(&a, &b);
    assert!(
        a.accepted.is_none() && b.accepted.is_none(),
        "ACK recycled the Open before refused admission"
    );
    let mut client = Client::default();
    client.begin(receipt.identity).unwrap();
    publish_client(&mut client);
    client.observe(receipt).unwrap();
    assert_eq!(client.confirmed_ack, 0);
    a.inbound.clear();
    b.inbound.clear();
    let receipt = carrier.submit(&mut b, &bytes);
    agrees(control(&mut a, &bytes), receipt, &bytes);
    client.begin(receipt.identity).unwrap();
    publish_client(&mut client);
    client.observe(receipt).unwrap();
    assert_eq!(client.confirmed_ack, ack);
    equal(&a, &b);
}

#[test]
fn transport_close_does_not_roll_back_export_custody_or_publish_after_withdrawal() {
    let mut files = lean_export();
    let mut carrier = Carrier::new(&mut files);
    let bytes = commit(20, 0);
    let receipt = carrier.submit(&mut files, &bytes);
    assert!(matches!(receipt.outcome, Outcome::Accepted { .. }));
    let watermark = files.submission_watermark;
    carrier.slot.close();
    assert!(carrier.slot.publish(true).is_none());
    assert_eq!(files.submission_watermark, watermark);
    assert_eq!(
        files.inbound.len(),
        1,
        "transport teardown is not owner cancellation"
    );
    assert!(
        carrier
            .slot
            .capture_published(
                carrier.slot.last_sequence() + 1,
                &carrier.source,
                &mut carrier.captured,
                ReadPlan::HeaderThenPayload
            )
            .is_err()
    );
}

#[path = "shell_frame_control_carrier_process.rs"]
mod process;

fn publish_client(client: &mut Client) {
    let (_reader, writer) = sophia_control_ring_probe::wake::pipe().unwrap();
    client.publish(&writer).unwrap();
}

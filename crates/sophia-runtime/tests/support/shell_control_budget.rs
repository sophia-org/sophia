use super::*;
use sophia_protocol::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    transport: ShellSessionTransport,
    directory: std::path::PathBuf,
}
impl Fixture {
    fn new(records: u32) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-control-budget-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellSessionTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::getuid().as_raw(),
        )
        .unwrap();
        let grant = ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        };
        let mut limits = ContentLimits::prototype(grant);
        limits.max_control_records = records;
        transport.content_epochs.admit(limits.clone()).unwrap();
        transport.content_grant = Some(grant);
        transport.content_limits = Some(limits);
        Self {
            transport,
            directory,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn all_store_credits_and_fifo_frames_share_one_capacity() {
    let mut fixture = Fixture::new(7);
    let transport = &mut fixture.transport;
    let grant = transport.content_grant.unwrap();
    transport
        .content_epochs
        .active_mut()
        .unwrap()
        .begin(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant,
                resource: ContentResourceId {
                    id: 1,
                    generation: 1,
                },
                width_px: 1,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 4,
            },
            0,
        )
        .unwrap();
    transport
        .content_epochs
        .active_candidates_mut()
        .unwrap()
        .grant_permit(
            TransactionId::from_raw(2),
            ContentOutputId {
                id: 1,
                generation: 1,
            },
            1,
            1,
            0,
        )
        .unwrap();
    transport
        .content_epochs
        .active_allocations_mut()
        .unwrap()
        .publish_outputs(
            TransactionId::from_raw(3),
            1,
            vec![ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
                local_width: 100,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    assert_eq!(transport.content_epochs.active_control_occupancy(), 7);
    let owned = transport.content_accounting();
    assert_eq!(owned.response_records, 7);
    assert_eq!(owned.epochs.transfers, 1);
    assert_eq!(owned.epochs.permits, 1);
    assert!(transport.control_capacity_available(0));
    assert!(!transport.content_action_capacity_available());
    let event = transport
        .content_epochs
        .active()
        .unwrap()
        .pending_event()
        .unwrap()
        .clone();
    transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 6;
    assert!(
        transport
            .queue_content_record(event.transaction, &event.record, true)
            .is_err()
    );
    assert_eq!(transport.content_epochs.active_control_occupancy(), 7);
    assert_eq!(transport.output.controls(), 0);
    assert_eq!(transport.content_accounting(), owned);
    assert_eq!(
        transport.content_epochs.active().unwrap().pending_event(),
        Some(&event)
    );
    transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 7;
    transport
        .queue_content_record(event.transaction, &event.record, true)
        .unwrap();
    transport.content_epochs.active_mut().unwrap().take_event();
    assert_eq!(transport.content_epochs.active_control_occupancy(), 6);
    assert_eq!(transport.output.controls(), 1);
    let transferred = transport.content_accounting();
    assert_eq!(transferred.response_records, owned.response_records);
    assert_eq!(transferred.response_bytes, owned.response_bytes);
    assert!(!transport.control_capacity_available(1));
    let frame_bytes = transport.output.front().len();
    transport.output.written(frame_bytes - 1);
    assert_eq!(transport.content_accounting(), transferred);
    assert!(!transport.control_capacity_available(1));
    transport.output.written(1);
    assert_eq!(transport.content_accounting().response_records, 6);
    assert_eq!(
        transport.content_accounting().response_bytes,
        transferred.response_bytes - CONTROL_FRAME_BYTES
    );
    assert!(transport.control_capacity_available(1));
    assert!(!transport.control_capacity_available(2));
}

#[test]
fn byte_budget_can_exhaust_before_record_budget_and_bulk_cannot_spend_it() {
    let mut fixture = Fixture::new(64);
    let transport = &mut fixture.transport;
    let limits = transport.content_limits.as_mut().unwrap();
    limits.reserved_control_queue_bytes = 1024;
    limits.max_output_queue_bytes = 2048;
    assert!(transport.control_capacity_available(4));
    assert!(!transport.control_capacity_available(5));
    assert!(transport.bulk_capacity_available(1024));
    assert!(!transport.bulk_capacity_available(1025));
    transport.output.push(vec![0; 1024], false);
    assert!(!transport.bulk_capacity_available(1));
    assert!(transport.control_capacity_available(4));
}

#[test]
fn cancellation_reserve_survives_bulk_and_partial_write_until_exact_transfer() {
    let mut fixture = Fixture::new(3);
    let transport = &mut fixture.transport;
    let action = ContentAction {
        grant: transport.content_grant.unwrap(),
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        candidate_generation: 1,
        presentation_epoch: 1,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        event_id: 1,
        kind: 1,
        reason: 0,
    };
    transport
        .send_content_action(TransactionId::from_raw(1), &action)
        .unwrap();
    assert_eq!(transport.output.records(), 1);
    assert_eq!(transport.action_cancellations.len(), 1);
    assert!(transport.bulk_capacity_available(8));
    transport.output.push(vec![0; 8], false);
    assert!(!transport.bulk_capacity_available(1));
    assert!(!transport.control_capacity_available(1));
    let mut cancel = action.clone();
    cancel.kind = 3;
    cancel.reason = ContentReason::Revoked as u16;
    let mut wrong = cancel.clone();
    wrong.target_generation += 1;
    assert!(
        transport
            .send_content_action(TransactionId::from_raw(2), &wrong)
            .is_err()
    );
    assert_eq!(
        transport
            .action_cancellations
            .iter()
            .find(|pending| pending.event_id == 1),
        Some(&action)
    );
    transport
        .send_content_action(TransactionId::from_raw(3), &cancel)
        .unwrap();
    assert!(transport.action_cancellations.is_empty());
    assert_eq!(transport.output.records(), 3);
    assert!(!transport.control_capacity_available(1));
    let remaining = transport.output.front().len();
    transport.output.written(remaining - 1);
    assert!(!transport.control_capacity_available(1));
    transport.output.written(1);
    assert!(transport.control_capacity_available(1));
    // Peer loss settles delivery as lost, never as sent, and leaves no charge
    // available to be freed a second time by the next grant.
    transport.disconnect().unwrap();
    assert_eq!(transport.output.records(), 0);
    assert_eq!(transport.output.len(), 0);
    assert_eq!(transport.content_epochs.active_control_occupancy(), 0);
    assert!(transport.action_cancellations.is_empty());
}

#[test]
fn a_reserved_allocation_rejection_progresses_at_the_aggregate_record_limit() {
    let mut fixture = Fixture::new(2);
    let transport = &mut fixture.transport;
    let grant = transport.content_grant.unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    let store = transport.content_epochs.active_allocations_mut().unwrap();
    store
        .publish_outputs(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output,
                local_width: 200,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    store.take_event().unwrap();
    store
        .request(
            TransactionId::from_raw(2),
            ContentAllocationRequest {
                grant,
                output,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: 200,
                desired_height: 20,
                margins: ContentMargins::default(),
            },
            &[],
            0,
        )
        .unwrap();
    transport.output.push(vec![0; 8], false);
    assert!(!transport.control_capacity_available(1));
    transport
        .reject_content_allocation(1, crate::ContentAllocationError::Budget)
        .unwrap();
    assert_eq!(transport.content_epochs.active_control_occupancy(), 0);
    assert_eq!(transport.output.records(), 2);
    assert!(!transport.control_capacity_available(1));
    transport.output.written(8);
    let (_, record) = decode_shell_content_frame(transport.output.front()).unwrap();
    assert!(matches!(record, ShellContentRecord::AllocationResult(value)
        if value.allocation_request_id == 1 && value.status == 2));
}

#[test]
fn refused_native_outcome_retains_candidate_then_publishes_once_before_action() {
    let mut fixture = Fixture::new(6);
    let transport = &mut fixture.transport;
    let grant = transport.content_grant.unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    transport
        .grant_content_permit(TransactionId::from_raw(1), output, 1, 1, 0)
        .unwrap();
    let (resources, candidates) = transport.content_epochs.active_parts_mut().unwrap();
    candidates
        .begin(
            TransactionId::from_raw(2),
            ContentCandidateBegin {
                grant,
                candidate_generation: 1,
                output,
                facts_generation: 1,
                pacing_permit: 1,
                interaction_generation: 1,
                surface_count: 0,
                placement_count: 0,
                target_count: 0,
            },
            1,
        )
        .unwrap();
    candidates
        .end(
            TransactionId::from_raw(3),
            ContentCandidateEnd {
                grant,
                candidate_generation: 1,
                surface_count: 0,
                placement_count: 0,
                target_count: 0,
            },
            crate::ContentCandidateContext {
                output,
                facts_generation: 1,
                interaction_generation: 1,
                allocations: &[],
            },
            resources,
            1,
        )
        .unwrap();
    // This is a real reducer submission with a simulated native completion;
    // no claim of compositor/Session owner-loop or hardware coverage.
    let bundle = transport.begin_content_submission(output, 1, 2).unwrap();
    transport
        .content_prepared(grant, output, 1, 1, 1, 2)
        .unwrap();
    transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 2;
    assert!(
        transport
            .content_presented(grant, output, 1, 1, 1, 1)
            .is_err()
    );
    assert_eq!(
        transport
            .content_epochs
            .active_candidates()
            .unwrap()
            .submitted_candidate_count(),
        1
    );
    assert_eq!(transport.output.records(), 2); // Permit and Prepared only.
    transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 6;
    transport
        .content_presented(grant, output, 1, 1, 1, 1)
        .unwrap();
    let action = ContentAction {
        grant,
        output,
        candidate_generation: 1,
        presentation_epoch: 1,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        event_id: 1,
        kind: 1,
        reason: 0,
    };
    // Transport owns ordering/credit, not the Session target-admission rule.
    transport
        .send_content_action(TransactionId::from_raw(4), &action)
        .unwrap();
    assert!(
        transport
            .content_presented(grant, output, 1, 1, 1, 1)
            .is_err()
    );
    let mut records = Vec::new();
    while !transport.output.is_empty() {
        let frame = transport.output.front();
        records.push(decode_shell_content_frame(frame).unwrap());
        let bytes = frame.len();
        transport.output.written(bytes);
    }
    assert_eq!(records.len(), 4);
    assert!(
        matches!(&records[2], (transaction, ShellContentRecord::CandidateOutcome(value))
        if transaction.raw() == 2 && value.kind == 2 && value.presentation_epoch == 1)
    );
    assert!(matches!(&records[3].1, ShellContentRecord::Action(value) if value == &action));
    drop(bundle);
}

#[test]
fn complete_inbox_frames_and_partial_input_share_the_advertised_byte_budget() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};
    let mut fixture = Fixture::new(64);
    let transport = &mut fixture.transport;
    let grant = transport.content_grant.unwrap();
    let (local, mut peer) = UnixStream::pair().unwrap();
    local.set_nonblocking(true).unwrap();
    transport.stream = Some(local);
    let writer = std::thread::spawn(move || {
        for transaction in 1..=8 {
            let frame = encode_shell_content_frame(
                TransactionId::from_raw(transaction),
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant,
                    resource: ContentResourceId {
                        id: 1,
                        generation: 1,
                    },
                    ordinal: transaction as u32 - 1,
                    offset: (transaction - 1) * 65488,
                    bytes: vec![42; 65488],
                }),
            )
            .unwrap();
            peer.write_all(&frame).unwrap();
        }
    });
    let limit = transport
        .content_limits
        .as_ref()
        .unwrap()
        .max_input_queue_bytes as usize;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        transport.poll_io().unwrap();
        let retained = transport.input.len() + transport.inbox.iter().map(Vec::len).sum::<usize>();
        let accounting = transport.content_accounting();
        assert_eq!(accounting.input_bytes, retained);
        assert_eq!(accounting.input_records, transport.inbox.len());
        assert!(retained <= limit);
        if retained == limit {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "private peer did not fill the input budget"
        );
        std::thread::yield_now();
    }
    assert!(
        !transport.input.is_empty(),
        "the second whole frame cannot yet fit"
    );
    let mut received = 0;
    while received < 8 {
        transport.poll_io().unwrap();
        assert!(
            transport.input.len() + transport.inbox.iter().map(Vec::len).sum::<usize>() <= limit
        );
        if let Some(frame) = transport.inbox.pop_front() {
            received += 1;
            let (transaction, _) = decode_shell_content_frame(&frame).unwrap();
            assert_eq!(transaction.raw(), received);
        }
        assert!(
            Instant::now() < deadline,
            "backpressure lost a complete or partial frame"
        );
    }
    writer.join().unwrap();
}

use super::*;
use crate::ShellSessionTransport;
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
        transport.state.store_grant = grant;
        transport.state.content_grant = Some(grant);
        transport.state.content_limits = Some(limits);
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
fn final_collection_refuses_a_live_socket_even_without_a_content_epoch() {
    let mut fixture = Fixture::new(7);
    let transport = &mut fixture.transport;
    let owner = Box::new([41u8; 8]);
    let address = owner.as_ptr();
    let owner = transport
        .finish_content_after_backend_drop(owner)
        .unwrap_err();
    assert_eq!(owner.as_ptr(), address);
    transport.disconnect().unwrap();
    let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    transport.state.stream = Some(socket);
    let owner = transport
        .finish_content_after_backend_drop(owner)
        .unwrap_err();
    assert_eq!(owner.as_ptr(), address);
    assert!(transport.state.stream.is_some());
    transport.disconnect().unwrap();
    let report = transport.finish_content_after_backend_drop(owner).unwrap();
    assert_eq!(report.settled_candidates, 0);
    assert!(report.accounting.quiescent());
}

#[test]
fn all_store_credits_and_fifo_frames_share_one_capacity() {
    let mut fixture = Fixture::new(7);
    let transport = &mut fixture.transport;
    let grant = transport.state.content_grant.unwrap();
    transport
        .content_epochs
        .resources_mut(transport.state.store_grant)
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
        .active_candidates_mut(transport.state.store_grant)
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
        .allocations_mut(transport.state.store_grant)
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
    assert_eq!(
        transport
            .content_epochs
            .control_occupancy(transport.state.store_grant),
        7
    );
    let owned = transport.content_accounting();
    assert_eq!(owned.response_records, 7);
    assert_eq!(owned.epochs.transfers, 1);
    assert_eq!(owned.epochs.permits, 1);
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 0)
    );
    assert!(!transport.content_action_capacity_available());
    let event = transport
        .content_epochs
        .resources(transport.state.store_grant)
        .unwrap()
        .pending_event()
        .unwrap()
        .clone();
    transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 6;
    assert!(
        transport
            .state
            .queue_content_record(
                &mut transport.content_epochs,
                event.transaction,
                &event.record,
                true
            )
            .is_err()
    );
    assert_eq!(
        transport
            .content_epochs
            .control_occupancy(transport.state.store_grant),
        7
    );
    assert_eq!(transport.state.output.controls(), 0);
    assert_eq!(transport.content_accounting(), owned);
    assert_eq!(
        transport
            .content_epochs
            .resources(transport.state.store_grant)
            .unwrap()
            .pending_event(),
        Some(&event)
    );
    transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 7;
    transport
        .state
        .queue_content_record(
            &mut transport.content_epochs,
            event.transaction,
            &event.record,
            true,
        )
        .unwrap();
    transport
        .content_epochs
        .resources_mut(transport.state.store_grant)
        .unwrap()
        .take_event();
    assert_eq!(
        transport
            .content_epochs
            .control_occupancy(transport.state.store_grant),
        6
    );
    assert_eq!(transport.state.output.controls(), 1);
    let transferred = transport.content_accounting();
    assert_eq!(transferred.response_records, owned.response_records);
    assert_eq!(transferred.response_bytes, owned.response_bytes);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    let frame_bytes = transport.state.output.front().len();
    transport.state.output.written(frame_bytes - 1);
    assert_eq!(transport.content_accounting(), transferred);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    transport.state.output.written(1);
    assert_eq!(transport.content_accounting().response_records, 6);
    assert_eq!(
        transport.content_accounting().response_bytes,
        transferred.response_bytes - CONTROL_FRAME_BYTES
    );
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 2)
    );
}

#[test]
fn byte_budget_can_exhaust_before_record_budget_and_bulk_cannot_spend_it() {
    let mut fixture = Fixture::new(64);
    let transport = &mut fixture.transport;
    let limits = transport.state.content_limits.as_mut().unwrap();
    limits.reserved_control_queue_bytes = 1024;
    limits.max_output_queue_bytes = 2048;
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 4)
    );
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 5)
    );
    assert!(
        transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 1024)
    );
    assert!(
        !transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 1025)
    );
    transport.state.output.push(vec![0; 1024], false);
    assert!(
        !transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 1)
    );
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 4)
    );
}

#[test]
fn cancellation_reserve_survives_bulk_and_partial_write_until_exact_transfer() {
    let mut fixture = Fixture::new(3);
    let transport = &mut fixture.transport;
    let action = ContentAction {
        grant: transport.state.content_grant.unwrap(),
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
    assert_eq!(transport.state.output.records(), 1);
    assert_eq!(transport.state.action_cancellations.len(), 1);
    assert!(
        transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 8)
    );
    transport.state.output.push(vec![0; 8], false);
    assert!(
        !transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 1)
    );
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
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
            .state
            .action_cancellations
            .iter()
            .find(|pending| pending.event_id == 1),
        Some(&action)
    );
    transport
        .send_content_action(TransactionId::from_raw(3), &cancel)
        .unwrap();
    assert!(transport.state.action_cancellations.is_empty());
    assert_eq!(transport.state.output.records(), 3);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    let remaining = transport.state.output.front().len();
    transport.state.output.written(remaining - 1);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    transport.state.output.written(1);
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    // Peer loss settles delivery as lost, never as sent, and leaves no charge
    // available to be freed a second time by the next grant.
    transport.disconnect().unwrap();
    assert_eq!(transport.state.output.records(), 0);
    assert_eq!(transport.state.output.len(), 0);
    assert_eq!(
        transport
            .content_epochs
            .control_occupancy(transport.state.store_grant),
        0
    );
    assert!(transport.state.action_cancellations.is_empty());
}

#[test]
fn a_reserved_allocation_rejection_progresses_at_the_aggregate_record_limit() {
    let mut fixture = Fixture::new(2);
    let transport = &mut fixture.transport;
    let grant = transport.state.content_grant.unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    let store = transport
        .content_epochs
        .allocations_mut(transport.state.store_grant)
        .unwrap();
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
    transport.state.output.push(vec![0; 8], false);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    transport
        .reject_content_allocation(1, crate::ContentAllocationError::Budget)
        .unwrap();
    assert_eq!(
        transport
            .content_epochs
            .control_occupancy(transport.state.store_grant),
        0
    );
    assert_eq!(transport.state.output.records(), 2);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    transport.state.output.written(8);
    let (_, record) = decode_shell_content_frame(transport.state.output.front()).unwrap();
    assert!(matches!(record, ShellContentRecord::AllocationResult(value)
        if value.allocation_request_id == 1 && value.status == 2));
}

#[test]
fn refused_native_outcome_retains_candidate_then_publishes_once_before_action() {
    let mut fixture = Fixture::new(6);
    let transport = &mut fixture.transport;
    let grant = transport.state.content_grant.unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    transport
        .grant_content_permit(TransactionId::from_raw(1), output, 1, 1, 0)
        .unwrap();
    let (resources, candidates) = transport
        .content_epochs
        .active_parts_mut(transport.state.store_grant)
        .unwrap();
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
        .state
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
            .active_candidates(transport.state.store_grant)
            .unwrap()
            .submitted_candidate_count(),
        1
    );
    assert_eq!(transport.state.output.records(), 2); // Permit and Prepared only.
    transport
        .state
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
    while !transport.state.output.is_empty() {
        let frame = transport.state.output.front();
        records.push(decode_shell_content_frame(frame).unwrap());
        let bytes = frame.len();
        transport.state.output.written(bytes);
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
    let grant = transport.state.content_grant.unwrap();
    let (local, mut peer) = UnixStream::pair().unwrap();
    local.set_nonblocking(true).unwrap();
    transport.state.stream = Some(local);
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
        .state
        .content_limits
        .as_ref()
        .unwrap()
        .max_input_queue_bytes as usize;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        transport.poll_io().unwrap();
        let retained =
            transport.state.input.len() + transport.state.inbox.iter().map(Vec::len).sum::<usize>();
        let accounting = transport.content_accounting();
        assert_eq!(accounting.input_bytes, retained);
        assert_eq!(accounting.input_records, transport.state.inbox.len());
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
        !transport.state.input.is_empty(),
        "the second whole frame cannot yet fit"
    );
    let mut received = 0;
    while received < 8 {
        transport.poll_io().unwrap();
        assert!(
            transport.state.input.len() + transport.state.inbox.iter().map(Vec::len).sum::<usize>()
                <= limit
        );
        if let Some(frame) = transport.state.inbox.pop_front() {
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

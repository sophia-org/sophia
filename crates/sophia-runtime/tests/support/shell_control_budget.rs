use super::super::outbound::{Admitted, OutboundRecord};
use super::super::outbox::tests::files::Peer;
use super::*;
use crate::ShellSessionTransport;
use crate::test_content_isb as isb;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn attach_files(transport: &mut ShellSessionTransport) -> Peer {
    Peer::attach(
        &mut transport.state,
        Some(crate::ContentStoreProfile::Legacy),
    )
}

#[test]
fn a_role_visit_cannot_timeout_an_acknowledgement_it_did_not_service() {
    let mut fixture = Fixture::new(7);
    let transport = &mut fixture.transport;
    let _peer = attach_files(transport);
    let record = transport
        .state
        .admit_record(
            OutboundRecord::Content(
                TransactionId::from_raw(1),
                ShellContentRecord::Action(ContentAction {
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
                }),
            ),
            super::Class::Control {
                limit: CONTROL_RECORD_BYTES,
                oversize: ShellTransportError::WrongContentRecord,
            },
        )
        .unwrap();
    Peer::fill_journal(&mut transport.state, &record.record);
    transport.state.output.push(record);
    let now = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let state = &mut transport.state;
    let files = state.wire.as_mut().unwrap();
    assert_eq!(
        ShellComponentTransport::turn_files(files, &mut state.output, false, now, None).unwrap(),
        (false, true)
    );
    assert_eq!(state.output.records(), 1);
    // Once input was served, a peer that really never acknowledges is still
    // refused. Sharing role input must not disable the existing deadline.
    assert!(matches!(
        ShellComponentTransport::turn_files(files, &mut state.output, true, now, None),
        Err(ShellTransportError::ContentQueueSaturated)
    ));
}

/// A real typed bulk record: output facts with `outputs` rows, admitted as
/// the owners admit one. Its charge is its native body, 40 + 40 per row.
fn output_facts(transport: &ShellSessionTransport, outputs: usize) -> Admitted {
    let grant = transport.state.content_grant.unwrap();
    let facts = ContentOutputFacts {
        grant,
        facts_generation: 1,
        outputs: (0..outputs)
            .map(|index| ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: index as u64 + 1,
                    generation: 1,
                },
                local_width: 100,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            })
            .collect(),
    };
    transport
        .state
        .admit_record(
            OutboundRecord::Content(
                TransactionId::from_raw(90),
                ShellContentRecord::OutputFacts(facts),
            ),
            super::Class::Bulk,
        )
        .unwrap()
}

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
        transport.state.connection_epoch = grant.connection_epoch;
        transport.state.capabilities = SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
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
fn final_collection_refuses_a_live_file_wire_even_without_a_content_epoch() {
    let mut fixture = Fixture::new(7);
    let transport = &mut fixture.transport;
    let owner = Box::new([41u8; 8]);
    let address = owner.as_ptr();
    let owner = transport
        .finish_content_after_backend_drop(owner)
        .unwrap_err();
    assert_eq!(owner.as_ptr(), address);
    transport.disconnect().unwrap();
    transport.state.connection_epoch = 2;
    let _peer = Peer::attach(&mut transport.state, None);
    let owner = transport
        .finish_content_after_backend_drop(owner)
        .unwrap_err();
    assert_eq!(owner.as_ptr(), address);
    assert!(transport.state.wire.is_some());
    transport.disconnect().unwrap();
    let report = transport.finish_content_after_backend_drop(owner).unwrap();
    assert_eq!(report.settled_candidates, 0);
    assert!(report.accounting.quiescent());
}

#[test]
fn all_store_credits_and_fifo_frames_share_one_capacity() {
    let mut fixture = Fixture::new(6);
    let transport = &mut fixture.transport;
    let mut peer = attach_files(transport);
    let grant = transport.state.content_grant.unwrap();
    let store = transport
        .content_epochs
        .resources_mut(transport.state.store_grant)
        .unwrap();
    isb::accept(
        store,
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
        &[0; 4],
        0,
    );
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
        6
    );
    let owned = transport.content_accounting();
    assert_eq!(owned.response_records, 6);
    assert_eq!(owned.epochs.resources, 1);
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
        .max_control_records = 5;
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
        6
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
        .max_control_records = 6;
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
        5
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
    let record = transport.state.output.front().unwrap().record.clone();
    let count = Peer::fill_journal(&mut transport.state, &record);
    assert!(!Peer::drain(&mut transport.state));
    assert_eq!(transport.content_accounting(), transferred);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    peer.expect(&mut transport.state, &record);
    assert!(Peer::drain(&mut transport.state));
    for _ in 0..count {
        peer.expect(&mut transport.state, &record);
    }
    peer.quiet(&mut transport.state);
    assert_eq!(transport.content_accounting().response_records, 5);
    assert_eq!(
        transport.content_accounting().response_bytes,
        transferred.response_bytes - CONTROL_RECORD_BYTES
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
    // Real typed bulk records of 680 + 320 body bytes leave exactly 24.
    let first = output_facts(transport, 16);
    let second = output_facts(transport, 7);
    assert_eq!(first.charge + second.charge, 1000);
    transport.state.transfer_record(first);
    transport.state.transfer_record(second);
    assert!(
        transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 24)
    );
    assert!(
        !transport
            .state
            .bulk_capacity_available(&transport.content_epochs, 25)
    );
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 4)
    );
}

#[test]
fn cancellation_reserve_survives_bulk_and_journal_pressure_until_exact_transfer() {
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
    let mut peer = attach_files(transport);
    let facts = output_facts(transport, 0);
    assert!(
        transport
            .state
            .bulk_capacity_available(&transport.content_epochs, facts.charge)
    );
    transport.state.transfer_record(facts);
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
    let first = transport.state.output.front().unwrap().record.clone();
    Peer::fill_journal(&mut transport.state, &first);
    assert!(!Peer::drain(&mut transport.state));
    assert_eq!(transport.state.output.records(), 3);
    assert!(
        !transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    peer.expect(&mut transport.state, &first);
    // Only the credited action fits the one freed journal slot; bulk cannot
    // use its terminal reserve, so the other obligations stay in the outbox.
    assert!(!Peer::drain(&mut transport.state));
    assert_eq!(transport.state.output.records(), 2);
    assert!(
        transport
            .state
            .control_capacity_available(&transport.content_epochs, 1)
    );
    // Peer loss settles delivery as lost, never as sent, and leaves no charge
    // available to be freed a second time by the next grant.
    transport.disconnect().unwrap();
    assert_eq!(transport.state.fifo_records(), 0);
    assert_eq!(transport.state.fifo_bytes(), 0);
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
    let facts = output_facts(transport, 0);
    transport.state.transfer_record(facts);
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
    // A wire takes whole custody of the bulk front; the rejection follows it.
    transport.state.output.pop_front();
    let Some(OutboundRecord::Content(_, record)) =
        transport.state.output.front().map(|queued| &queued.record)
    else {
        panic!("the rejection is queued as a typed content record");
    };
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
    while let Some(queued) = transport.state.output.front() {
        let OutboundRecord::Content(transaction, record) = &queued.record else {
            panic!("only content records were queued");
        };
        records.push((*transaction, record.clone()));
        transport.state.output.pop_front();
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
fn full_file_inbox_refuses_custody_then_retries_the_same_id_without_loss() {
    let mut fixture = Fixture::new(64);
    let t = &mut fixture.transport;
    let mut peer = attach_files(t);
    let grant = t.state.content_grant.unwrap();
    let demand = |id| {
        ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant,
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            demand_id: id,
            reason: 1,
        })
    };
    let submit = |peer: &mut Peer, t: &mut ShellSessionTransport, id| {
        peer.try_submit(&mut t.state, ShellFileKind::FrameDemand, |header| {
            encode_shell_file_transaction(
                header,
                &ShellFileTransactionRecord {
                    transaction: TransactionId::from_raw(id),
                    record: demand(id),
                },
            )
            .unwrap()
        })
    };
    // Pin the export's INBOUND_RECORDS bound without exposing it for tests.
    let bound = 64;
    for id in 1..=bound {
        assert!(submit(&mut peer, t, id));
    }
    assert!(!submit(&mut peer, t, bound + 1));
    peer.quiet(&mut t.state);
    let take = |t: &mut ShellSessionTransport| {
        t.state
            .files_mut()
            .unwrap()
            .export_mut()
            .take_content(|r| matches!(r, ShellContentRecord::FrameDemand(_)))
    };
    assert_eq!(take(t), Some((TransactionId::from_raw(1), demand(1))));
    assert!(submit(&mut peer, t, bound + 1));
    for id in 2..=bound + 1 {
        assert_eq!(take(t), Some((TransactionId::from_raw(id), demand(id))));
    }
    assert!(take(t).is_none());
    assert!(t.state.inbound_idle());
    peer.quiet(&mut t.state);
}

/// W2 prototype: a queued Presented owns its retire intent across a resource
/// flush and a real prepare failure; its transfer commits it once, so
/// ResourceReleased can only follow Presented in the FIFO.
#[test]
fn a_queued_presented_commits_its_retire_only_at_transfer_and_precedes_released() {
    let mut fixture = Fixture::new(16);
    let transport = &mut fixture.transport;
    let (grant, output, predecessor) = stage_retire_intent(transport);
    let _bundle = transport.begin_content_submission(output, 1, 2).unwrap();
    transport
        .content_prepared(grant, output, 1, 1, 1, 2)
        .unwrap();
    // Presented is queued in the store but not transferred, as when a flush
    // has not run or stopped before it.
    transport
        .content_epochs
        .candidates_mut(grant)
        .unwrap()
        .presented(output, 1, 1, 1, 1)
        .unwrap();
    let flush_resources = |transport: &mut ShellSessionTransport| {
        transport.content_epochs.collect();
        transport
            .state
            .flush_content_resource_events(&mut transport.content_epochs)
            .unwrap();
    };
    flush_resources(transport);
    let reserved = |transport: &ShellSessionTransport| {
        transport
            .content_epochs
            .resources(transport.state.store_grant)
            .unwrap()
            .reserved_retiring()
    };
    assert_eq!(reserved(transport), 8, "a resource flush cannot commit it");
    // A real transfer failure: with no record capacity, prepare_content_record
    // refuses Presented, which stays queued and keeps its intent.
    let limit = transport
        .state
        .content_limits
        .as_ref()
        .unwrap()
        .max_control_records;
    transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    assert_eq!(
        transport
            .state
            .flush_content_candidate_events(&mut transport.content_epochs),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert!(
        transport
            .content_epochs
            .active_candidates(transport.state.store_grant)
            .unwrap()
            .pending_event()
            .is_some_and(|event| event.retire.is_some())
    );
    assert_eq!(reserved(transport), 8, "a failed prepare keeps it reserved");
    flush_resources(transport);
    assert_eq!(reserved(transport), 8);
    transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = limit;
    // The retry transfers Presented and commits; repeated flushes add nothing.
    for _ in 0..3 {
        transport
            .state
            .flush_content_candidate_events(&mut transport.content_epochs)
            .unwrap();
        flush_resources(transport);
    }
    assert_eq!(reserved(transport), 0);
    let mut order = Vec::new();
    while transport.state.output.front().is_some() {
        if let OutboundRecord::Content(transaction, record) =
            transport.state.output.pop_front().record
        {
            match record {
                ShellContentRecord::CandidateOutcome(value) if value.kind == 2 => {
                    order.push(("presented", transaction))
                }
                ShellContentRecord::ResourceReleased(value) => {
                    assert_eq!(value.resource, predecessor);
                    order.push(("released", transaction))
                }
                _ => {}
            }
        }
    }
    assert_eq!(
        order,
        [
            ("presented", TransactionId::from_raw(2)),
            ("released", TransactionId::from_raw(50)),
        ]
    );
}

/// W2 prototype: resource 1 accepted under tx 9, and candidate 1 on output 1
/// ended with an intent to retire it under tx 50.
fn stage_retire_intent(
    transport: &mut ShellSessionTransport,
) -> (ContentGrant, ContentOutputId, ContentResourceId) {
    let (grant, output, predecessor) = stage_predecessor(transport);
    let (resources, candidates) = transport
        .content_epochs
        .active_pair_mut(transport.state.store_grant)
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
        .end_with_retire(
            TransactionId::from_raw(3),
            ContentCandidateEnd {
                grant,
                candidate_generation: 1,
                surface_count: 0,
                placement_count: 0,
                target_count: 0,
            },
            crate::ContentRetireOnPresent {
                retire_transaction: TransactionId::from_raw(50),
                resource: predecessor,
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
    (grant, output, predecessor)
}

/// W2 prototype: an explicit retire reusing an intent's held ID is refused,
/// and that refusal needs its own credit. With the control budget full the
/// retire stays queued on the wire; it is consumed only once the credit fits.
#[test]
fn a_full_budget_leaves_a_held_id_retire_unconsumed_until_its_refusal_fits() {
    let mut fixture = Fixture::new(16);
    let transport = &mut fixture.transport;
    let mut peer = attach_files(transport);
    let (grant, _, _) = stage_retire_intent(transport);
    let other = ContentResourceId {
        id: 2,
        generation: 1,
    };
    isb::accept(
        transport
            .content_epochs
            .resources_mut(transport.state.store_grant)
            .unwrap(),
        TransactionId::from_raw(10),
        ContentResourceBegin {
            grant,
            resource: other,
            width_px: 2,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 8,
        },
        &[0; 8],
        1,
    );
    // Drop the accept statuses and the queued output, so the budget holds
    // only what stays reserved; the service would otherwise send them first.
    let resources = transport
        .content_epochs
        .resources_mut(transport.state.store_grant)
        .unwrap();
    while resources.take_event().is_some() {}
    while transport.state.output.front().is_some() {
        transport.state.output.pop_front();
    }
    assert!(peer.try_submit(
        &mut transport.state,
        ShellFileKind::ResourceRetire,
        |header| {
            encode_shell_file_resource_retire(
                header,
                &ShellFileTransactionRecord {
                    transaction: TransactionId::from_raw(50),
                    record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                        grant,
                        resource: other,
                    }),
                },
            )
            .unwrap()
        }
    ));
    peer.quiet(&mut transport.state);
    let mut full = None;
    let set_limit = |transport: &mut ShellSessionTransport, records| {
        transport
            .state
            .content_limits
            .as_mut()
            .unwrap()
            .max_control_records = records;
    };
    let retiring = |transport: &ShellSessionTransport| {
        transport
            .content_epochs
            .resources(transport.state.store_grant)
            .unwrap()
            .usage()
            .retiring
    };
    // Raise the record limit one at a time. Below the first limit that
    // consumes the retire, the budget had room for a credit-free request
    // but not for this refusal, and the retire stayed on the wire.
    // Bounded by the fixture's normal limit, so a regression fails here.
    let mut consumed = None;
    for limit in 0..=16 {
        set_limit(transport, limit);
        let processed = transport
            .state
            .service_content_resources(&mut transport.content_epochs, 1);
        if processed == Ok(1) {
            consumed = Some(limit);
            break;
        }
        assert_eq!(processed, Ok(0));
        assert!(!transport.state.inbound_idle(), "the retire stays queued");
        assert_eq!(retiring(transport), 0);
        let room = |additional| {
            transport
                .state
                .control_capacity_available(&transport.content_epochs, additional)
        };
        if room(0) {
            assert!(!room(1));
            full = Some(limit);
        }
    }
    let limit = consumed.expect("no limit up to 16 consumed the retire");
    assert_eq!(full, Some(limit - 1), "the full budget held the retire");
    assert!(transport.state.inbound_idle());
    assert_eq!(retiring(transport), 0, "neither resource retires");
    // The refusal is the only response, and it reaches the peer.
    peer.expect(
        &mut transport.state,
        &OutboundRecord::Content(
            TransactionId::from_raw(50),
            ShellContentRecord::ResourceStatus(ContentResourceStatus {
                grant,
                resource: other,
                status: 3,
                reason: ContentReason::Stale as u16,
                next_ordinal: 0,
                admitted_bytes: 0,
            }),
        ),
    );
    peer.quiet(&mut transport.state);
    assert_eq!(
        transport
            .content_epochs
            .resources(transport.state.store_grant)
            .unwrap()
            .reserved_retiring(),
        8,
        "the intent keeps its reservation"
    );
}

/// W2 prototype: resource 1 accepted under tx 9, and permit 1 granted on
/// output 1.
fn stage_predecessor(
    transport: &mut ShellSessionTransport,
) -> (ContentGrant, ContentOutputId, ContentResourceId) {
    let grant = transport.state.content_grant.unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    let predecessor = ContentResourceId {
        id: 1,
        generation: 1,
    };
    isb::accept(
        transport
            .content_epochs
            .resources_mut(transport.state.store_grant)
            .unwrap(),
        TransactionId::from_raw(9),
        ContentResourceBegin {
            grant,
            resource: predecessor,
            width_px: 2,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 8,
        },
        &[0; 8],
        0,
    );
    transport
        .grant_content_permit(TransactionId::from_raw(1), output, 1, 1, 0)
        .unwrap();
    (grant, output, predecessor)
}

#[path = "shell_retire_on_present_wire.rs"]
mod retire_on_present_wire;

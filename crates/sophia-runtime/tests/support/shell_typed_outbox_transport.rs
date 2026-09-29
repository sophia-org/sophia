//! The typed outbox under the real transport: neutral per-record charges,
//! bulk saturation refused before any owner changes, whole file record
//! bounds, disconnected cleanup and neighbouring owners. The credit checks
//! are proven here by
//! intake that stays queued until its credit exists.
use super::files::Peer;
use crate::shell_transport::control_budget::{CONTROL_RECORD_BYTES, Class};
use crate::shell_transport::outbound::{OutboundRecord, output_facts_charge};
use crate::shell_transport::{ShellComponentTransport, ShellTransportError};
use crate::{ContentEpochRegistry, ContentStoreProfile};
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
const MIB: u64 = 1024 * 1024;

fn tx(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}

const OUTPUT: ContentOutputId = ContentOutputId {
    id: 1,
    generation: 1,
};

/// One component with supplied negotiated content state, sharing `epochs`.
struct Owner {
    transport: ShellComponentTransport,
    directory: std::path::PathBuf,
}

impl Owner {
    fn new(epochs: &mut ContentEpochRegistry, epoch: u64, records: u32) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-typed-outbox-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::getuid().as_raw(),
        )
        .unwrap();
        let grant = grant(epoch);
        let mut limits = ContentLimits::prototype(grant);
        limits.max_control_records = records;
        // Two owners must fit the one 64 MiB registry.
        limits.max_staging_bytes = 4 * MIB;
        limits.max_resident_bytes = 4 * MIB;
        limits.max_retiring_bytes = 4 * MIB;
        epochs
            .admit_with_profile(limits.clone(), ContentStoreProfile::Legacy)
            .unwrap();
        transport.store_grant = grant;
        transport.content_grant = Some(grant);
        transport.content_limits = Some(limits);
        transport.connection_epoch = epoch;
        transport.capabilities = SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        Self {
            transport,
            directory,
        }
    }

    fn attach_files(&mut self) -> Peer {
        Peer::attach(&mut self.transport, Some(ContentStoreProfile::Legacy))
    }
    fn inbox(&mut self) -> usize {
        usize::from(
            !self
                .transport
                .files_mut()
                .unwrap()
                .export()
                .inbound_is_empty(),
        )
    }
    fn begin_resource(&mut self, peer: &mut Peer) {
        let grant = self.transport.store_grant;
        peer.submit(
            &mut self.transport,
            ShellFileKind::ResourceBegin,
            |header| {
                encode_shell_file_resource_begin(
                    header,
                    &ShellFileResourceBegin {
                        transaction: tx(3),
                        slot: 0,
                        record: ShellContentRecord::ResourceBegin(ContentResourceBegin {
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
                        }),
                    },
                )
                .unwrap()
            },
        );
    }
    fn facts(&self, outputs: usize) -> OutboundRecord {
        OutboundRecord::Content(
            tx(90),
            ShellContentRecord::OutputFacts(ContentOutputFacts {
                grant: self.transport.store_grant,
                facts_generation: 1,
                outputs: (0..outputs).map(|index| entry(index as u64 + 1)).collect(),
            }),
        )
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn entry(id: u64) -> ContentOutputFactsEntry {
    ContentOutputFactsEntry {
        output: ContentOutputId { id, generation: 1 },
        local_width: 100,
        local_height: 100,
        scale_numerator: 1,
        scale_denominator: 1,
        scale_generation: 1,
    }
}

fn registry() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * MIB).unwrap()
}

fn native_input(text: &str, grant: ContentGrant) -> OutboundRecord {
    let binding = NativeLauncherBinding {
        grant,
        output: OUTPUT,
        opening: 1,
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        catalog_generation: 1,
        candidate_generation: 1,
        presentation_epoch: 1,
        interaction_generation: 1,
        state_revision: 1,
        focus_lease: 1,
    };
    OutboundRecord::NativeLauncher(
        tx(40),
        ShellNativeLauncherRecord::Input(NativeLauncherInput {
            event: NativeLauncherEvent {
                binding,
                event_id: 1,
                state_revision: 2,
            },
            issued_mono_usec: 1,
            kind: NativeLauncherInputKind::Text,
            text: text.to_owned(),
        }),
    )
}

fn rejected_allocation(grant: ContentGrant) -> OutboundRecord {
    OutboundRecord::Content(
        tx(41),
        ShellContentRecord::AllocationResult(ContentAllocationResult {
            grant,
            allocation_request_id: 1,
            status: 2,
            reason: ContentReason::Budget as u16,
            output: OUTPUT,
            allocation: ContentAllocationId::default(),
            parent: ContentAllocationId::default(),
            scale_generation: 0,
            logical: ContentLogicalRect::default(),
            pixel: ContentPixelRect::default(),
            scale_numerator: 0,
            scale_denominator: 0,
            allowed_reservation_extent: 0,
            margins: ContentMargins::default(),
            acknowledged_anchor: ContentPixelRect::default(),
        }),
    )
}

fn control(limit: usize) -> Class {
    Class::Control {
        limit,
        oversize: ShellTransportError::ContentQueueSaturated,
    }
}

#[test]
fn the_registry_and_the_fifo_charge_output_facts_the_same_neutral_body_bytes() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    for outputs in 0..=16 {
        let admitted = owner
            .transport
            .admit_record(owner.facts(outputs), Class::Bulk)
            .unwrap();
        assert_eq!(admitted.charge, output_facts_charge(outputs));
        assert_eq!(admitted.charge, 40 + 40 * outputs);
    }
    // The registry's queued charge moves into the FIFO unchanged.
    let store_grant = owner.transport.store_grant;
    epochs
        .allocations_mut(store_grant)
        .unwrap()
        .publish_outputs(tx(1), 1, vec![entry(1), entry(2)])
        .unwrap();
    assert_eq!(
        epochs.bulk_occupancy(store_grant),
        (1, output_facts_charge(2))
    );
    owner
        .transport
        .flush_content_allocation_events(&mut epochs)
        .unwrap();
    assert_eq!(epochs.bulk_occupancy(store_grant), (0, 0));
    assert_eq!(owner.transport.output.bulk_bytes(), output_facts_charge(2));
    assert_eq!(owner.transport.output.records(), 1);
}

#[test]
fn typed_bulk_saturation_is_refused_before_the_facts_owner_changes() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let filler = owner
        .transport
        .admit_record(owner.facts(16), Class::Bulk)
        .unwrap();
    let limits = owner.transport.content_limits.as_mut().unwrap();
    limits.reserved_control_queue_bytes = 1024;
    // Exactly one 16-row record of bulk fits.
    limits.max_output_queue_bytes = 1024 + filler.charge as u32;
    owner.transport.transfer_record(filler);
    let store_grant = owner.transport.store_grant;
    let generation = epochs.allocations(store_grant).unwrap().facts_generation();
    assert_eq!(
        owner
            .transport
            .publish_content_output_facts(&mut epochs, tx(2), 9, vec![entry(1)]),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    let allocations = epochs.allocations(store_grant).unwrap();
    assert_eq!(allocations.facts_generation(), generation);
    assert!(allocations.pending_event().is_none());
    assert_eq!(owner.transport.output.records(), 1);
    // The same record admits once the bulk it would exceed has left.
    owner.transport.output.pop_front();
    owner
        .transport
        .publish_content_output_facts(&mut epochs, tx(2), 9, vec![entry(1)])
        .unwrap();
    assert_eq!(owner.transport.output.bulk_bytes(), output_facts_charge(1));
}

#[test]
fn every_control_record_fits_its_credit_as_a_whole_file_record() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let _peer = owner.attach_files();
    let grant = owner.transport.store_grant;
    let text = "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES);
    for (record, limit, charge) in [
        (rejected_allocation(grant), CONTROL_RECORD_BYTES, 168),
        (native_input(&text, grant), 512, 398),
    ] {
        let (_, body) = record.native().unwrap();
        assert_eq!(body.len(), charge);
        let admitted = owner
            .transport
            .admit_record(record.clone(), control(limit))
            .unwrap();
        assert_eq!(admitted.charge, charge);
        let whole = SHELL_FILE_HEADER_BYTES + charge;
        owner
            .transport
            .admit_record(record.clone(), control(whole))
            .unwrap();
        assert_eq!(
            owner
                .transport
                .admit_record(record, control(whole - 1))
                .unwrap_err(),
            ShellTransportError::ContentQueueSaturated
        );
    }
    assert!(owner.transport.output.front().is_none());
}

#[test]
fn native_input_is_bounded_by_its_text_and_whole_file_record() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let _peer = owner.attach_files();
    let grant = owner.transport.store_grant;
    let longest = "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES);
    let over = "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES + 1);
    assert_eq!(
        native_input(&over, grant).native().unwrap_err(),
        ShellTransportError::WrongContentRecord
    );
    owner
        .transport
        .admit_record(native_input(&longest, grant), control(430))
        .unwrap();
    assert_eq!(
        owner
            .transport
            .admit_record(native_input(&longest, grant), control(429))
            .unwrap_err(),
        ShellTransportError::ContentQueueSaturated
    );
    assert!(owner.transport.output.front().is_none());
}

#[test]
fn disconnect_releases_typed_records_file_input_and_snapshots() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let mut peer = owner.attach_files();
    owner.begin_resource(&mut peer);
    assert_eq!(owner.inbox(), 1);
    let facts = owner
        .transport
        .admit_record(owner.facts(16), Class::Bulk)
        .unwrap();
    owner.transport.transfer_record(facts);
    assert!(Peer::drain(&mut owner.transport));
    assert!(
        owner
            .transport
            .content_accounting(&epochs)
            .snapshot_retained_bytes
            > 0
    );
    // The snapshot and its announcement are still unread, and a submitted
    // Begin still belongs to the export. Further typed records stay queued.
    for outputs in [3, 4] {
        let admitted = owner
            .transport
            .admit_record(owner.facts(outputs), Class::Bulk)
            .unwrap();
        owner.transport.transfer_record(admitted);
    }
    assert_eq!(owner.transport.fifo_records(), 2);
    assert!(owner.transport.fifo_bytes() > 0);
    owner.transport.disconnect(&mut epochs).unwrap();
    assert!(owner.transport.wire.is_none());
    assert_eq!(owner.transport.fifo_records(), 0);
    assert_eq!(owner.transport.fifo_bytes(), 0);
    assert_eq!(owner.transport.output.charged(), 0);
    let accounting = owner.transport.content_accounting(&epochs);
    assert_eq!(accounting.response_records, 0);
    assert_eq!(accounting.response_bytes, 0);
    assert_eq!(accounting.input_records, 0);
    assert_eq!(accounting.input_bytes, 0);
    assert_eq!(accounting.snapshot_retained_bytes, 0);
    assert_eq!(accounting.snapshot_reserved_bytes, 0);
    let mut successor = Owner::new(&mut epochs, 2, 64);
    let _successor_peer = successor.attach_files();
    assert!(successor.transport.inbound_idle());
    assert!(successor.transport.fifo_is_empty());
}

#[test]
fn a_saturated_or_disconnected_neighbour_does_not_spend_another_owners_budget() {
    let mut epochs = registry();
    let mut first = Owner::new(&mut epochs, 1, 4);
    let mut second = Owner::new(&mut epochs, 2, 4);
    let _first_peer = first.attach_files();
    let _second_peer = second.attach_files();
    while first
        .transport
        .bulk_capacity_available(&epochs, output_facts_charge(0))
    {
        let admitted = first
            .transport
            .admit_record(first.facts(0), Class::Bulk)
            .unwrap();
        first.transport.transfer_record(admitted);
    }
    assert!(!first.transport.control_capacity_available(&epochs, 1));
    assert!(second.transport.control_capacity_available(&epochs, 3));
    assert!(
        second
            .transport
            .bulk_capacity_available(&epochs, output_facts_charge(16))
    );
    let admitted = second
        .transport
        .admit_record(second.facts(2), Class::Bulk)
        .unwrap();
    second.transport.transfer_record(admitted);
    first.transport.disconnect(&mut epochs).unwrap();
    assert_eq!(second.transport.output.records(), 1);
    assert_eq!(second.transport.output.bulk_bytes(), output_facts_charge(2));
    assert!(epochs.resources(second.transport.store_grant).is_some());
    assert_eq!(
        second
            .transport
            .content_accounting(&epochs)
            .response_records,
        1
    );
}

#[test]
fn a_resource_request_stays_queued_until_its_response_credits_exist() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 2);
    let mut peer = owner.attach_files();
    owner.begin_resource(&mut peer);
    // Begin owes up to three responses; two record credits cannot hold them.
    assert_eq!(
        owner
            .transport
            .service_content_resources(&mut epochs, 0)
            .unwrap(),
        0
    );
    assert_eq!(owner.inbox(), 1);
    assert!(owner.transport.output.front().is_none());
    let store_grant = owner.transport.store_grant;
    assert_eq!(
        epochs.resources(store_grant).unwrap().control_occupancy(),
        0
    );
    owner
        .transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 64;
    assert_eq!(
        owner
            .transport
            .service_content_resources(&mut epochs, 0)
            .unwrap(),
        1
    );
    assert_eq!(owner.inbox(), 0);
}

#[test]
fn a_permit_and_an_allocation_answer_need_their_credits_before_the_owner_changes() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 2);
    let store_grant = owner.transport.store_grant;
    // A permit reserves three credits: permit, prepared and presented.
    assert_eq!(
        owner
            .transport
            .grant_content_permit(&mut epochs, tx(1), OUTPUT, 1, 1, 0),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(
        epochs
            .active_candidates(store_grant)
            .unwrap()
            .control_occupancy(),
        0
    );
    assert!(owner.transport.output.front().is_none());
    // An allocation answer is already credited; a full FIFO still holds it.
    let allocations = epochs.allocations_mut(store_grant).unwrap();
    allocations
        .publish_outputs(tx(4), 1, vec![entry(1)])
        .unwrap();
    allocations.take_event().unwrap();
    allocations
        .request(
            tx(5),
            ContentAllocationRequest {
                grant: store_grant,
                output: OUTPUT,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: 100,
                desired_height: 20,
                margins: ContentMargins::default(),
            },
            &[],
            0,
        )
        .unwrap();
    // The answer's credit plus two queued records exceed the two allowed.
    for _ in 0..2 {
        let admitted = owner
            .transport
            .admit_record(owner.facts(0), Class::Bulk)
            .unwrap();
        owner.transport.transfer_record(admitted);
    }
    assert!(!owner.transport.control_capacity_available(&epochs, 0));
    assert_eq!(
        owner.transport.reject_content_allocation(
            &mut epochs,
            1,
            crate::ContentAllocationError::Budget
        ),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert!(
        epochs
            .allocations(store_grant)
            .unwrap()
            .pending_request()
            .is_some()
    );
    assert_eq!(owner.transport.output.records(), 2);
}

#[test]
fn the_file_journal_takes_typed_records_in_admission_order() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let mut peer = owner.attach_files();
    let records = [90, 7, 91].map(|id| {
        let OutboundRecord::Content(_, value) = rejected_allocation(owner.transport.store_grant)
        else {
            unreachable!()
        };
        OutboundRecord::Content(tx(id), value)
    });
    for record in &records {
        let admitted = owner
            .transport
            .admit_record(record.clone(), Class::Bulk)
            .unwrap();
        owner.transport.transfer_record(admitted);
    }
    assert!(Peer::drain(&mut owner.transport));
    assert!(owner.transport.fifo_is_empty());
    for record in &records {
        peer.expect(&mut owner.transport, record);
    }
    peer.quiet(&mut owner.transport);
}

#[test]
fn file_publication_charges_the_snapshot_once_after_outbox_transfer() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 2);
    let mut peer = owner.attach_files();
    let facts = owner.facts(3);
    let bytes = facts.native().unwrap().1.len() + SHELL_FILE_HEADER_BYTES;
    let admitted = owner
        .transport
        .admit_record(facts.clone(), Class::Bulk)
        .unwrap();
    owner.transport.transfer_record(admitted);
    assert_eq!(
        owner.transport.content_accounting(&epochs).response_records,
        1
    );
    assert_eq!(
        owner
            .transport
            .content_accounting(&epochs)
            .snapshot_retained_bytes,
        0
    );
    assert!(Peer::drain(&mut owner.transport));
    let accounting = owner.transport.content_accounting(&epochs);
    assert_eq!(accounting.response_records, 0);
    assert_eq!(accounting.response_bytes, 0);
    assert_eq!(accounting.snapshot_retained_bytes, bytes);
    peer.expect(&mut owner.transport, &facts);
    // Reading/clunking releases the pin, but the current snapshot remains.
    assert_eq!(
        owner
            .transport
            .content_accounting(&epochs)
            .snapshot_retained_bytes,
        bytes
    );
}

#[test]
fn disconnect_releases_a_published_file_snapshot() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 2);
    let _peer = owner.attach_files();
    let facts = owner
        .transport
        .admit_record(owner.facts(3), Class::Bulk)
        .unwrap();
    owner.transport.transfer_record(facts);
    assert!(Peer::drain(&mut owner.transport));
    assert!(
        owner
            .transport
            .content_accounting(&epochs)
            .snapshot_retained_bytes
            > 0
    );
    owner.transport.disconnect(&mut epochs).unwrap();
    let accounting = owner.transport.content_accounting(&epochs);
    assert_eq!(accounting.response_records, 0);
    assert_eq!(accounting.response_bytes, 0);
    assert_eq!(accounting.snapshot_retained_bytes, 0);
    assert_eq!(accounting.snapshot_reserved_bytes, 0);
}

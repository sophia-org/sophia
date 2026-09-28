//! The typed outbox under the real transport: neutral per-record charges,
//! bulk saturation refused before any owner changes, each wire's own record
//! bounds, disconnected cleanup and neighbouring owners. The credit checks
//! that replaced the per-owner socket-byte pre-checks are proven here by
//! intake that stays queued until its credit exists.
use crate::shell_transport::control_budget::{CONTROL_RECORD_BYTES, Class};
use crate::shell_transport::outbound::{OutboundRecord, output_facts_charge};
use crate::shell_transport::socket::SocketWire;
use crate::shell_transport::wire::Wire;
use crate::shell_transport::{ShellComponentTransport, ShellTransportError};
use crate::{ContentEpochRegistry, ContentStoreProfile};
use sophia_protocol::*;
use std::io::Write as _;
use std::os::unix::net::UnixStream;
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
        Self {
            transport,
            directory,
        }
    }

    /// Attaches a real socket wire; the returned peer never reads unless asked.
    fn attach_socket(&mut self) -> UnixStream {
        let (local, peer) = UnixStream::pair().unwrap();
        local.set_nonblocking(true).unwrap();
        let limits = self.transport.content_limits.clone();
        self.transport.wire = Some(Wire::Socket(Box::new(SocketWire::new(
            local,
            limits.as_ref(),
        ))));
        peer
    }

    /// Delivers one client frame through the socket's production read path.
    fn deliver(&mut self, peer: &mut UnixStream, frame: &[u8]) {
        peer.write_all(frame).unwrap();
        let socket = self.transport.socket_mut().unwrap();
        let before = socket.input_accounting().0;
        while socket.input_accounting().0 == before {
            socket.receive(4096).unwrap();
        }
    }

    fn inbox(&self) -> usize {
        self.transport.socket().unwrap().input_accounting().0
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
fn every_control_record_fits_its_credit_as_a_whole_record_on_both_wires() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let grant = owner.transport.store_grant;
    let text = "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES);
    for socket in [false, true] {
        let _peer = socket.then(|| owner.attach_socket());
        // The largest base-content response and the largest native record.
        let allocation = owner
            .transport
            .admit_record(rejected_allocation(grant), control(CONTROL_RECORD_BYTES))
            .unwrap();
        assert_eq!(allocation.charge, 168);
        let input = owner
            .transport
            .admit_record(native_input(&text, grant), control(512))
            .unwrap();
        assert_eq!(input.charge, 398);
        for (record, limit) in [
            (rejected_allocation(grant), CONTROL_RECORD_BYTES),
            (native_input(&text, grant), 512),
        ] {
            let frame = SocketWire::encode(&record).unwrap().len();
            let (_, body) = record.native().unwrap();
            assert!(frame <= limit && 32 + body.len() <= limit);
        }
        // A credit smaller than the whole record (412 framed, 430 as a file
        // record) refuses before custody.
        assert_eq!(
            owner
                .transport
                .admit_record(native_input(&text, grant), control(411))
                .unwrap_err(),
            ShellTransportError::ContentQueueSaturated
        );
        owner.transport.wire = None;
    }
}

#[test]
fn native_input_is_bounded_by_its_text_and_by_each_wires_record_bound() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let grant = owner.transport.store_grant;
    let longest = "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES);
    let over = "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES + 1);
    // The record's own text bound, on every wire.
    assert_eq!(
        native_input(&over, grant).native().unwrap_err(),
        ShellTransportError::WrongContentRecord
    );
    // The file record has a fixed text slot: the longest text admits.
    owner
        .transport
        .admit_record(native_input(&longest, grant), control(512))
        .unwrap();
    // A socket peer advertised a smaller frame payload; it stays enforced
    // there, before any custody, and nowhere else.
    let limits = owner.transport.content_limits.as_mut().unwrap();
    limits.max_chunk_bytes = 256;
    limits.max_frame_payload = 304;
    let _peer = owner.attach_socket();
    owner
        .transport
        .admit_record(native_input(&"a".repeat(172), grant), control(512))
        .unwrap();
    assert_eq!(
        owner
            .transport
            .admit_record(native_input(&"a".repeat(173), grant), control(512))
            .unwrap_err(),
        ShellTransportError::WrongContentRecord
    );
    assert!(owner.transport.output.front().is_none());
}

#[test]
fn disconnect_releases_typed_records_lane_frames_publication_and_a_partial_write() {
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let _peer = owner.attach_socket();
    for outputs in [16, 3] {
        let admitted = owner
            .transport
            .admit_record(owner.facts(outputs), Class::Bulk)
            .unwrap();
        owner.transport.transfer_record(admitted);
    }
    let lane = encode_shell_content_frame(
        tx(7),
        &ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: owner.transport.store_grant,
            facts_generation: 1,
            outputs: vec![entry(1)],
        }),
    )
    .unwrap();
    owner.transport.enqueue_async(&epochs, lane).unwrap();
    owner
        .transport
        .publish_indicators(
            &epochs,
            tx(8),
            &ShellIndicatorSnapshot {
                connection_epoch: 1,
                generation: 1,
                active_output: None,
                statuses: Vec::new(),
                indicators: Vec::new(),
            },
        )
        .unwrap();
    // One byte of the front record is in the kernel; its charge stays.
    let Some(Wire::Socket(socket)) = owner.transport.wire.as_mut() else {
        unreachable!("attached");
    };
    socket.send(&mut owner.transport.output, 1).unwrap();
    assert!(owner.transport.fifo_records() >= 4);
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
    // A later epoch's socket starts with no half-written predecessor.
    let _peer = owner.attach_socket();
    assert!(owner.transport.inbound_idle());
    assert!(owner.transport.fifo_is_empty());
}

#[test]
fn a_saturated_or_disconnected_neighbour_does_not_spend_another_owners_budget() {
    let mut epochs = registry();
    let mut first = Owner::new(&mut epochs, 1, 4);
    let mut second = Owner::new(&mut epochs, 2, 4);
    let _first_peer = first.attach_socket();
    let _second_peer = second.attach_socket();
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
    let mut peer = owner.attach_socket();
    let begin = encode_shell_content_frame(
        tx(3),
        &ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant: owner.transport.store_grant,
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
    )
    .unwrap();
    owner.deliver(&mut peer, &begin);
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
fn the_socket_writes_typed_records_and_its_lane_frames_in_admission_order() {
    use std::io::Read as _;
    let mut epochs = registry();
    let mut owner = Owner::new(&mut epochs, 1, 64);
    let mut peer = owner.attach_socket();
    let first = owner
        .transport
        .admit_record(owner.facts(1), Class::Bulk)
        .unwrap();
    owner.transport.transfer_record(first);
    let lane = encode_shell_content_frame(
        tx(7),
        &ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: owner.transport.store_grant,
            facts_generation: 1,
            outputs: vec![entry(1), entry(2)],
        }),
    )
    .unwrap();
    owner.transport.enqueue_async(&epochs, lane).unwrap();
    let last = owner
        .transport
        .admit_record(owner.facts(3), Class::Bulk)
        .unwrap();
    owner.transport.transfer_record(last);
    owner.transport.poll_io(&mut epochs).unwrap();
    assert!(owner.transport.fifo_is_empty());
    peer.set_nonblocking(true).unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    while let Ok(count) = peer.read(&mut chunk) {
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let mut order = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let length = SOPHIA_IPC_HEADER_LEN
            + u32::from_le_bytes(bytes[at + 16..at + 20].try_into().unwrap()) as usize;
        let (transaction, record) = decode_shell_content_frame(&bytes[at..at + length]).unwrap();
        let ShellContentRecord::OutputFacts(facts) = record else {
            panic!("only output facts were queued");
        };
        order.push((transaction.raw(), facts.outputs.len()));
        at += length;
    }
    assert_eq!(order, [(90, 1), (7, 2), (90, 3)]);
}

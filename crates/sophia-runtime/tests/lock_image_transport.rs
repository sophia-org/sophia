//! t338 lock sealed-image path, slice 1: revision 2 selection, disclosure
//! and the refuse-only `ImageBegin`. No socket is opened here. Readiness in
//! these tests is supplied by the test acting as owner; it proves nothing
//! about an authenticated Hello.
use std::time::Instant;

use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;

const EPOCH: u64 = 7;
const GENERATION: u64 = 5;
const EBUSY: Errno = Errno(16);

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 4,
        upload_slots: 2,
        max_chords: 2,
        max_width_px: 64,
        max_height_px: 64,
        max_resource_bytes: 64 * 64 * 4,
        max_live_resources: 3,
        journal_records: 16,
        journal_bytes: 4096,
        assembly_timeout_ms: 1000,
        ack_progress_timeout_ms: 2000,
    }
}

fn lock() -> LockObject {
    LockObject {
        lock_epoch: 4,
        topology_generation: 3,
        phase: LockPhase::Locked,
        allocations: vec![LockAllocation {
            output_id: 1,
            output_generation: 1,
            allocation_id: 11,
            allocation_generation: 1,
            pixel_width: 4,
            pixel_height: 2,
            scale_numerator: 1,
            scale_denominator: 1,
        }],
    }
}

fn image_settings() -> LockImageSettings {
    LockImageSettings {
        discovery: LockImageDiscovery {
            connection_epoch: EPOCH,
            channel_generation: GENERATION,
            endpoint: "image.sock".into(),
            source_kinds: 1,
            max_offers: 4,
            max_packets_per_turn: 8,
            hello_timeout_ms: 1000,
            validation_timeout_ms: 100,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 200,
            formats_generation: 2,
            limits_generation: 3,
        },
        limits: LockImageLimits {
            channel_generation: GENERATION,
            limits_generation: 3,
            max_resource_bytes: 64 * 64 * 4,
            max_sealed_backing_bytes: 2 * 64 * 64 * 4,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 4,
            max_validation_jobs: 2,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 1,
            max_width: 64,
            max_height: 64,
            validation_timeout_ms: 100,
            capture_interest_timeout_ms: 200,
            validation_chunk_bytes: 65536,
            max_packets_per_turn: 8,
        },
        formats: LockImageFormats {
            channel_generation: GENERATION,
            formats_generation: 2,
            limits_generation: 3,
            rows: Vec::new(),
        },
    }
}

fn custody() -> LockFileCustody {
    LockFileCustody::new(LockFileSettings {
        epoch: EPOCH,
        limits: limits(),
        reserved_chords: Vec::new(),
        lock: lock(),
        lock_qid: 90,
    })
    .unwrap()
}

fn metadata(offer: u64) -> LockImageMetadata {
    LockImageMetadata {
        offer_id: offer,
        resource: 20 + offer,
        resource_generation: 1,
        admission_epoch: 4,
        output_id: 1,
        allocation_id: 11,
        topology_generation: 3,
        source_kind: LOCK_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: LOCK_IMAGE_SEALED_FORMAT,
        width: 4,
        height: 2,
        stride: 16,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 32,
        modifier: 0,
    }
}

struct Provider {
    custody: LockFileCustody,
    revision: u16,
    submission: u64,
    read: u64,
    journal: Vec<u8>,
    now: Instant,
}

impl Provider {
    fn new(configured: bool) -> Self {
        let mut custody = custody();
        if configured {
            custody.configure_image(image_settings()).unwrap();
        }
        Self {
            custody,
            revision: 1,
            submission: 0,
            read: 0,
            journal: Vec::new(),
            now: Instant::now(),
        }
    }

    fn record(&self, kind: LockFileKind, submission: u64, body: &[u8]) -> Vec<u8> {
        encode_lock_file_record_at_revision(
            LockFileHeader {
                kind,
                connection_epoch: EPOCH,
                submission_id: submission,
                sequence: 0,
            },
            body,
            self.revision,
        )
        .unwrap()
    }

    /// Submits under the next ID; a refused submission keeps its ID for a
    /// retry, as the SDKs do.
    fn submit(&mut self, kind: LockFileKind, body: &[u8]) -> Result<(), Errno> {
        let bytes = self.record(kind, self.submission + 1, body);
        let result = self.custody.submit(&bytes, self.now);
        if result.is_ok() {
            self.submission += 1;
        }
        result
    }

    fn negotiate(&mut self, minimum: u16, maximum: u16, capabilities: u64) -> (u16, u64) {
        let body = LockNegotiate {
            minimum_revision: minimum,
            maximum_revision: maximum,
            requested_capabilities: capabilities,
            chords: Vec::new(),
        }
        .encode()
        .unwrap();
        self.submit(LockFileKind::Negotiate, &body).unwrap();
        let events = self.events();
        let (_, body) = events
            .iter()
            .find(|(kind, _)| *kind == LockFileKind::Negotiated)
            .expect("negotiated");
        let (selected, negotiated) = LockNegotiated::decode_selected(body).unwrap();
        assert_eq!(self.custody.revision(), selected);
        self.revision = selected;
        (selected, negotiated.granted_capabilities)
    }

    fn image_begin(&mut self, offer: u64) -> Result<(), Errno> {
        self.submit(LockFileKind::ImageBegin, &metadata(offer).encode().unwrap())
    }

    /// Every event journaled since the last call, acknowledged as read.
    fn events(&mut self) -> Vec<(LockFileKind, Vec<u8>)> {
        let tail = self.custody.position().tail;
        if tail == self.read {
            return Vec::new();
        }
        let ReadOutcome::Ready(bytes) = self.custody.read(self.read, 1 << 16).unwrap() else {
            panic!("journal read not ready");
        };
        self.read += bytes.len() as u64;
        self.journal.extend_from_slice(&bytes);
        let mut events = Vec::new();
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
            let record =
                decode_lock_file_record_at_revision(&rest[..size], LockFileClass::Event, 2)
                    .unwrap();
            events.push((record.header.kind, record.body.to_vec()));
            rest = &rest[size..];
        }
        let sequence = self.custody.position().next_sequence - 1;
        self.custody
            .acknowledge(LockFileAck {
                connection_epoch: EPOCH,
                sequence,
            })
            .unwrap();
        events
    }
}

const PRESENT: u64 = LOCK_FILE_CAPABILITY_PRESENT;
const SEALED: u64 = LOCK_FILE_CAPABILITY_SEALED_MEMORY;
const DMA: u64 = LOCK_FILE_CAPABILITY_DMA_BUF;

#[test]
fn revision_one_offers_are_byte_identical_with_or_without_configuration() {
    let mut plain = Provider::new(false);
    let mut configured = Provider::new(true);
    assert_eq!(plain.negotiate(1, 1, PRESENT), (1, PRESENT));
    assert_eq!(configured.negotiate(1, 1, PRESENT | SEALED), (1, PRESENT));
    assert_eq!(plain.journal, configured.journal);
    assert_eq!(
        configured.custody.image_readiness(),
        LockImageReadiness::Disabled
    );
    assert!(configured.custody.image_settings().is_none());
    // Image kinds do not exist at selection 1.
    let bytes = encode_lock_file_record_at_revision(
        LockFileHeader {
            kind: LockFileKind::ImageBegin,
            connection_epoch: EPOCH,
            submission_id: 2,
            sequence: 0,
        },
        &metadata(1).encode().unwrap(),
        2,
    )
    .unwrap();
    assert_eq!(
        configured.custody.submit(&bytes, configured.now),
        Err(Errno::EINVAL)
    );
}

#[test]
fn an_unconfigured_epoch_selects_one_from_a_range_spanning_two() {
    let mut provider = Provider::new(false);
    assert_eq!(
        provider.negotiate(1, 2, PRESENT | SEALED | DMA),
        (1, PRESENT)
    );
    assert_eq!(
        provider.custody.image_readiness(),
        LockImageReadiness::Disabled
    );
    // Two alone is refused as before.
    let mut provider = Provider::new(false);
    let body = LockNegotiate {
        minimum_revision: 2,
        maximum_revision: 2,
        requested_capabilities: PRESENT,
        chords: Vec::new(),
    }
    .encode()
    .unwrap();
    provider.submit(LockFileKind::Negotiate, &body).unwrap();
    assert!(
        provider
            .events()
            .iter()
            .any(|(kind, _)| *kind == LockFileKind::Refused)
    );
}

#[test]
fn a_configured_epoch_selects_two_and_grants_sealed_only_on_request() {
    let mut provider = Provider::new(true);
    assert_eq!(
        provider.negotiate(1, 2, PRESENT | SEALED | DMA),
        (2, PRESENT | SEALED)
    );
    assert!(provider.custody.image_disclosed());
    assert_eq!(
        provider.custody.image_readiness(),
        LockImageReadiness::AwaitingHello
    );
    assert_eq!(provider.custody.image_settings(), Some(&image_settings()));
    // Selected 2 without a sealed request: nothing is disclosed.
    let mut provider = Provider::new(true);
    assert_eq!(provider.negotiate(2, 2, PRESENT), (2, PRESENT));
    assert!(!provider.custody.image_disclosed());
    assert_eq!(
        provider.custody.image_readiness(),
        LockImageReadiness::Disabled
    );
    assert!(provider.custody.image_settings().is_none());
    assert_eq!(provider.image_begin(1), Err(Errno::EINVAL));
}

#[test]
fn image_begin_is_busy_until_the_owner_reports_ready_then_refused_cleanly() {
    let mut provider = Provider::new(true);
    provider.negotiate(1, 2, PRESENT | SEALED);
    let before = provider.custody.position();
    // Awaiting Hello: no receipt, no status, nothing journalled.
    assert_eq!(provider.image_begin(1), Err(EBUSY));
    assert_eq!(provider.custody.position(), before);
    // The test, acting as owner, supplies readiness.
    assert_eq!(
        provider.custody.image_channel_ready(GENERATION + 1, 1),
        Err(Errno::ESTALE)
    );
    assert_eq!(
        provider.custody.image_channel_ready(GENERATION, 3),
        Err(Errno::EINVAL)
    );
    provider.custody.image_channel_ready(GENERATION, 1).unwrap();
    assert_eq!(
        provider.custody.image_channel_ready(GENERATION, 1),
        Err(Errno::EACCES)
    );
    let events = provider.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, LockFileKind::ChannelReady);
    assert_eq!(
        LockChannelReady::decode(&events[0].1).unwrap(),
        LockChannelReady {
            channel_generation: GENERATION,
            granted_source_kinds: 1,
        }
    );
    // The same submission retried now gets its receipt and reason 9.
    provider.image_begin(1).unwrap();
    let events = provider.events();
    assert_eq!(
        events.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        [LockFileKind::Submitted, LockFileKind::ResourceStatus]
    );
    let receipt = LockSubmitted::decode_at_revision(&events[0].1, 2).unwrap();
    assert_eq!(receipt.submission_id, 2);
    assert_eq!(receipt.candidate_kind, LockFileKind::ImageBegin);
    assert_eq!(
        LockResourceStatus::decode(&events[1].1).unwrap(),
        LockResourceStatus {
            transaction: 1,
            resource: LockResourceId {
                id: 21,
                generation: 1,
            },
            status: LockResourceState::Rejected,
            reason: LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE,
            admitted_bytes: 0,
        }
    );
    // A refused offer's ID, or a lower one, cannot return.
    assert_eq!(provider.image_begin(1), Err(Errno::ESTALE));
    provider.image_begin(3).unwrap();
    assert_eq!(provider.image_begin(2), Err(Errno::ESTALE));
    provider.events();
    // Closing spends the held credit once.
    provider
        .custody
        .image_channel_closed(GENERATION, LockChannelClosedReason::PeerDisconnect)
        .unwrap();
    assert_eq!(
        provider
            .custody
            .image_channel_closed(GENERATION, LockChannelClosedReason::PeerDisconnect),
        Err(Errno::EACCES)
    );
    let events = provider.events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        LockChannelClosed::decode(&events[0].1).unwrap().reason,
        LockChannelClosedReason::PeerDisconnect
    );
    assert_eq!(
        provider.custody.image_readiness(),
        LockImageReadiness::Closed {
            reason: LockChannelClosedReason::PeerDisconnect,
        }
    );
    // Closed: still a clean refusal.
    provider.image_begin(4).unwrap();
    assert_eq!(provider.events()[1].0, LockFileKind::ResourceStatus);
}

#[test]
fn a_failed_hello_closes_at_once_and_journals_when_room_allows() {
    let mut provider = Provider::new(true);
    provider.negotiate(1, 2, PRESENT | SEALED);
    assert_eq!(
        provider
            .custody
            .image_channel_hello_failed(GENERATION + 1, LockChannelClosedReason::HelloTimeout),
        Err(Errno::ESTALE)
    );
    provider
        .custody
        .image_channel_hello_failed(GENERATION, LockChannelClosedReason::HelloTimeout)
        .unwrap();
    assert_eq!(
        provider.custody.image_channel_ready(GENERATION, 1),
        Err(Errno::EACCES)
    );
    let events = provider.events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        LockChannelClosed::decode(&events[0].1).unwrap(),
        LockChannelClosed {
            channel_generation: GENERATION,
            reason: LockChannelClosedReason::HelloTimeout,
        }
    );
    assert!(!provider.custody.retry_image_publications().unwrap());
    provider.image_begin(1).unwrap();
    assert_eq!(provider.events()[1].0, LockFileKind::ResourceStatus);
}

#[test]
fn a_failed_hello_waits_for_journal_room() {
    let mut provider = Provider::new(true);
    provider.negotiate(1, 2, PRESENT | SEALED);
    // Fill the journal without acknowledging.
    let mut entries = 0;
    while provider
        .custody
        .entry(LockEntry {
            lock_epoch: 4,
            entry: LockEntryKind::Insert,
            empty_after: false,
        })
        .is_ok()
    {
        entries += 1;
    }
    assert!(entries > 0);
    provider
        .custody
        .image_channel_hello_failed(GENERATION, LockChannelClosedReason::ProtocolFault)
        .unwrap();
    assert!(matches!(
        provider.custody.image_readiness(),
        LockImageReadiness::Closed { .. }
    ));
    // Acknowledging the backlog lets the pending ChannelClosed through.
    let events = provider.events();
    assert!(events.iter().all(|(kind, _)| *kind == LockFileKind::Entry));
    let events = provider.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, LockFileKind::ChannelClosed);
    assert!(!provider.custody.retry_image_publications().unwrap());
}

#[test]
fn configuration_is_coherent_sealed_only_and_before_negotiation() {
    let mut bad = Vec::new();
    let mut settings = image_settings();
    settings.discovery.connection_epoch = EPOCH + 1;
    bad.push(settings);
    let mut settings = image_settings();
    settings.discovery.source_kinds = 3;
    bad.push(settings);
    let mut settings = image_settings();
    settings.limits.limits_generation = 4;
    bad.push(settings);
    let mut settings = image_settings();
    settings.formats.formats_generation = 9;
    bad.push(settings);
    let mut settings = image_settings();
    settings.discovery.endpoint = "other.sock".into();
    bad.push(settings);
    let mut settings = image_settings();
    settings.limits.max_offers = 3;
    bad.push(settings);
    let mut settings = image_settings();
    settings.limits.max_width = 65;
    bad.push(settings);
    for settings in bad {
        assert_eq!(custody().configure_image(settings), Err(Errno::EINVAL));
    }
    // Too small a journal for the terminal credits.
    let mut small = limits();
    small.journal_records = 9;
    let mut custody = LockFileCustody::new(LockFileSettings {
        epoch: EPOCH,
        limits: small,
        reserved_chords: Vec::new(),
        lock: lock(),
        lock_qid: 90,
    })
    .unwrap();
    assert_eq!(
        custody.configure_image(image_settings()),
        Err(Errno::EINVAL)
    );
    // Once only, and never after negotiation.
    let mut provider = Provider::new(true);
    assert_eq!(
        provider.custody.configure_image(image_settings()),
        Err(Errno::EINVAL)
    );
    let mut provider = Provider::new(false);
    provider.negotiate(1, 2, PRESENT);
    assert_eq!(
        provider.custody.configure_image(image_settings()),
        Err(Errno::EINVAL)
    );
}

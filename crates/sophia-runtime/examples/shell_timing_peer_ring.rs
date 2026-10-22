//! Raw owned-ring panel peer for the D4 performance arm (schema 5).
//!
//! This measurement peer uses the authenticated launch attach and then speaks raw 9P on that one
//! connection: the owned-ring codec and shell-file records for
//! limits, allocation, demand, candidate and retire. Fresh pacing by
//! default; an optional `interval_msec` switches to the external bursty
//! pattern on an absolute schedule (idle is frames=1 plus the hold).
//! No sealed `UploadOffer` is emitted for a ring frame.
//!
//! Caveats for readers of the journals: one frame is in flight at a
//! time (no N-slot overlap), warmup covers first-touch slot pages, and
//! the ring arm's admitted-to-ready interval includes ProducerRing
//! creation.
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::owned_ring_protocol::{CONTROL_FILE, EventKind, Identity, Offer, Registration};
use std::{io::Read, os::unix::net::UnixStream, time::Duration};

#[path = "shell_timing_peer_ring/journal.rs"]
mod journal;
use journal::{Event as JournalEvent, Upload};
#[path = "shell_timing_peer_ring/raw.rs"]
mod raw;
use raw::{Client, Codec, Inbound};
#[cfg(test)]
#[path = "shell_timing_peer_ring/smoke.rs"]
mod smoke;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const WAIT: Duration = Duration::from_secs(5);

fn clock_usec() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    u64::try_from(t.tv_sec).unwrap() * 1_000_000 + u64::try_from(t.tv_nsec).unwrap() / 1000
}

fn emit(schema: u8, event: JournalEvent<'_>) -> Result<()> {
    journal::write(&mut std::io::stdout().lock(), schema, event)?;
    Ok(())
}

fn header(epoch: u64, kind: ShellFileKind, submission_id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id,
        sequence: 0,
    }
}

/// Encode one candidate record with the submission id its envelope carries.
fn submit_record(
    client: &mut Client,
    next_submission: &mut dyn FnMut() -> u64,
    build: impl Fn(u64) -> Result<Vec<u8>>,
) -> Result<()> {
    let submission = next_submission();
    let bytes = build(submission)?;
    client.submit(&bytes, submission)
}

fn resource_for_frame(frame: u64) -> ContentResourceId {
    ContentResourceId {
        id: (frame - 1) % 2 + 1,
        generation: (frame - 1) / 2 + 1,
    }
}

/// Per-slot header size in the ring geometry (the runtime's
/// `OwnedRingGeometry` HEADER: one 4096-byte header before each slot's
/// pixels, with the generation word at its start). No protocol accessor
/// exists for it, so the geometry is asserted against it at setup.
const SLOT_HEADER_BYTES: usize = 4096;

/// The shared canvas routine, identical for every source arm: shade from
/// the frame generation, opaque alpha in every four-byte pixel.
fn fill_pixels(bytes: &mut [u8], generation: u64) {
    let shade = (generation % 128) as u8;
    let (pixels, remainder) = bytes.as_chunks_mut::<4>();
    pixels.fill([shade, shade, shade, 255]);
    remainder.fill(shade);
}

/// The external-bar bursty pattern (tools/external-bar/common/fill.c
/// `fill_bursty_panel`): rewrite the whole buffer with a fixed
/// generation-1 background; only the rightmost 96 columns change with
/// the frame generation.
fn fill_bursty_panel(bytes: &mut [u8], width: u32, generation: u64) {
    fill_pixels(bytes, 1);
    let width = width as usize;
    let rect = 96.min(width);
    for row in bytes.chunks_mut(width * 4) {
        fill_pixels(&mut row[(width - rect) * 4..], generation);
    }
}

struct Config {
    output_id: u64,
    frames: u64,
    height: u64,
    hold: u64,
    slots: usize,
    /// Absolute bursty interval; absent is the historical fresh pacing.
    interval_msec: Option<u64>,
    /// Retained tile pool size; absent is a per-frame upload workload.
    retained: Option<usize>,
}

impl Config {
    fn schema(&self) -> u8 {
        5
    }
}

fn parse_config() -> Result<Config> {
    let config_path = std::env::var_os("SOPHIA_SHELL_CONFIG").ok_or("missing configuration")?;
    let mut text = String::new();
    std::fs::File::open(config_path)?
        .take(4097)
        .read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err("configuration too large".into());
    }
    let config: serde_json::Value = serde_json::from_str(&text)?;
    let object = config
        .as_object()
        .ok_or("configuration must be an object")?;
    let allowed = [
        "wire",
        "output",
        "frames",
        "height",
        "hold_msec",
        "source",
        "ring_slots",
        "probe_bytes",
        "workload",
        "interval_msec",
        "retained_resources",
    ];
    if object.keys().any(|k| !allowed.contains(&k.as_str()))
        || !["wire", "output", "frames", "height", "hold_msec", "source"]
            .iter()
            .all(|k| object.contains_key(*k))
    {
        return Err(
            "configuration requires wire, output, frames, height, hold_msec and source".into(),
        );
    }
    let number = |key: &str| -> Result<u64> {
        config[key]
            .as_u64()
            .ok_or_else(|| format!("invalid {key}").into())
    };
    let output_id = number("output")?;
    let frames = number("frames")?;
    let height = number("height")?;
    let hold = number("hold_msec")?;
    if config["source"].as_str() != Some("owned_ring") {
        return Err("source must be owned_ring; sealed shell uploads are retired".into());
    }
    let slots = number("ring_slots")?;
    if !(1..=4).contains(&slots) {
        return Err("ring_slots outside 1..=4".into());
    }
    let interval_msec = match object.get("interval_msec") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .filter(|v| (1..=10_000).contains(v))
                .ok_or("interval_msec outside 1..=10000")?,
        ),
    };
    let retained = match object.get("workload").and_then(|v| v.as_str()) {
        None | Some("fresh") => {
            if object.contains_key("retained_resources") {
                return Err("retained_resources requires the retained workload".into());
            }
            None
        }
        Some("retained") => {
            if interval_msec.is_some() {
                return Err("retained workload is vsync-paced, not interval-paced".into());
            }
            Some(
                object
                    .get("retained_resources")
                    .and_then(|v| v.as_u64())
                    .filter(|v| (2..=8).contains(v))
                    .ok_or("retained workload requires retained_resources in 2..=8")?
                    as usize,
            )
        }
        Some(_) => return Err("workload must be fresh or retained".into()),
    };
    if config["wire"].as_str() != Some("files")
        || object
            .get("probe_bytes")
            .is_some_and(|v| v.as_u64() != Some(0))
        || output_id == 0
        || !(1..=10000).contains(&frames)
        || !(1..=512).contains(&height)
        || hold > 600000
    {
        return Err("configuration outside bounded panel workload".into());
    }
    Ok(Config {
        output_id,
        frames,
        height,
        hold,
        slots: slots as usize,
        interval_msec,
        retained,
    })
}

fn run() -> Result<()> {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--serve"] {
        return Err("expected --serve and explicit SOPHIA_SHELL_CONFIG".into());
    }
    let config = parse_config()?;
    let socket = std::env::var_os("SOPHIA_SHELL_9P_SOCKET").ok_or("missing file endpoint")?;
    let auth_fd: i32 = std::env::var_os("SOPHIA_LAUNCH_AUTH_FD")
        .ok_or("ring source requires an authenticated file launch")?
        .to_str()
        .ok_or("non-UTF8 launch descriptor")?
        .parse()?;
    if auth_fd < 3 {
        return Err("invalid launch descriptor".into());
    }
    // This peer never spawns another process. Both the inherited channel
    // and its safe CLOEXEC duplicate live until process exit.
    let relay = UnixStream::from(sophia_linux_peer::duplicate_inherited_fd(auth_fd)?);
    let mut auth = sophia_launch_client::Client::with_timeout(
        UnixStream::connect(&socket)?,
        relay,
        b"shell",
        b"bar",
        WAIT,
    )?;
    while !auth.ready() {
        auth.poll()?;
        if !auth.ready() {
            auth.wait_for_io(Duration::from_millis(10))?;
        }
    }
    let attached = auth.into_authenticated()?;
    let _relay = attached.relay;
    let mut client = Client::new(attached.role, attached.root);
    client.init()?;
    serve(&mut client, &config, &socket)
}

/// The whole peer after attach: one connection, exact schema-5 journal.
fn serve(client: &mut Client, config: &Config, socket: &std::ffi::OsStr) -> Result<()> {
    let epoch = client.epoch();
    let mut submissions = 0u64;
    let mut next_submission = move || {
        submissions += 1;
        submissions
    };
    // Negotiate the production owned-ring capability explicitly.
    submit_record(client, &mut next_submission, |submission| {
        encode_shell_file_negotiate(
            header(epoch, ShellFileKind::Negotiate, submission),
            ShellV1ClientHello {
                minimum_revision: 10,
                maximum_revision: 10,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                    | 1 << 15,
            },
        )
        .codec()
    })?;
    let welcome = match client.take()? {
        Inbound::Negotiated(welcome) => welcome,
        other => return Err(format!("expected negotiated, got {other:?}").into()),
    };
    if welcome.selected_revision != 10 || welcome.connection_epoch != epoch {
        return Err("wrong negotiated revision or epoch".into());
    }
    // Limits is a connect-time object; output facts follow their
    // Outputs announcement. Neither is a journal content record here.
    let limits = client.fetch_limits()?;
    let facts = loop {
        match client.take()? {
            Inbound::Facts(facts) => break facts,
            Inbound::Published(ShellFileKind::Outputs) => {
                break client.fetch_facts()?;
            }
            Inbound::Published(_) => {}
            other => return Err(format!("expected output facts, got {other:?}").into()),
        }
    };
    let output = facts
        .outputs
        .iter()
        .find(|entry| entry.output.id == config.output_id)
        .ok_or("selected output absent")?;
    let grant = limits.grant;
    if grant.connection_epoch != epoch {
        return Err("content grant from another connection epoch".into());
    }
    if facts.grant != grant || facts.facts_generation == 0 || output.output.generation == 0 {
        return Err("stale or misattributed output facts".into());
    }
    if limits.max_resource_ids < config.retained.map_or(2, |n| n as u32) {
        return Err("insufficient resource identities for paced workload".into());
    }
    // Grant the panel allocation before any ring registration: ring pixel
    // geometry must equal the granted allocation geometry exactly.
    submit_record(client, &mut next_submission, |submission| {
        encode_shell_file_allocation_request(
            header(epoch, ShellFileKind::AllocationRequest, submission),
            ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(1),
                record: ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant,
                    output: output.output,
                    allocation_request_id: 1,
                    operation: 1,
                    role: 1,
                    edge: 1,
                    prior: ContentAllocationId::default(),
                    parent: ContentAllocationId::default(),
                    parent_presentation_epoch: 0,
                    anchor_parent_rect: ContentPixelRect::default(),
                    desired_width: output.local_width,
                    desired_height: config.height as u32,
                    margins: ContentMargins::default(),
                }),
            },
        )
        .codec()
    })?;
    let allocation = match client.take()? {
        Inbound::Allocation(allocation) => allocation,
        other => return Err(format!("expected allocation result, got {other:?}").into()),
    };
    if allocation.status != 1
        || allocation.grant != grant
        || allocation.output != output.output
        || allocation.allocation_request_id != 1
        || allocation.pixel.width != output.local_width
        || allocation.pixel.height != config.height as u32
    {
        return Err("allocation refused, misattributed or geometry differs".into());
    }
    let schema = config.schema();
    emit(
        schema,
        JournalEvent::Start {
            frames: config.frames,
            wire: "files",
            probe_bytes: 0,
            source: Some("owned_ring"),
            workload: Some(if config.retained.is_some() {
                "retained"
            } else {
                "fresh"
            }),
            ring_slots: Some(config.slots as u64),
            interval_msec: config.interval_msec,
            retained_resources: config.retained.map(|n| n as u64),
        },
    )?;
    // Image channel: discovery, limits precheck, Hello, ChannelReady.
    let discovery_fid = client.open(b"image-channel", 0)?;
    let discovery_text = client.read(discovery_fid, 0, 65500)?;
    let discovery = ShellImageDiscovery::decode(std::str::from_utf8(&discovery_text)?).codec()?;
    let limits_fid = client.open(b"image-limits", 0)?;
    let limits_text = client.read(limits_fid, 0, 65500)?;
    let limits_record =
        decode_shell_file_record_at_revision(&limits_text, ShellFileClass::Object, 10).codec()?;
    let image_limits = ShellImageLimits::decode(limits_record.body).codec()?;
    let width = allocation.pixel.width;
    let height = allocation.pixel.height;
    let pixels = u64::from(width) * u64::from(height) * 4;
    if pixels > image_limits.max_resource_bytes
        || width > image_limits.max_width
        || height > image_limits.max_height
    {
        return Err("panel geometry exceeds image limits".into());
    }
    let identity = Identity {
        grant,
        channel: discovery.channel_generation,
        ring: 1,
        generation: 1,
    };
    let registration = Registration {
        identity,
        slots: config.slots,
        pixels: pixels as usize,
    };
    let geometry = registration.geometry()?;
    if geometry.slot_extent()
        != SLOT_HEADER_BYTES + (pixels as usize).next_multiple_of(SLOT_HEADER_BYTES)
    {
        return Err("ring geometry does not match the slot header layout".into());
    }
    if geometry.source_bytes() as u64 + pixels > image_limits.max_sealed_backing_bytes {
        return Err("ring geometry exceeds image limits".into());
    }
    let image_path = std::path::Path::new(socket)
        .parent()
        .ok_or("file endpoint has no directory")?
        .join(&discovery.endpoint);
    let image_socket = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::SEQPACKET,
        rustix::net::SocketFlags::CLOEXEC,
        None,
    )?;
    rustix::net::connect(
        &image_socket,
        &rustix::net::SocketAddrUnix::new(&image_path)?,
    )?;
    let hello = ShellImagePacket::Hello {
        connection_epoch: grant.connection_epoch,
        channel_generation: discovery.channel_generation,
    }
    .encode()
    .codec()?;
    rustix::net::send(&image_socket, &hello, rustix::net::SendFlags::NOSIGNAL)?;
    match client.take()? {
        Inbound::ChannelReady(4) => {}
        other => return Err(format!("expected channel ready, got {other:?}").into()),
    }
    // D4C1 registration, then D4F1 with the size-sealed ring descriptor. The
    // ring is mapped once; per-frame fills write the mapping directly.
    let control = client.open(CONTROL_FILE, 1)?;
    let register = registration.encode();
    client.write(control, &register)?;
    let admitted_usec = match client.take()? {
        Inbound::Owned(event) if event.kind == EventKind::RingAdmitted => {
            if event.identity != identity {
                return Err("ring admitted for another identity".into());
            }
            clock_usec()
        }
        other => return Err(format!("expected ring admitted, got {other:?}").into()),
    };
    let producer = sophia_linux_peer::ProducerRing::create(geometry.source_bytes())?;
    let mut ring_source = RingSource {
        control,
        producer,
        geometry,
        identity,
        slots: config.slots,
    };
    {
        let packet = registration.source_packet();
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control_msg = rustix::net::SendAncillaryBuffer::new(&mut space);
        let rights = [ring_source.producer.source_fd()];
        if !control_msg.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)) {
            return Err("ring descriptor did not fit ancillary buffer".into());
        }
        rustix::net::sendmsg(
            &image_socket,
            &[std::io::IoSlice::new(&packet)],
            &mut control_msg,
            rustix::net::SendFlags::NOSIGNAL,
        )?;
    }
    let ready_usec = match client.take()? {
        Inbound::Owned(event) if event.kind == EventKind::RingReady => {
            if event.identity != identity {
                return Err("ring ready for another identity".into());
            }
            clock_usec()
        }
        other => return Err(format!("expected ring ready, got {other:?}").into()),
    };
    emit(
        schema,
        JournalEvent::RingReady {
            connection_epoch: grant.connection_epoch,
            content_grant_epoch: grant.content_grant_epoch,
            channel: identity.channel,
            ring: identity.ring,
            ring_generation: identity.generation,
            slots: config.slots as u64,
            pixel_bytes: pixels,
            admitted_usec,
            ready_usec,
        },
    )?;
    run_frames(
        client,
        &mut next_submission,
        config,
        grant,
        &facts,
        output,
        &allocation,
        &mut ring_source,
        width,
        height,
    )?;
    emit(
        schema,
        JournalEvent::End {
            offered: config.frames,
            completed: config.frames,
        },
    )?;
    std::thread::sleep(Duration::from_millis(config.hold));
    Ok(())
}

/// One frame's source-independent context.
struct FrameCtx<'a> {
    schema: u8,
    grant: ContentGrant,
    facts: &'a ContentOutputFacts,
    output: &'a ContentOutputFactsEntry,
    allocation: &'a ContentAllocationResult,
    frame: u64,
    resource: ContentResourceId,
    width: u32,
    height: u32,
    /// Absolute scheduled due time for bursty frames; absent when fresh.
    due_usec: Option<u64>,
    /// Retained pool tile fill generation; absent for frame uploads.
    tile: Option<u64>,
    /// Journal candidate generation: the frame number, or 0 for pool uploads.
    candidate_generation: u64,
}

/// Direct mapped fill and acceptance settlement, separate from visual sequencing.
struct RingSource {
    control: sophia_9p_client::records::Fid,
    producer: sophia_linux_peer::ProducerRing,
    geometry: sophia_runtime::OwnedRingGeometry,
    identity: Identity,
    slots: usize,
}

impl RingSource {
    fn upload(&mut self, client: &mut Client, ctx: &FrameCtx<'_>) -> Result<()> {
        let frame = ctx.frame;
        let resource = ctx.resource;
        let slot = ((frame - 1) % self.slots as u64) as usize;
        let slot_generation = 2 * frame;
        let pixels = ctx.width as usize * ctx.height as usize * 4;
        // Fill the mapped slot directly: generation word at the slot
        // start, the shared canvas routine over the pixels after the
        // fixed per-slot header.
        let fill_start_usec = clock_usec();
        let slot_offset = slot * self.geometry.slot_extent();
        let slot_bytes =
            &mut self.producer.bytes_mut()[slot_offset..slot_offset + self.geometry.slot_extent()];
        slot_bytes[..8].copy_from_slice(&slot_generation.to_le_bytes());
        let slot_pixels = &mut slot_bytes[SLOT_HEADER_BYTES..SLOT_HEADER_BYTES + pixels];
        if let Some(generation) = ctx.tile {
            fill_pixels(slot_pixels, generation);
        } else if ctx.due_usec.is_some() {
            fill_bursty_panel(slot_pixels, ctx.width, frame);
        } else {
            fill_pixels(slot_pixels, frame);
        }
        let offer = Offer {
            identity: self.identity,
            request: sophia_runtime::OwnedCopyImageRequest {
                slot: sophia_runtime::OwnedSlotIdentity {
                    offer: frame,
                    slot,
                    generation: slot_generation,
                },
                resource,
                allocation: ctx.allocation.allocation,
                output: ctx.output.output,
                topology_generation: ctx.facts.facts_generation,
                width: ctx.width,
                height: ctx.height,
            },
        };
        // Journal first, with the offer time sampled before the write:
        // the record precedes submission, as in the existing peer.
        let begin_usec = clock_usec();
        emit(
            ctx.schema,
            JournalEvent::RingOffer {
                upload: Upload {
                    connection_epoch: ctx.grant.connection_epoch,
                    content_grant_epoch: ctx.grant.content_grant_epoch,
                    output: ctx.output.output.id,
                    candidate_generation: ctx.candidate_generation,
                    resource_id: resource.id,
                    resource_generation: resource.generation,
                    width: ctx.width,
                    height: ctx.height,
                    total_bytes: pixels as u64,
                    begin_usec,
                },
                offer: frame,
                slot: slot as u64,
                slot_generation,
                fill_start_usec,
                due_usec: ctx.due_usec,
            },
        )?;
        client.write(self.control, &offer.encode())?;
        // Settlement pacing matches the historical sealed baseline: status 2, then the
        // exact slot release, and only then the frame demand.
        match client.take()? {
            Inbound::Status(tx, status)
                if tx.raw() == frame
                    && status.grant == ctx.grant
                    && status.resource == resource
                    && status.status == 2 => {}
            other => return Err(format!("expected ring status 2, got {other:?}").into()),
        }
        emit(
            ctx.schema,
            JournalEvent::RingAccepted {
                offer: frame,
                resource_id: resource.id,
                resource_generation: resource.generation,
                accepted_usec: clock_usec(),
            },
        )?;
        match client.take()? {
            Inbound::Owned(event)
                if event.kind == EventKind::SlotReleased
                    && event.identity == self.identity
                    && event.slot.offer == frame
                    && event.slot.slot == slot
                    && event.slot.generation == slot_generation
                    && event.resource == resource
                    && event.reason == 0
                    && !event.changed_generation => {}
            other => return Err(format!("expected exact slot release, got {other:?}").into()),
        }
        emit(
            ctx.schema,
            JournalEvent::SlotReleased {
                offer: frame,
                slot: slot as u64,
                slot_generation,
                resource_id: resource.id,
                resource_generation: resource.generation,
                released_usec: clock_usec(),
            },
        )
    }
}

fn run_frames(
    client: &mut Client,
    next_submission: &mut dyn FnMut() -> u64,
    config: &Config,
    grant: ContentGrant,
    facts: &ContentOutputFacts,
    output: &ContentOutputFactsEntry,
    allocation: &ContentAllocationResult,
    source: &mut RingSource,
    width: u32,
    height: u32,
) -> Result<()> {
    let epoch = client.epoch();
    let schema = config.schema();
    // Retained pool: upload each tile once through the ring before the
    // first candidate; the resources then stay live for every frame.
    let tile_width = config.retained.map(|n| (width / n as u32) & !1);
    if let (Some(n), Some(tile_width)) = (config.retained, tile_width) {
        if tile_width == 0 {
            return Err("retained tile width rounds to zero".into());
        }
        for index in 1..=n as u64 {
            let ctx = FrameCtx {
                schema,
                grant,
                facts,
                output,
                allocation,
                frame: index,
                resource: ContentResourceId {
                    id: index,
                    generation: 1,
                },
                width: tile_width,
                height,
                due_usec: None,
                tile: Some(index),
                candidate_generation: 0,
            };
            source.upload(client, &ctx)?;
        }
    }
    // Absolute bursty schedule: frame 1 is due at the anchor, each
    // later frame one interval after the previous due time, regardless
    // of how long earlier frames took to settle.
    let anchor_usec = clock_usec();
    for frame in 1..=config.frames {
        let due_usec = config
            .interval_msec
            .map(|interval| anchor_usec + (frame - 1) * interval * 1000);
        if let Some(due) = due_usec {
            let now = clock_usec();
            if now < due {
                std::thread::sleep(Duration::from_micros(due - now));
            }
        }
        let resource = resource_for_frame(frame);
        let ctx = FrameCtx {
            schema,
            grant,
            facts,
            output,
            allocation,
            frame,
            resource,
            width,
            height,
            due_usec,
            tile: None,
            candidate_generation: frame,
        };
        if config.retained.is_none() {
            source.upload(client, &ctx)?;
        }
        let demand_usec = clock_usec();
        submit_record(client, next_submission, |submission| {
            encode_shell_file_transaction(
                header(epoch, ShellFileKind::FrameDemand, submission),
                &ShellFileTransactionRecord {
                    transaction: TransactionId::from_raw(5),
                    record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                        grant,
                        output: output.output,
                        allocation: ContentAllocationId::default(),
                        demand_id: frame,
                        reason: 1,
                    }),
                },
            )
            .codec()
        })?;
        let permit = match client.take()? {
            Inbound::Permit(permit) => permit,
            other => return Err(format!("expected frame permit, got {other:?}").into()),
        };
        if permit.state != 1
            || permit.grant != grant
            || permit.output != output.output
            || permit.demand_id != frame
            || permit.permit_id == 0
        {
            return Err("permit refused or misattributed".into());
        }
        emit(
            schema,
            JournalEvent::Permit {
                generation: frame,
                demand_usec,
                received_usec: clock_usec(),
            },
        )?;
        let placements: Vec<ContentPlacement> = match (config.retained, tile_width) {
            (Some(n), Some(tile_width)) => (0..n as u64)
                .map(|index| ContentPlacement {
                    resource: ContentResourceId {
                        id: index + 1,
                        generation: 1,
                    },
                    surface_index: 0,
                    destination_x_px: (((index + frame - 1) % n as u64) * tile_width as u64) as i32,
                    destination_y_px: 0,
                })
                .collect(),
            _ => vec![ContentPlacement {
                resource,
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
        };
        if config.retained.is_some() {
            emit(
                schema,
                JournalEvent::CandidatePlacements {
                    candidate_generation: frame,
                    placements: placements
                        .iter()
                        .map(|p| journal::Placement {
                            resource_id: p.resource.id,
                            resource_generation: p.resource.generation,
                            destination_x_px: p.destination_x_px,
                            destination_y_px: p.destination_y_px,
                        })
                        .collect(),
                },
            )?;
        }
        let submission = next_submission();
        let candidate = encode_shell_file_candidate(
            header(epoch, ShellFileKind::Candidate, submission),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(6),
                candidate: ContentCandidate {
                    grant,
                    candidate_generation: frame,
                    output: output.output,
                    facts_generation: facts.facts_generation,
                    pacing_permit: permit.permit_id,
                    interaction_generation: 1,
                    surfaces: vec![ContentSurface {
                        allocation: allocation.allocation,
                        scale_generation: allocation.scale_generation,
                        role: 1,
                        edge: 1,
                        margins: allocation.margins,
                        reservation_extent: 0,
                        parent_surface_index: u16::MAX,
                        anchor_parent_rect: ContentPixelRect::default(),
                    }],
                    placements,
                    targets: vec![],
                },
            },
        )
        .codec()?;
        emit(
            schema,
            JournalEvent::Offer {
                connection_epoch: grant.connection_epoch,
                content_grant_epoch: grant.content_grant_epoch,
                output: output.output.id,
                candidate_generation: frame,
                submitted_monotonic_usec: clock_usec(),
            },
        )?;
        client.submit(&candidate, submission)?;
        let mut prepared = false;
        loop {
            let outcome = match client.take()? {
                Inbound::Outcome(outcome) => outcome,
                other => {
                    return Err(format!("expected candidate outcome, got {other:?}").into());
                }
            };
            if outcome.grant != grant
                || outcome.output != output.output
                || outcome.candidate_generation != frame
            {
                return Err("misattributed candidate outcome".into());
            }
            if outcome.kind == 1 {
                if prepared {
                    return Err("duplicate prepared outcome".into());
                }
                prepared = true;
                continue;
            }
            emit(
                schema,
                JournalEvent::Terminal {
                    connection_epoch: grant.connection_epoch,
                    content_grant_epoch: grant.content_grant_epoch,
                    output: output.output.id,
                    candidate_generation: frame,
                    outcome: if outcome.kind == 2 {
                        "presented"
                    } else {
                        "rejected"
                    },
                    kind: outcome.kind,
                    reason: outcome.reason,
                    observed_monotonic_usec: clock_usec(),
                },
            )?;
            if outcome.kind != 2 || !prepared || outcome.presentation_epoch == 0 {
                return Err("candidate was not presented".into());
            }
            break;
        }
        // Keep the displayed resource until a successor is shown, as fresh.
        // Retained tiles are never retired during the frame phase.
        if frame > 1 && config.retained.is_none() {
            let old = resource_for_frame(frame - 1);
            submit_record(client, next_submission, |submission| {
                encode_shell_file_resource_retire(
                    header(epoch, ShellFileKind::ResourceRetire, submission),
                    &ShellFileTransactionRecord {
                        transaction: TransactionId::from_raw(7),
                        record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                            grant,
                            resource: old,
                        }),
                    },
                )
                .codec()
            })?;
            match client.take()? {
                Inbound::Released(tx, released)
                    if tx.raw() == 7
                        && released.grant == grant
                        && released.resource == old
                        && released.reason == 0 => {}
                other => return Err(format!("expected retired release, got {other:?}").into()),
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        let _ = emit(
            5,
            JournalEvent::Failed {
                error: &error.to_string(),
            },
        );
        std::process::exit(1);
    }
}

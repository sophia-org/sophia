//! Generic paced shell peer for the isolated comparison tree.
//! Emits every offer before submission; native timing comes from host custody.
use sophia_protocol::*;
use sophia_shell_client::{
    IMAGE_CAPABILITY_SEALED, ProducerMetadata, ShellClientError, ShellClientOptions,
    ShellConnection,
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const WAIT: Duration = Duration::from_secs(5);
#[path = "shell_timing_peer/journal.rs"]
mod journal;
use journal::{Event, Upload};
#[path = "shell_timing_peer/canvas.rs"]
mod canvas;
#[path = "shell_timing_peer/sealed.rs"]
mod sealed;
use sealed::SealedConfig;

#[cfg(feature = "comparator-allocation-counting")]
#[global_allocator]
static ALLOCATOR: sophia_runtime::comparator_counters::Allocator =
    sophia_runtime::comparator_counters::ALLOCATOR;

enum Wire {
    Files {
        client: Box<ShellConnection>,
        _relay: Option<Box<sophia_9p_client::pipeline::Pipeline>>,
    },
    Ipc(UnixStream),
}

fn frame(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header)?;
    let bytes = u32::from_le_bytes(header[16..20].try_into()?) as usize;
    if bytes > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err("oversized IPC frame".into());
    }
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + bytes, 0);
    stream.read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])?;
    Ok(frame)
}

impl Wire {
    fn connect(files: bool, sealed: Option<SealedConfig>) -> Result<Self> {
        let caps = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        // Sealed sources need revision 9 and the sealed capability; the
        // byte comparison keeps its revision 6 handshake unchanged.
        let revision = if sealed.is_some() { 9 } else { 6 };
        let options = ShellClientOptions {
            minimum_revision: revision,
            maximum_revision: revision,
            required_capabilities: caps
                | if sealed.is_some() {
                    IMAGE_CAPABILITY_SEALED
                } else {
                    0
                },
            handshake_timeout: WAIT,
        };
        let auth = std::env::var_os("SOPHIA_LAUNCH_AUTH_FD");
        if sealed.is_some() && (!files || auth.is_none()) {
            return Err("sealed source requires an authenticated file launch".into());
        }
        if files {
            if std::env::var_os("SOPHIA_SHELL_SOCKET").is_some() {
                return Err("ambiguous shell wire".into());
            }
            if sealed.is_none() {
                return Err(
                    "byte file uploads are retired; use the frozen historical candidate".into(),
                );
            }
            let socket =
                std::env::var_os("SOPHIA_SHELL_9P_SOCKET").ok_or("missing file endpoint")?;
            if let Some(fd) = auth {
                let fd: i32 = fd.to_str().ok_or("non-UTF8 launch descriptor")?.parse()?;
                if fd < 3 {
                    return Err("invalid launch descriptor".into());
                }
                // This peer never spawns another process. Both the inherited
                // channel and its safe CLOEXEC duplicate live until process exit.
                let relay = UnixStream::from(sophia_linux_peer::duplicate_inherited_fd(fd)?);
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
                let client = match sealed {
                    Some(config) => {
                        let origin_ms = clock_usec() / 1000;
                        let image = sealed::image_config(
                            config,
                            std::path::Path::new(&socket),
                            origin_ms,
                            WAIT,
                        )?;
                        ShellConnection::from_attached_with_image(
                            attached.role,
                            attached.root,
                            options,
                            image,
                            Instant::now(),
                            origin_ms,
                        )?
                    }
                    None => ShellConnection::from_attached(attached.role, attached.root, options)?,
                };
                Ok(Self::Files {
                    client: Box::new(client),
                    _relay: Some(Box::new(attached.relay)),
                })
            } else {
                Ok(Self::Files {
                    client: Box::new(ShellConnection::connect_files(socket, options)?),
                    _relay: None,
                })
            }
        } else {
            if auth.is_some() || std::env::var_os("SOPHIA_SHELL_9P_SOCKET").is_some() {
                return Err("IPC comparison cannot bypass an authenticated file launch".into());
            }
            let mut stream = UnixStream::connect(
                std::env::var_os("SOPHIA_SHELL_SOCKET").ok_or("missing IPC endpoint")?,
            )?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.set_write_timeout(Some(WAIT))?;
            stream.write_all(
                &encode_shell_v1_client_hello_frame(ShellV1ClientHello {
                    minimum_revision: 6,
                    maximum_revision: 6,
                    required_capabilities: caps,
                })
                .map_err(|e| format!("{e:?}"))?,
            )?;
            let welcome = decode_shell_v1_server_welcome_frame(&frame(&mut stream)?)
                .map_err(|e| format!("{e:?}"))?;
            if welcome.selected_revision != 6 || welcome.capabilities & caps != caps {
                return Err("wrong welcome".into());
            }
            Ok(Self::Ipc(stream))
        }
    }

    fn client(&mut self) -> Result<&mut ShellConnection> {
        match self {
            Self::Files { client, .. } => Ok(client),
            Self::Ipc(_) => Err("sealed source requires the file wire".into()),
        }
    }

    fn send(&mut self, id: u64, records: &[ShellContentRecord]) -> Result<()> {
        let transaction = TransactionId::from_raw(id);
        match self {
            Self::Ipc(stream) => {
                for record in records {
                    stream.write_all(
                        &encode_shell_content_frame(transaction, record)
                            .map_err(|e| format!("{e:?}"))?,
                    )?;
                }
            }
            Self::Files { client, .. } => {
                let deadline = Instant::now() + WAIT;
                loop {
                    let result = if records.len() == 1 {
                        client.enqueue_content(transaction, &records[0])
                    } else {
                        client.enqueue_content_group(transaction, records)
                    };
                    match result {
                        Ok(()) => break,
                        Err(ShellClientError::QueueSaturated) if Instant::now() < deadline => {
                            #[cfg(feature = "comparator-progress")]
                            sophia_runtime::comparator_counters::progress::add(
                                sophia_runtime::comparator_counters::progress::Counter::PeerWait,
                                1,
                            );
                            client
                                .wait_for_io(deadline.saturating_duration_since(Instant::now()))?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                client.poll_io()?;
            }
        }
        Ok(())
    }

    fn receive(&mut self) -> Result<ShellContentRecord> {
        match self {
            Self::Ipc(stream) => Ok(decode_shell_content_frame(&frame(stream)?)
                .map_err(|e| format!("{e:?}"))?
                .1),
            Self::Files { client, .. } => {
                let deadline = Instant::now() + WAIT;
                loop {
                    #[cfg(feature = "comparator-progress")]
                    sophia_runtime::comparator_counters::progress::add(
                        sophia_runtime::comparator_counters::progress::Counter::PeerContentPoll,
                        1,
                    );
                    // wait_for_io drives the connection before sleeping. Drain its
                    // buffered result here instead of making a second I/O pass.
                    if let Some((_, record)) = client.take_content()? {
                        return Ok(record);
                    }
                    if Instant::now() >= deadline {
                        return Err("response timeout".into());
                    }
                    #[cfg(feature = "comparator-progress")]
                    sophia_runtime::comparator_counters::progress::add(
                        sophia_runtime::comparator_counters::progress::Counter::PeerWait,
                        1,
                    );
                    client.wait_for_io(deadline.saturating_duration_since(Instant::now()))?;
                }
            }
        }
    }
}

fn clock_usec() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    u64::try_from(t.tv_sec).unwrap() * 1_000_000 + u64::try_from(t.tv_nsec).unwrap() / 1000
}

fn emit(event: Event<'_>) -> Result<()> {
    journal::write(&mut std::io::stdout().lock(), event)?;
    Ok(())
}

fn run() -> Result<()> {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--serve"] {
        return Err("expected --serve and explicit SOPHIA_SHELL_CONFIG".into());
    }
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
    if !(5..=8).contains(&object.len())
        || object.keys().any(|k| {
            ![
                "wire",
                "output",
                "frames",
                "height",
                "hold_msec",
                "probe_bytes",
                "source",
                "image",
            ]
            .contains(&k.as_str())
        })
    {
        return Err("configuration requires wire, output, frames, height, hold_msec and optional probe_bytes, source and image".into());
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
    let probe_bytes = if object.contains_key("probe_bytes") {
        number("probe_bytes")?
    } else {
        0
    };
    if output_id == 0
        || !(1..=10000).contains(&frames)
        || !(1..=512).contains(&height)
        || hold > 600000
        || !matches!(probe_bytes, 0 | 4194304)
    {
        return Err("configuration outside bounded workload".into());
    }
    let files = match config["wire"].as_str() {
        Some("files") => true,
        Some("ipc") => false,
        _ => return Err("invalid wire".into()),
    };
    // Bytes is the default. Sealed names its role bounds explicitly and
    // never falls back to bytes.
    let sealed = match object.get("source").map(|v| v.as_str()) {
        None | Some(Some("bytes")) if !object.contains_key("image") => None,
        Some(Some("sealed")) => Some(sealed::parse(
            object
                .get("image")
                .ok_or("sealed source requires image bounds")?,
        )?),
        None | Some(Some("bytes")) => return Err("image bounds require sealed source".into()),
        _ => return Err("invalid source".into()),
    };
    if sealed.is_some() && !files {
        return Err("sealed source requires the file wire".into());
    }
    let mut wire = Wire::connect(files, sealed)?;
    let ShellContentRecord::Limits(limits) = wire.receive()? else {
        return Err("missing limits".into());
    };
    if let Some(config) = sealed {
        sealed::check_limits(config, &limits)?;
    }
    let ShellContentRecord::OutputFacts(facts) = wire.receive()? else {
        return Err("missing output facts".into());
    };
    let output = facts
        .outputs
        .iter()
        .find(|entry| entry.output.id == output_id)
        .ok_or("selected output absent")?;
    let grant = limits.grant;
    if facts.grant != grant || facts.facts_generation == 0 || output.output.generation == 0 {
        return Err("stale or misattributed output facts".into());
    }
    if limits.max_resource_ids < if probe_bytes == 0 { 2 } else { 3 } {
        return Err("insufficient resource identities for paced workload".into());
    }
    wire.send(
        1,
        &[ShellContentRecord::AllocationRequest(
            ContentAllocationRequest {
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
                desired_height: height as u32,
                margins: ContentMargins::default(),
            },
        )],
    )?;
    let ShellContentRecord::AllocationResult(allocation) = wire.receive()? else {
        return Err("missing allocation".into());
    };
    if allocation.status != 1
        || allocation.grant != grant
        || allocation.output != output.output
        || allocation.allocation_request_id != 1
    {
        return Err("allocation refused or misattributed".into());
    }
    // The granted geometry must be the requested geometry: a smaller
    // grant would silently measure a smaller presented surface.
    if allocation.pixel.width != output.local_width || allocation.pixel.height != height as u32 {
        return Err("allocation geometry differs from the requested panel".into());
    }
    if sealed.is_some() {
        sealed::wait_ready(wire.client()?, WAIT)?;
    }
    emit(Event::Start {
        frames,
        wire: if files { "files" } else { "ipc" },
        probe_bytes,
        source: sealed.map(|_| "sealed"),
    })?;
    let bar_bytes = canvas::rgba_bytes(allocation.pixel.width, allocation.pixel.height)
        .ok_or("upload size overflow")?;
    let probe_canvas_bytes = if probe_bytes == 0 {
        0
    } else {
        canvas::rgba_bytes(1024, 1024).ok_or("upload size overflow")?
    };
    let mut canvas = canvas::Canvas::new(usize::try_from(bar_bytes.max(probe_canvas_bytes))?);
    for generation in 1..=frames {
        #[cfg(feature = "comparator-counters")]
        sophia_runtime::comparator_counters::record("peer", "frame_start", generation)?;
        // The optional probe measures transfer acceptance, not presentation.
        // Release it before uploading the bar so only one staging slot is needed.
        // New IDs must increase: probe 1, then alternating bar IDs 2 and 3.
        for probe in [true, false] {
            if probe && probe_bytes == 0 {
                continue;
            }
            let resource = if probe {
                ContentResourceId { id: 1, generation }
            } else {
                resource_for_frame(generation, probe_bytes != 0)
            };
            let width = if probe { 1024 } else { allocation.pixel.width };
            let height = if probe { 1024 } else { allocation.pixel.height };
            let length = canvas::rgba_bytes(width, height).ok_or("upload size overflow")?;
            if length == 0 || length > limits.max_staging_bytes {
                return Err("invalid upload size".into());
            }
            let row = width.checked_mul(4).ok_or("row size overflow")?;
            let chunk_bytes = (limits.max_chunk_bytes / row * row) as usize;
            if chunk_bytes == 0 {
                return Err("row exceeds chunk limit".into());
            }
            let chunks = usize::try_from(length)?.div_ceil(chunk_bytes) as u32;
            let source = match sealed {
                Some(_) => Some(sealed::prepare(
                    width,
                    height,
                    limits.max_resource_bytes,
                    generation,
                )?),
                None => None,
            };
            let bytes = if source.is_none() {
                canvas.fill(usize::try_from(length)?, generation)?
            } else {
                &[]
            };
            // Preserve the attempt before sending. This conservative interval includes
            // flushing the offer journal, but excludes pixel preparation above.
            let begin_usec = clock_usec();
            emit(Event::UploadOffer {
                upload: Upload {
                    connection_epoch: grant.connection_epoch,
                    content_grant_epoch: grant.content_grant_epoch,
                    output: output.output.id,
                    candidate_generation: generation,
                    resource_id: resource.id,
                    resource_generation: resource.generation,
                    width,
                    height,
                    total_bytes: length,
                    begin_usec,
                },
            })?;
            if let Some(source) = source {
                let metadata = ProducerMetadata {
                    offer_id: 0,
                    resource: resource.id,
                    resource_generation: resource.generation,
                    admission_epoch: grant.content_grant_epoch,
                    output_id: output.output.id,
                    allocation_id: allocation.allocation.id,
                    topology_generation: facts.facts_generation,
                    source_kind: 1,
                    source_format: 1,
                    width,
                    height,
                    stride: row,
                    plane_count: 1,
                    descriptor_count: 1,
                    declared_bytes: length,
                    modifier: 0,
                };
                let offer = sealed::begin(wire.client()?, metadata, source)?;
                expect_status(&mut wire, grant, resource, 2)?;
                sealed::settle(wire.client()?, offer, resource, WAIT)?;
            } else {
                wire.send(
                    2,
                    &[ShellContentRecord::ResourceBegin(ContentResourceBegin {
                        grant,
                        resource,
                        width_px: width,
                        height_px: height,
                        rendered_scale_numerator: allocation.scale_numerator,
                        rendered_scale_denominator: allocation.scale_denominator,
                        pixel_format: 1,
                        chunk_count: chunks,
                        total_bytes: length,
                    })],
                )?;
                expect_status(&mut wire, grant, resource, 1)?;
                for (ordinal, chunk) in bytes.chunks(chunk_bytes).enumerate() {
                    wire.send(
                        3,
                        &[ShellContentRecord::ResourceChunk(ContentResourceChunk {
                            grant,
                            resource,
                            ordinal: ordinal as u32,
                            offset: ordinal as u64 * chunk_bytes as u64,
                            bytes: chunk.to_vec(),
                        })],
                    )?;
                }
                wire.send(
                    4,
                    &[ShellContentRecord::ResourceEnd(ContentResourceEnd {
                        grant,
                        resource,
                        total_bytes: length,
                        chunk_count: chunks,
                    })],
                )?;
                expect_status(&mut wire, grant, resource, 2)?;
            }
            let accepted_usec = clock_usec();
            emit(Event::UploadAccepted {
                resource_id: resource.id,
                resource_generation: resource.generation,
                accepted_usec,
            })?;
            if probe {
                wire.send(
                    7,
                    &[ShellContentRecord::ResourceRetire(ContentResourceRetire {
                        grant,
                        resource,
                    })],
                )?;
                let ShellContentRecord::ResourceReleased(released) = wire.receive()? else {
                    return Err("missing probe release".into());
                };
                if released.grant != grant || released.resource != resource || released.reason != 0
                {
                    return Err("wrong probe release".into());
                }
                emit(Event::ProbeReleased {
                    resource_id: resource.id,
                    resource_generation: resource.generation,
                    released_usec: clock_usec(),
                })?;
            }
        }
        let resource = resource_for_frame(generation, probe_bytes != 0);
        let demand_time = clock_usec();
        wire.send(
            5,
            &[ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant,
                output: output.output,
                allocation: ContentAllocationId::default(),
                demand_id: generation,
                reason: 1,
            })],
        )?;
        let ShellContentRecord::FramePermit(permit) = wire.receive()? else {
            return Err("missing permit".into());
        };
        if permit.state != 1
            || permit.grant != grant
            || permit.output != output.output
            || permit.demand_id != generation
            || permit.permit_id == 0
        {
            return Err("permit refused or misattributed".into());
        }
        emit(Event::Permit {
            generation,
            demand_usec: demand_time,
            received_usec: clock_usec(),
        })?;
        let records = [
            ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                grant,
                output: output.output,
                candidate_generation: generation,
                facts_generation: facts.facts_generation,
                pacing_permit: permit.permit_id,
                interaction_generation: 1,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            }),
            ShellContentRecord::CandidateChunk(ContentCandidateChunk {
                grant,
                candidate_generation: generation,
                chunk_ordinal: 0,
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
                placements: vec![ContentPlacement {
                    resource,
                    surface_index: 0,
                    destination_x_px: 0,
                    destination_y_px: 0,
                }],
                targets: vec![],
            }),
            ShellContentRecord::CandidateEnd(ContentCandidateEnd {
                grant,
                candidate_generation: generation,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            }),
        ];
        emit(Event::Offer {
            connection_epoch: grant.connection_epoch,
            content_grant_epoch: grant.content_grant_epoch,
            output: output.output.id,
            candidate_generation: generation,
            submitted_monotonic_usec: clock_usec(),
        })?;
        wire.send(6, &records)?;
        let mut prepared = false;
        loop {
            let ShellContentRecord::CandidateOutcome(outcome) = wire.receive()? else {
                return Err("unexpected candidate response".into());
            };
            if outcome.grant != grant
                || outcome.output != output.output
                || outcome.candidate_generation != generation
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
            let status = if outcome.kind == 2 {
                "presented"
            } else {
                "rejected"
            };
            emit(Event::Terminal {
                connection_epoch: grant.connection_epoch,
                content_grant_epoch: grant.content_grant_epoch,
                output: output.output.id,
                candidate_generation: generation,
                outcome: status,
                kind: outcome.kind,
                reason: outcome.reason,
                observed_monotonic_usec: clock_usec(),
            })?;
            if outcome.kind != 2 {
                return Err("candidate was not presented".into());
            }
            if !prepared || outcome.presentation_epoch == 0 {
                return Err("presentation lacks preparation or epoch".into());
            }
            break;
        }
        // Keep the currently displayed resource until a successor is shown.
        if generation > 1 {
            let old = resource_for_frame(generation - 1, probe_bytes != 0);
            wire.send(
                7,
                &[ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant,
                    resource: old,
                })],
            )?;
            let ShellContentRecord::ResourceReleased(released) = wire.receive()? else {
                return Err("missing retired resource release".into());
            };
            if released.grant != grant || released.resource != old || released.reason != 0 {
                return Err("wrong retired resource release".into());
            }
        }
        #[cfg(feature = "comparator-counters")]
        sophia_runtime::comparator_counters::record("peer", "frame_end", generation)?;
    }
    emit(Event::End {
        offered: frames,
        completed: frames,
    })?;
    std::thread::sleep(Duration::from_millis(hold));
    Ok(())
}

fn expect_status(
    wire: &mut Wire,
    grant: ContentGrant,
    resource: ContentResourceId,
    expected: u16,
) -> Result<()> {
    match wire.receive()? {
        ShellContentRecord::ResourceStatus(s)
            if s.grant == grant && s.resource == resource && s.status == expected =>
        {
            Ok(())
        }
        other => Err(format!("unexpected resource response: {other:?}").into()),
    }
}

fn resource_for_frame(frame: u64, probe: bool) -> ContentResourceId {
    ContentResourceId {
        id: (frame - 1) % 2 + 1 + u64::from(probe),
        generation: (frame - 1) / 2 + 1,
    }
}

fn main() {
    if let Err(error) = run() {
        let _ = emit(Event::Failed {
            error: &error.to_string(),
        });
        std::process::exit(1);
    }
}

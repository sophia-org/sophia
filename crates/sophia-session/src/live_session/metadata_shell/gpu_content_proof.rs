//! Isolated hardware proof for the production shell GPU and content seams.
//!
//! A shell client runs inside the protected domain with a direct render-node
//! grant. It must request the one edge-anchored surface the parameters name,
//! within the proof's content limits, and submit as many candidates as the
//! parameters list outcomes. The proof answers each candidate synthetically;
//! it never acquires DRM master or claims native presentation. The pixels are
//! checked only as far as the selected [`ShellGpuProofPixels`] pattern says,
//! and no pattern proves GPU origin: the proof records the protected device
//! grant, and whether the client actually rendered on that device is shown by
//! the client's own adapter and render evidence, read by its verifier.
//! Client-specific expectations -- what a particular shell prints, which
//! adapter it selects, how it reacts to a failed render -- belong to that
//! client's own verifier, which reads the records emitted here.

use super::gpu::ShellGpuLaunchPolicy;
use sophia_backend_live::LiveRenderDeviceIdentitySnapshot;
use sophia_config::ShellGpuMode;
use sophia_protocol::{
    ContentAllocationId, ContentLogicalRect, ContentMargins, ContentOutputFactsEntry,
    ContentOutputId, ContentPixelRect, TransactionId,
};
use sophia_runtime::{
    ContentAllocationSnapshot, ContentCandidateContext, ContentRenderBundle, ProcessLaunchSpec,
    ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec, ProtectionPath,
    ShellContentAdmissionPolicy, ShellSessionTransport, SupervisedProcessKind, SupervisorCommand,
    SupervisorEvent,
};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod domain;
mod parameters;
pub use domain::exec_client;
pub use parameters::{
    SHELL_GPU_PROOF_BYTES_PER_PIXEL, SHELL_GPU_PROOF_DEFAULT_TIMEOUT,
    SHELL_GPU_PROOF_MAX_OUTPUT_EXTENT, SHELL_GPU_PROOF_MAX_RENDERS, SHELL_GPU_PROOF_MAX_TIMEOUT,
    SHELL_GPU_PROOF_MIN_TIMEOUT, ShellGpuContentProof, ShellGpuProofEnd, ShellGpuProofError,
    ShellGpuProofExtent, ShellGpuProofOutcome, ShellGpuProofPixels, ShellGpuProofSurface,
    shell_gpu_proof_content_limits,
};

/// Candidate intake across the proof's visits to the transport.
///
/// Each visit services one batch, which the transport owner bounds by its
/// negotiated `max_frames_per_service_tick` and output-queue room. The proof
/// adds no cap across visits: a fragmented candidate's Begin, chunks and End
/// may arrive on any number of visits, and a lifetime cap would strand its End
/// until the deadline. The proof's deadline bounds the whole run.
#[derive(Debug, Default)]
struct CandidateIntake {
    serviced: usize,
}

impl CandidateIntake {
    /// Service one owner-bounded batch once a frame permit exists; a
    /// candidate cannot begin without one.
    fn visit<E>(
        &mut self,
        permits_sent: u64,
        service: impl FnOnce() -> Result<usize, E>,
    ) -> Result<usize, E> {
        if permits_sent == 0 {
            return Ok(0);
        }
        let serviced = service()?;
        self.serviced = self.serviced.saturating_add(serviced);
        Ok(serviced)
    }
}

type Inventory = Vec<LiveRenderDeviceIdentitySnapshot>;

/// Run the proof its parameters describe. Parameters are validated before the
/// render inventory is read or any device is touched.
///
/// Only a final RendererFailed render proves lease retention across the
/// client's disconnect. A final PresentedSynthetic end state has not been
/// exercised on hardware; it remains unclaimed until a manual gate runs it.
pub fn run(proof: &ShellGpuContentProof) -> Result<(), Box<dyn std::error::Error>> {
    run_with_inventory(proof, |seat| {
        Ok(sophia_backend_live::snapshot_seat_render_inventory(seat)?)
    })
}

fn run_with_inventory(
    proof: &ShellGpuContentProof,
    inventory: impl FnOnce(&str) -> Result<Inventory, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    proof.validate()?;
    validate_input(&proof.client, "shell client")?;
    if let Some(config) = &proof.config {
        validate_input(config, "shell config")?;
    }
    let devices = inventory(&proof.seat)?;
    let matching = devices
        .iter()
        .filter(|device| device.node == proof.render_node)
        .collect::<Vec<_>>();
    let [device] = matching.as_slice() else {
        return Err("proof requires exactly one admitted render-node identity".into());
    };

    let directory = proof_directory()?;
    let mut transport = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )?;
    let socket = transport.socket_path().to_path_buf();
    let mut observation = [0_u8; 16];
    if rustix::rand::getrandom(&mut observation, rustix::rand::GetRandomFlags::empty())?
        != observation.len()
    {
        return Err("GPU proof observation identity was incomplete".into());
    }
    let observation = observation
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?
        .path(ProtectionPath::read_only(
            socket.parent().ok_or("shell socket lacks a parent")?,
        ))?
        .path(ProtectionPath::read_only(&proof.client))?;
    if let Some(config) = &proof.config {
        domain = domain.path(ProtectionPath::read_only(config))?;
    }
    let mut base = ProcessLaunchSpec::new(std::env::current_exe()?)
        .arg("sophia-shell-gpu-proof-exec")
        .arg(prefixed("--client=", proof.client.as_os_str()));
    for argument in &proof.client_args {
        base = base.arg(prefixed("--client-arg=", argument));
    }
    base = base
        .env(domain::OBSERVATION_ENV, &observation)
        .env(sophia_runtime::SOPHIA_SHELL_SOCKET_ENV, &socket)
        .env(
            "SOPHIA_SHELL_BAR_THICKNESS",
            proof.surface.thickness().to_string(),
        );
    if let Some(config) = &proof.config {
        base = base.env("SOPHIA_SHELL_CONFIG", config);
    }
    let base = base.process_group().protection_domain(domain);
    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some((**device).clone()))?;
    let (spec, gpu) = policy.prepare(&base, 1)?;
    let gpu = gpu.ok_or("direct proof produced no GPU grant evidence")?;
    require_expected_device(&gpu, proof.expected_device.as_deref())?;
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor.apply(SupervisorCommand::StartProcess {
        process: SupervisedProcessKind::Shell,
        delay: Duration::ZERO,
    })?;
    let protection = supervisor
        .protection_evidence()
        .ok_or("shell process has no protection evidence")?
        .clone();
    transport.authorize_protected_peer(&protection)?;
    let welcome = transport.accept_and_negotiate_with_content_policy(
        1,
        Duration::from_secs(5),
        proof_content_admission_policy(proof.discrete_input),
    )?;
    let grant = transport.content_grant().ok_or("content was not granted")?;
    // validate() checked the geometry against this profile; refuse if the
    // transport ever negotiates another one.
    let negotiated = transport
        .content_limits()
        .ok_or("content limits were not negotiated")?;
    if *negotiated != sophia_protocol::ContentLimits::prototype(negotiated.grant) {
        return Err("negotiated content limits differ from the limits the proof validated".into());
    }
    // Environment fields correlate observations; only this protected-peer
    // authorization and completed negotiation establish the parent binding.
    crate::session_println!(
        "sophia_shell_gpu_domain_parent schema=1 status=bound protected=true observation_id={} grant_epoch={} device_major={} device_minor={} peer_pid={} supervisor_pid={}",
        observation,
        gpu.epoch,
        gpu.major,
        gpu.minor,
        protection.peer_pid,
        protection.supervisor_pid,
    );
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    transport.publish_content_output_facts(
        TransactionId::from_raw(1),
        1,
        vec![ContentOutputFactsEntry {
            output,
            local_width: proof.output.width,
            local_height: proof.output.height,
            scale_numerator: 1,
            scale_denominator: 1,
            scale_generation: 1,
        }],
    )?;

    let renders = proof.outcomes.len();
    let started = Instant::now();
    let deadline = started + proof.timeout;
    let mut allocation = None;
    let mut permits_sent = 0_u64;
    let mut intake = CandidateIntake::default();
    let mut presentations = 0_u64;
    let mut last_generation = 0_u64;
    let mut verified = 0_usize;
    let mut retained: Option<ContentRenderBundle> = None;
    let mut stopped = false;
    while Instant::now() < deadline {
        let now = elapsed_msec(started);
        let done = verified == renders;
        service_resources(&mut transport, now, done)
            .map_err(|error| format!("resource intake: {error}"))?;
        service_allocations(&mut transport, now, done)
            .map_err(|error| format!("allocation intake: {error}"))?;
        if allocation.is_none()
            && let Some((_, request)) = transport.next_content_allocation_request()
        {
            if request.output != output
                || request.operation != 1
                || request.role != 1
                || request.edge != proof.surface.edge.wire()
                || request.margins != ContentMargins::default()
                || request.desired_width != proof.surface.width
                || request.desired_height != proof.surface.height
            {
                return Err(
                    "client allocation request differs from the proof's surface parameters".into(),
                );
            }
            // validate() refused every geometry that does not place.
            let pixel = proof
                .surface
                .placement(proof.output)
                .ok_or("validated surface geometry did not place")?;
            let snapshot = ContentAllocationSnapshot {
                native_opening: None,
                output,
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
                scale_generation: 1,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: request.edge,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: pixel.x,
                    y: pixel.y,
                    width: pixel.width,
                    height: pixel.height,
                },
                pixel,
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: proof.surface.thickness(),
            };
            transport
                .grant_content_allocation(request.allocation_request_id, snapshot, &[])
                .map_err(|error| format!("grant surface allocation: {error}"))?;
            allocation = Some(());
        }
        let allocations = transport.content_allocation_snapshots();
        service_demands(&mut transport, output, &allocations, done).map_err(|error| {
            format!("frame-demand intake after {permits_sent} permits: {error}")
        })?;
        while let Some((transaction, demand)) = transport.next_content_demand() {
            if demand.output != output {
                return Err("client demanded an unknown output".into());
            }
            permits_sent = permits_sent
                .checked_add(1)
                .ok_or("proof permit identity exhausted")?;
            transport
                .grant_content_demand(transaction, output, permits_sent, now)
                .map_err(|error| {
                    format!(
                        "grant frame permit {permits_sent} for demand {}: {error}",
                        demand.demand_id
                    )
                })?;
        }
        let serviced_before = intake.serviced;
        intake
            .visit(permits_sent, || {
                transport.service_content_candidates(
                    &[ContentCandidateContext {
                        output,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    now,
                )
            })
            .map_err(|error| {
                format!(
                    "candidate intake after {serviced_before} records and {permits_sent} permits: {error}"
                )
            })?;
        while let Some((candidate_output, generation)) = transport.next_content_submission() {
            if candidate_output != output {
                return Err("client submitted a candidate for an unknown output".into());
            }
            if verified == renders {
                return Err(format!(
                    "client submitted a candidate after the proof's {renders} renders"
                )
                .into());
            }
            if generation <= last_generation {
                return Err("client candidate generations did not increase".into());
            }
            last_generation = generation;
            let index = verified + 1;
            let bundle = transport
                .begin_content_submission(output, generation, now)
                .map_err(|error| format!("candidate {index} submission: {error}"))?;
            let (bytes, checksum) = verify_bundle(&bundle, proof.surface, proof.pixels)?;
            let outcome = proof.outcomes[verified];
            match outcome {
                ShellGpuProofOutcome::PresentedSynthetic => {
                    drop(bundle);
                    presentations += 1;
                    transport
                        .content_prepared(grant, output, generation, 1, 1, now)
                        .map_err(|error| format!("candidate {index} prepared: {error}"))?;
                    transport
                        .content_presented(grant, output, generation, presentations, 1, 1)
                        .map_err(|error| format!("candidate {index} presented: {error}"))?;
                }
                ShellGpuProofOutcome::RendererFailed => {
                    transport
                        .content_renderer_failed(grant, output, generation)
                        .map_err(|error| format!("candidate {index} rejection: {error}"))?;
                    // The final failed render stays renderer-owned so the
                    // disconnect below must retain its lease.
                    if index == renders {
                        retained = Some(bundle);
                    }
                }
            }
            // One record per verified render: the bytes and checksum of the
            // placed resources as they crossed the content path, and the
            // synthetic outcome. It says nothing about which processor drew
            // them.
            crate::session_println!(
                "sophia_shell_gpu_content_render schema=1 index={} generation={} bytes={} checksum={:016x} outcome={}",
                index,
                generation,
                bytes,
                checksum,
                outcome.record_name(),
            );
            verified = index;
        }
        if verified == renders && proof.end == ShellGpuProofEnd::StopClient && !stopped {
            supervisor.terminate()?;
            stopped = true;
        }
        if stopped || supervisor.poll()? == Some(SupervisorEvent::ProcessExited) {
            if verified != renders {
                return Err(format!(
                    "client exited after {verified} of {renders} verified renders"
                )
                .into());
            }
            transport.disconnect()?;
            if let Some(bundle) = retained.take() {
                if transport.content_reserved_bytes() == 0 {
                    return Err(
                        "disconnect failed to retain the renderer-owned content lease".into(),
                    );
                }
                drop(bundle);
                transport.disconnect()?;
            }
            if transport.content_reserved_bytes() != 0
                || transport.content_backing_reserved_bytes() != 0
            {
                return Err("content lease or backing survived the proof's end".into());
            }
            // Completion: the protected grant, the geometry and the pixel
            // pattern checked. native_presentation stays false because every
            // outcome here is synthetic; GPU origin of the pixels is not
            // claimed and belongs to the client's external verifier.
            crate::session_println!(
                "sophia_shell_gpu_content_proof schema=1 status=complete protected=true revision={} capabilities=0x{:x} grant_epoch={} render_node={} device_major={} device_minor={} pci_bus_id={} output_width={} output_height={} edge={} width={} height={} renders={} pixels={} discrete_input={} end={} backing_bytes=0 native_presentation=false",
                welcome.selected_revision,
                welcome.capabilities,
                gpu.epoch,
                gpu.render_node.display(),
                gpu.major,
                gpu.minor,
                gpu.pci_bus_id.as_deref().unwrap_or("none"),
                proof.output.width,
                proof.output.height,
                edge_name(proof.surface.edge),
                proof.surface.width,
                proof.surface.height,
                renders,
                proof.pixels.record_name(),
                proof.discrete_input,
                proof.end.record_name(),
            );
            return Ok(());
        }
        std::thread::yield_now();
    }
    Err(format!(
        "shell GPU/content proof exceeded its {:?} deadline after {verified} of {renders} renders",
        proof.timeout
    )
    .into())
}

fn prefixed(prefix: &str, value: &std::ffi::OsStr) -> OsString {
    let mut argument = OsString::from(prefix);
    argument.push(value);
    argument
}

fn edge_name(edge: sophia_config::ShellComponentEdge) -> &'static str {
    match edge {
        sophia_config::ShellComponentEdge::Top => "top",
        sophia_config::ShellComponentEdge::Bottom => "bottom",
        sophia_config::ShellComponentEdge::Left => "left",
        sophia_config::ShellComponentEdge::Right => "right",
    }
}

/// Discrete input is an explicit parameter: the proof emits no input, but a
/// client may refuse a negotiation that lacks the grant its profile expects.
fn proof_content_admission_policy(discrete_input: bool) -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted { discrete_input }
}

fn require_expected_device(
    evidence: &super::gpu::ShellGpuLaunchEvidence,
    expected: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual = format!(
        "{}:{}@{}",
        evidence.major,
        evidence.minor,
        evidence.pci_bus_id.as_deref().unwrap_or("none")
    );
    if expected.is_some_and(|expected| expected != actual) {
        return Err("GPU proof selected device differs from the pinned expectation".into());
    }
    Ok(())
}

fn validate_input(path: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !path.is_absolute() || !path.is_file() {
        return Err(format!("{name} must be an absolute file").into());
    }
    Ok(())
}

fn proof_directory() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(std::env::temp_dir().join(format!(
        "sophia-shell-gpu-content-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    )))
}

fn elapsed_msec(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn service_resources(
    transport: &mut ShellSessionTransport,
    now: u64,
    allow_disconnect: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match transport.service_content_resources(now) {
        Ok(_) => Ok(()),
        Err(sophia_runtime::ShellTransportError::NotConnected) if allow_disconnect => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn service_allocations(
    transport: &mut ShellSessionTransport,
    now: u64,
    allow_disconnect: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match transport.service_content_allocation_requests(&[], now) {
        Ok(_) => Ok(()),
        Err(sophia_runtime::ShellTransportError::NotConnected) if allow_disconnect => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn service_demands(
    transport: &mut ShellSessionTransport,
    output: ContentOutputId,
    allocations: &[ContentAllocationSnapshot],
    allow_disconnect: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match transport.service_content_demands(&[output], allocations) {
        Ok(_) => Ok(()),
        Err(sophia_runtime::ShellTransportError::NotConnected) if allow_disconnect => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Check one candidate against the proof's pixel pattern and summarise the
/// bytes of its placed resources, in placement order, for the render record.
fn verify_bundle(
    bundle: &ContentRenderBundle,
    surface: ShellGpuProofSurface,
    pixels: ShellGpuProofPixels,
) -> Result<(usize, u64), Box<dyn std::error::Error>> {
    if pixels == ShellGpuProofPixels::FullSurfaceRaster {
        if bundle.surfaces.len() != 1 {
            return Err("full-surface raster: client did not submit exactly one surface".into());
        }
        let first = bundle
            .placements
            .first()
            .ok_or("full-surface raster: the surface has no placement")?;
        let lease = bundle
            .resource(first.resource)
            .ok_or("candidate placement named no admitted resource")?;
        let description = lease.description();
        check_full_surface_raster(
            surface,
            description.width_px,
            description.height_px,
            lease.bytes(),
        )?;
    }
    let mut seen = Vec::new();
    let mut total = 0_usize;
    let mut hash = FNV_OFFSET;
    for placement in &bundle.placements {
        if seen.contains(&placement.resource) {
            continue;
        }
        seen.push(placement.resource);
        let lease = bundle
            .resource(placement.resource)
            .ok_or("candidate placement named no admitted resource")?;
        total = total.saturating_add(lease.bytes().len());
        hash = fnv(hash, lease.bytes());
    }
    Ok((total, hash))
}

/// The full-surface raster peer requirement for one resource: the
/// allocation's exact size in the admitted format, with bytes that are
/// neither empty nor uniform.
fn check_full_surface_raster(
    surface: ShellGpuProofSurface,
    width_px: u32,
    height_px: u32,
    bytes: &[u8],
) -> Result<(), String> {
    if width_px != surface.width || height_px != surface.height {
        return Err("full-surface raster: resource does not match the allocation".into());
    }
    let expected = surface
        .bytes()
        .ok_or("full-surface raster: surface byte size overflows")?;
    let mut pixels = bytes.chunks_exact(SHELL_GPU_PROOF_BYTES_PER_PIXEL as usize);
    let Some(first) = pixels.next() else {
        return Err("full-surface raster: resource holds no complete pixel".into());
    };
    if u64::try_from(bytes.len()).ok() != Some(expected)
        || bytes.iter().all(|byte| *byte == 0)
        || pixels.all(|pixel| pixel == first)
    {
        return Err("full-surface raster: resource bytes are empty or uniform".into());
    }
    Ok(())
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[path = "../../../tests/support/metadata_shell_gpu_content_proof.rs"]
mod tests;

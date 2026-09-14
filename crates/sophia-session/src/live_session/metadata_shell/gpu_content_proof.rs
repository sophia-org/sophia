//! Isolated hardware proof for the production shell GPU and content seams.

use super::gpu::ShellGpuLaunchPolicy;
use sophia_config::ShellGpuMode;
use sophia_protocol::{
    ContentAllocationId, ContentLogicalRect, ContentMargins, ContentOutputFactsEntry,
    ContentOutputId, ContentPixelRect, ContentReason, TransactionId,
};
use sophia_runtime::{
    ContentAllocationSnapshot, ContentCandidateContext, ContentRenderBundle, ProcessLaunchSpec,
    ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec, ProtectionPath,
    ShellContentAdmissionPolicy, ShellSessionTransport, SupervisedProcessKind, SupervisorCommand,
    SupervisorEvent,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const OUTPUT_WIDTH: u32 = 256;
const OUTPUT_HEIGHT: u32 = 64;
const PANEL_HEIGHT: u32 = 24;
const PROOF_TIMEOUT: Duration = Duration::from_secs(15);

/// Run two sequential real Vulkan renders through Lom and accept both complete
/// content candidates. The first receives a synthetic Presented outcome so the
/// production client's replacement path and reusable renderer execute without
/// taking over a native output. The second terminal outcome is deliberately
/// RendererFailed because this process cannot prove native presentation.
pub fn run(
    client: &Path,
    config: &Path,
    seat: &str,
    render_node: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_input(client, "shell client")?;
    validate_input(config, "shell config")?;
    let devices = sophia_backend_live::discover_seat_render_devices(seat)?;
    let matching = devices
        .iter()
        .filter(|device| device.identity.node == render_node)
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
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?
        .path(ProtectionPath::read_only(
            socket.parent().ok_or("shell socket lacks a parent")?,
        ))?
        .path(ProtectionPath::read_only(config))?;
    let base = ProcessLaunchSpec::new(client)
        .arg("--serve")
        .env(sophia_runtime::SOPHIA_SHELL_SOCKET_ENV, &socket)
        .env("SOPHIA_SHELL_CONFIG", config)
        .env("SOPHIA_SHELL_BAR_THICKNESS", PANEL_HEIGHT.to_string())
        .process_group()
        .protection_domain(domain);
    let policy = ShellGpuLaunchPolicy::new(ShellGpuMode::Direct, Some(device.identity.clone()))?;
    let (spec, gpu) = policy.prepare(&base, 1)?;
    let gpu = gpu.ok_or("direct proof produced no GPU grant evidence")?;
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
        proof_content_admission_policy(),
    )?;
    let grant = transport.content_grant().ok_or("content was not granted")?;
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    transport.publish_content_output_facts(
        TransactionId::from_raw(1),
        1,
        vec![ContentOutputFactsEntry {
            output,
            local_width: OUTPUT_WIDTH,
            local_height: OUTPUT_HEIGHT,
            scale_numerator: 1,
            scale_denominator: 1,
            scale_generation: 1,
        }],
    )?;

    let started = Instant::now();
    let deadline = started + PROOF_TIMEOUT;
    let mut allocation = None;
    let mut permits_sent = 0_u64;
    let mut candidate_records = 0;
    let mut render: Option<ContentRenderBundle> = None;
    let mut first_verified = None;
    let mut second_verified = None;
    while Instant::now() < deadline {
        let now = elapsed_msec(started);
        service_resources(&mut transport, now, second_verified.is_some())
            .map_err(|error| format!("resource intake: {error}"))?;
        service_allocations(&mut transport, now, second_verified.is_some())
            .map_err(|error| format!("allocation intake: {error}"))?;
        if allocation.is_none()
            && let Some((_, request)) = transport.next_content_allocation_request()
        {
            if request.output != output
                || request.operation != 1
                || request.role != 1
                || request.edge != 1
                || request.desired_width != OUTPUT_WIDTH
                || request.desired_height != PANEL_HEIGHT
            {
                return Err("Lom changed the admitted first-panel allocation".into());
            }
            let snapshot = ContentAllocationSnapshot {
                output,
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
                scale_generation: 1,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: OUTPUT_WIDTH,
                    height: PANEL_HEIGHT,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: OUTPUT_WIDTH,
                    height: PANEL_HEIGHT,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: PANEL_HEIGHT,
            };
            transport
                .grant_content_allocation(request.allocation_request_id, snapshot, &[])
                .map_err(|error| format!("grant panel allocation: {error}"))?;
            allocation = Some(());
        }
        let allocations = transport.content_allocation_snapshots();
        service_demands(
            &mut transport,
            output,
            &allocations,
            second_verified.is_some(),
        )
        .map_err(|error| format!("frame-demand intake after {permits_sent} permits: {error}"))?;
        while let Some((transaction, demand)) = transport.next_content_demand() {
            if demand.output != output {
                return Err("Lom demanded an unknown output".into());
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
        if permits_sent != 0 && candidate_records < 6 {
            candidate_records += transport
                .service_content_candidates(
                    &[ContentCandidateContext {
                        output,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    now,
                )
                .map_err(|error| {
                    format!(
                        "candidate intake after {candidate_records} records and {permits_sent} permits: {error}"
                    )
                })?;
        }
        while let Some((candidate_output, generation)) = transport.next_content_submission() {
            if candidate_output != output {
                return Err("Lom submitted a candidate for an unknown output".into());
            }
            match generation {
                1 if first_verified.is_none() => {
                    let bundle = transport
                        .begin_content_submission(output, generation, now)
                        .map_err(|error| format!("first candidate submission: {error}"))?;
                    let verified = verify_bundle(&bundle)?;
                    drop(bundle);
                    transport
                        .content_prepared(grant, output, generation, 1, 1, now)
                        .map_err(|error| format!("first candidate prepared: {error}"))?;
                    transport
                        .content_presented(grant, output, generation, 1, 1, 1)
                        .map_err(|error| format!("first candidate presented: {error}"))?;
                    first_verified = Some(verified);
                }
                2 if first_verified.is_some() && second_verified.is_none() => {
                    let bundle = transport
                        .begin_content_submission(output, generation, now)
                        .map_err(|error| format!("second candidate submission: {error}"))?;
                    let verified = verify_bundle(&bundle)?;
                    transport
                        .content_renderer_failed(grant, output, generation)
                        .map_err(|error| format!("second candidate rejection: {error}"))?;
                    render = Some(bundle);
                    second_verified = Some(verified);
                }
                _ => return Err("Lom changed the bounded two-render candidate sequence".into()),
            }
        }
        if supervisor.poll()? == Some(SupervisorEvent::ProcessExited) {
            let Some((first_bytes, first_checksum)) = first_verified else {
                return Err("Lom exited before its first verified panel".into());
            };
            let Some((second_bytes, second_checksum)) = second_verified else {
                return Err("Lom exited before its second verified panel".into());
            };
            transport.disconnect()?;
            if render.is_none() || transport.content_reserved_bytes() == 0 {
                return Err("disconnect failed to retain the renderer-owned content lease".into());
            }
            drop(render.take());
            transport.disconnect()?;
            if transport.content_reserved_bytes() != 0
                || transport.content_backing_reserved_bytes() != 0
            {
                return Err("content lease or backing survived renderer release".into());
            }
            crate::session_println!(
                "sophia_shell_gpu_content_hardware_proof schema=2 status=complete protected=true revision={} capabilities=0x{:x} grant_epoch={} render_node={} device_major={} device_minor={} pci_bus_id={} width={} height={} renders=2 first_bytes={} first_checksum={:016x} second_bytes={} second_checksum={:016x} first_outcome=presented_synthetic second_renderer_outcome={} backing_bytes=0 native_presentation=false",
                welcome.selected_revision,
                welcome.capabilities,
                gpu.epoch,
                gpu.render_node.display(),
                gpu.major,
                gpu.minor,
                gpu.pci_bus_id.as_deref().unwrap_or("none"),
                OUTPUT_WIDTH,
                PANEL_HEIGHT,
                first_bytes,
                first_checksum,
                second_bytes,
                second_checksum,
                ContentReason::RendererFailed as u16,
            );
            return Ok(());
        }
        std::thread::yield_now();
    }
    Err("Lom GPU/content proof exceeded its bounded deadline".into())
}

fn proof_content_admission_policy() -> ShellContentAdmissionPolicy {
    // This launches the production Lom client, so its negotiation must match
    // the production profile even though this isolated proof emits no input.
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}

fn validate_input(path: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !path.is_absolute() || !path.is_file() {
        return Err(format!("{name} must be an absolute file").into());
    }
    Ok(())
}

fn proof_directory() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(std::env::temp_dir().join(format!(
        "sophia-lom-gpu-content-{}-{}",
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

fn verify_bundle(bundle: &ContentRenderBundle) -> Result<(usize, u64), Box<dyn std::error::Error>> {
    if bundle.surfaces.len() != 1 || bundle.placements.is_empty() {
        return Err("Lom did not submit one complete panel surface".into());
    }
    let resource = bundle.placements[0].resource;
    let lease = bundle
        .resource(resource)
        .ok_or("candidate placement named no admitted resource")?;
    let description = lease.description();
    if description.width_px != OUTPUT_WIDTH || description.height_px != PANEL_HEIGHT {
        return Err("Lom GPU pixels do not match the acknowledged allocation".into());
    }
    let bytes = lease.bytes();
    let mut pixels = bytes.chunks_exact(4);
    let first = pixels
        .next()
        .ok_or("Lom GPU render produced no complete pixel")?;
    if bytes.len() != (OUTPUT_WIDTH * PANEL_HEIGHT * 4) as usize
        || bytes.iter().all(|byte| *byte == 0)
        || pixels.all(|pixel| pixel == first)
    {
        return Err("Lom GPU render produced empty or uniform panel bytes".into());
    }
    Ok((bytes.len(), checksum(bytes)))
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[path = "gpu_content_proof/tests.rs"]
mod tests;

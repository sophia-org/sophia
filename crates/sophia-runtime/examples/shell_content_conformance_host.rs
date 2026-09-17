use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_protocol::{
    ContentAllocationId, ContentLogicalRect, ContentMargins, ContentOutputFactsEntry,
    ContentOutputId, ContentPixelRect, ContentReason, ContentResourceId, TransactionId,
};
use sophia_runtime::{
    ContentAllocationSnapshot, ContentCandidateContext, ContentRenderBundle, ProcessLaunchSpec,
    ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec, ProtectionPath,
    ShellContentAdmissionPolicy, ShellSessionTransport, SupervisedProcessKind, SupervisorCommand,
    SupervisorEvent,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("shell-content-conformance-host: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let client = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: shell_content_conformance_host CLIENT")?;
    if !client.is_absolute() || !client.is_file() {
        return Err("shell client must be an absolute executable path".into());
    }
    let directory = std::env::temp_dir().join(format!(
        "sophia-shell-content-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut transport = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )?;
    let socket = transport.socket_path().to_path_buf();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?.path(
        ProtectionPath::read_only(socket.parent().ok_or("shell socket lacks a parent")?),
    )?;
    let spec = ProcessLaunchSpec::new(client)
        .arg("content-proof")
        .arg("--socket")
        .arg(&socket)
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor.apply(SupervisorCommand::StartProcess {
        process: SupervisedProcessKind::Shell,
        delay: Duration::ZERO,
    })?;
    let evidence = supervisor
        .protection_evidence()
        .ok_or("shell process has no protection evidence")?
        .clone();
    transport.authorize_protected_peer(&evidence)?;
    transport.accept_and_negotiate_with_content_policy(
        1,
        Duration::from_secs(5),
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
    )?;
    let grant = transport.content_grant().ok_or("content was not granted")?;
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    transport.publish_content_output_facts(
        TransactionId::from_raw(2),
        1,
        vec![ContentOutputFactsEntry {
            output,
            local_width: 64,
            local_height: 64,
            scale_numerator: 1,
            scale_denominator: 1,
            scale_generation: 1,
        }],
    )?;
    let allocation = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let started = Instant::now();
    let mut render: Option<ContentRenderBundle> = None;
    let mut allocation_granted = false;
    let mut permit_sent = false;
    let mut candidate_records = 0;
    let mut candidate_settled = false;
    let mut verified = false;
    loop {
        if Instant::now() >= deadline {
            return Err("content client did not settle before the host deadline".into());
        }
        match transport.service_content_resources(started.elapsed().as_millis() as u64) {
            Ok(_) => {}
            Err(sophia_runtime::ShellTransportError::NotConnected)
                if verified
                    && transport
                        .content_usage()
                        .is_some_and(|usage| usage == Default::default()) => {}
            Err(error) => return Err(error.into()),
        }
        match transport
            .service_content_allocation_requests(&[], started.elapsed().as_millis() as u64)
        {
            Ok(_) => {}
            Err(sophia_runtime::ShellTransportError::NotConnected) if verified => {}
            Err(error) => return Err(error.into()),
        }
        if !allocation_granted
            && let Some((_, request)) = transport.next_content_allocation_request()
        {
            if request.output != output
                || request.operation != 1
                || request.role != 1
                || request.edge != 1
                || request.desired_width != 64
                || request.desired_height != 32
            {
                return Err("independent client changed its panel allocation request".into());
            }
            transport.grant_content_allocation(
                request.allocation_request_id,
                ContentAllocationSnapshot {
                    native_opening: None,
                    output,
                    allocation,
                    scale_generation: 1,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    logical: ContentLogicalRect {
                        x: 0,
                        y: 0,
                        width: 64,
                        height: 32,
                    },
                    pixel: ContentPixelRect {
                        x: 0,
                        y: 0,
                        width: 64,
                        height: 32,
                    },
                    parent: ContentAllocationId::default(),
                    anchor_parent_rect: ContentPixelRect::default(),
                    allowed_reservation_extent: 32,
                },
                &[],
            )?;
            allocation_granted = true;
        }
        let allocations = transport.content_allocation_snapshots();
        match transport.service_content_demands(&[output], &allocations) {
            Ok(_) => {}
            Err(sophia_runtime::ShellTransportError::NotConnected) if verified => {}
            Err(error) => return Err(error.into()),
        }
        if !permit_sent && let Some((_, demand)) = transport.next_content_demand() {
            if demand.output != output || demand.allocation != ContentAllocationId::default() {
                return Err("independent client demanded an unknown output or allocation".into());
            }
            transport.grant_content_demand(
                TransactionId::from_raw(10),
                output,
                1,
                started.elapsed().as_millis() as u64,
            )?;
            permit_sent = true;
        }
        if permit_sent && candidate_records < 3 {
            let context = ContentCandidateContext {
                output,
                facts_generation: 1,
                interaction_generation: 1,
                allocations: &allocations,
            };
            candidate_records += transport
                .service_content_candidates(&[context], started.elapsed().as_millis() as u64)?;
        }
        if candidate_records == 3 && render.is_none() && !candidate_settled {
            let candidate = transport.begin_content_submission(
                output,
                1,
                started.elapsed().as_millis() as u64,
            )?;
            let pixels = candidate
                .resource(resource)
                .ok_or("candidate omitted its referenced resource")?;
            if pixels.bytes() != [0, 0, 255, 255, 0, 128, 0, 128] {
                return Err("independent client uploaded different canonical pixels".into());
            }
            if candidate.surfaces.len() != 1
                || candidate.placements.len() != 1
                || candidate.targets.len() != 1
            {
                return Err("independent client changed the complete candidate tables".into());
            }
            render = Some(candidate);
            // This host has no native output. Exercise the real terminal failure
            // path instead of manufacturing Prepared or Presented evidence.
            transport.content_renderer_failed(grant, output, 1)?;
            candidate_settled = true;
        }
        if render.is_some()
            && transport
                .content_usage()
                .is_some_and(|usage| usage.retiring == 8)
        {
            drop(render.take());
            transport.service_content_resources(started.elapsed().as_millis() as u64)?;
            verified = true;
        }
        if supervisor.poll()? == Some(SupervisorEvent::ProcessExited) {
            if !verified
                || !candidate_settled
                || transport
                    .content_usage()
                    .is_none_or(|usage| usage != Default::default())
            {
                return Err("client exited before exact lease-backed release".into());
            }
            break;
        }
        std::thread::yield_now();
    }
    transport.disconnect()?;
    println!(
        "sophia_shell_content_transport schema=1 status=complete protected=true allocation=granted bytes=8 accepted=true candidate=accepted renderer_outcome={} lease_retained=true released=true native_presentation=false transaction={}",
        ContentReason::RendererFailed as u16,
        TransactionId::from_raw(1).raw()
    );
    Ok(())
}

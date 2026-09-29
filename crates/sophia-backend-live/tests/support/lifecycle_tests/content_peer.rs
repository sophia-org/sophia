//! Protected cross-language client joined to production projection/composition.
//! Device completion is supplied by Target; this never opens a live device.
//!
//! The independent C peer is built from the pinned SDK without its IPC
//! library. It drives the actual component registry and production owners.
use super::*;
use sophia_runtime::*;
use std::path::{Path, PathBuf};

#[path = "../../../../sophia-conformance/tests/support/c_file_peer.rs"]
mod c_file_peer;

// Exercise the very same allocation-to-frame mapping used by the session,
// without creating a production API solely for an integration fixture.
#[path = "../../../../sophia-session/src/live_session/metadata_shell/content/projection.rs"]
mod session_projection;
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");

#[test]
fn protected_popout_file_client_uses_composition_and_retirement_owners() {
    let scratch = PeerScratch::new();
    run(&scratch.peer(0), false).unwrap();
}

#[test]
fn protected_popout_file_client_refuses_an_action_with_the_wrong_receipt() {
    let scratch = PeerScratch::new();
    run(&scratch.peer(0), true).unwrap();
}

/// Receipt and custody mutants of the file peer. Each must fail the shared
/// lifecycle's own assertions, not merely stop early.
#[test]
fn protected_popout_file_client_mutants_fail_their_receipt_and_custody_checks() {
    let scratch = PeerScratch::new();
    // An early exit is refused by whichever lifecycle rule sees it first: the
    // supervisor's exit check, or a lost peer outside the settled disconnect.
    let unsettled: &[&str] = &[
        "peer exited before full lifecycle and lease release",
        "NotConnected",
    ];
    for (mutation, expected) in [
        // The first action's receipt names another event.
        (4, &["assertion `left == right` failed"][..]),
        // A second receipt for the dismissal has no action to settle.
        (5, &["unexpected action receipt"][..]),
        // The dismissal is never acknowledged.
        (6, unsettled),
        // The resource is never retired after the parent loss.
        (7, unsettled),
    ] {
        let peer = scratch.peer(mutation);
        let failure =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&peer, false))) {
                Ok(Ok(())) => panic!("mutant {mutation} completed the lifecycle"),
                Ok(Err(error)) => error.to_string(),
                Err(panic) => panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                    .unwrap_or_default(),
            };
        assert!(
            expected.iter().any(|text| failure.contains(text)),
            "mutant {mutation}: {failure}"
        );
    }
}

struct PeerScratch(PathBuf);

impl PeerScratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-popout-file-peer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn peer(&self, mutation: u32) -> PathBuf {
        c_file_peer::build(&self.0, "shell_content_file_peer", mutation)
    }
}

impl Drop for PeerScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The actual component and shared resource owners behind the connection view.
struct Owner(ShellComponentTransport, ContentEpochRegistry);

impl Owner {
    fn connection(&mut self) -> ShellTransportConnection<'_> {
        self.0.connection(&mut self.1)
    }

    fn disconnect(&mut self) -> Result<(), ShellTransportError> {
        let result = self.0.disconnect(&mut self.1);
        self.1.collect();
        result
    }
}

fn run(client: &Path, wrong_action_epoch: bool) -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::temp_dir().join(format!(
        "sophia-popout-peer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let euid = rustix::process::geteuid().as_raw();
    let mut owner = Owner(
        ShellComponentTransport::bind_for_supervised_uid(&directory, euid)?,
        ContentEpochRegistry::new(64 * 1024 * 1024).map_err(ShellTransportError::from)?,
    );
    let socket = owner.0.socket_path().to_path_buf();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?
        .path(ProtectionPath::read_only(&directory))?;
    let spec = ProcessLaunchSpec::new(client.to_path_buf())
        .arg("content-lifecycle")
        .arg("--socket")
        .arg(socket)
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor.apply(SupervisorCommand::StartProcess {
        process: SupervisedProcessKind::Shell,
        delay: Duration::ZERO,
    })?;
    let evidence = supervisor
        .protection_evidence()
        .ok_or("no protection evidence")?;
    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    };
    owner.0.authorize_protected_peer(evidence)?;
    owner
        .0
        .begin_file_negotiation(&owner.1, 1, Duration::from_secs(5), policy)?;
    // The owner's deadline bounds negotiation; each visit serves bounded 9P turns.
    while owner.0.poll_negotiation(&mut owner.1, 64 * 1024)?.is_none() {
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut transport = owner.connection();
    let grant = transport.content_grant().ok_or("no grant")?;
    let head = HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    };
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    let parent = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    let popup = ContentAllocationId {
        id: 2,
        generation: 1,
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[head], None)?;
    let scene = LiveProductionCpuScene::new(head.size);
    let mut target = Target::new(&[head]);
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
    let started = Instant::now();
    let mut candidate = 0;
    let mut records = 0;
    let mut epochs = Vec::new();
    let mut receipts = Vec::new();
    let mut capture = ContentCaptureState::default();
    let mut expected_action: Option<ContentAction> = None;
    let mut dismissed = false;
    let mut parent_lost = false;
    let mut retained = None;
    let mut negative = false;
    loop {
        if started.elapsed() > Duration::from_secs(10) {
            return Err("lifecycle deadline expired".into());
        }
        let now = started.elapsed().as_millis() as u64;
        if supervisor.poll()? == Some(SupervisorEvent::ProcessExited) {
            if wrong_action_epoch && candidate == 2 && expected_action.is_some() {
                negative = true;
                break;
            }
            if !parent_lost
                || !dismissed
                || candidate != 4
                || transport
                    .content_usage()
                    .is_none_or(|u| u != Default::default())
            {
                return Err("peer exited before full lifecycle and lease release".into());
            }
            break;
        }
        let turn = (|| -> Result<(), Box<dyn std::error::Error>> {
            transport.service_content_resources(now)?;
            transport.service_content_allocation_requests(&receipts, now)?;
            while let Some((_, request)) = transport.next_content_allocation_request() {
                let popout = request.role == 2;
                if request.output != output
                    || request.operation != 1
                    || request.edge != 1
                    || request.desired_width != if popout { 16 } else { 64 }
                    || request.desired_height != if popout { 8 } else { 16 }
                {
                    return Err("unexpected allocation request".into());
                }
                if popout
                    && (candidate != 1
                        || request.parent != parent
                        || request.parent_presentation_epoch != epochs[0])
                {
                    return Err("popout did not name the actual parent receipt".into());
                }
                let pixel = ContentPixelRect {
                    x: if popout { 3 } else { 0 },
                    y: if popout { 5 } else { 0 },
                    width: request.desired_width,
                    height: request.desired_height,
                };
                transport.grant_content_allocation(
                    request.allocation_request_id,
                    ContentAllocationSnapshot {
                        native_opening: None,
                        output,
                        allocation: if popout { popup } else { parent },
                        scale_generation: 1,
                        scale_numerator: 1,
                        scale_denominator: 1,
                        role: request.role,
                        edge: 1,
                        margins: request.margins,
                        logical: ContentLogicalRect {
                            x: pixel.x,
                            y: pixel.y,
                            width: pixel.width,
                            height: pixel.height,
                        },
                        pixel,
                        parent: request.parent,
                        anchor_parent_rect: request.anchor_parent_rect,
                        allowed_reservation_extent: if popout { 0 } else { 16 },
                    },
                    &receipts,
                )?;
            }
            let allocations = transport.content_allocation_snapshots();
            transport.service_content_demands(&[output], &allocations)?;
            if let Some((_, demand)) = transport.next_content_demand() {
                if demand.output != output {
                    return Err("foreign demand".into());
                }
                transport.grant_content_demand(
                    TransactionId::from_raw(100 + candidate),
                    output,
                    demand.demand_id,
                    now,
                )?;
            }
            let context = ContentCandidateContext {
                output,
                facts_generation: 1,
                interaction_generation: candidate + 1,
                allocations: &allocations,
            };
            records += transport.service_content_candidates(&[context], now)?;
            if records == 3 {
                records = 0;
                candidate += 1;
                let bundle = transport.begin_content_submission(output, candidate, now)?;
                let count = if matches!(candidate, 2 | 3) { 2 } else { 1 };
                assert_eq!(
                    (
                        bundle.surfaces.len(),
                        bundle.placements.len(),
                        bundle.targets.len()
                    ),
                    (count, count, count)
                );
                assert_eq!(
                    bundle
                        .resource(ContentResourceId {
                            id: 1,
                            generation: 1
                        })
                        .unwrap()
                        .bytes(),
                    [0, 0, 255, 255, 0, 128, 0, 128]
                );
                if retained.is_none() {
                    retained = Some(
                        bundle
                            .resource(ContentResourceId {
                                id: 1,
                                generation: 1,
                            })
                            .unwrap()
                            .clone(),
                    );
                }
                let frame =
                    session_projection::project_render_bundle(&bundle, head, output, &allocations)?;
                runtime.set_shell_content_on_target(frame, &scene, Some(&mut target))?;
                assert!(
                    runtime
                        .shell_content_presentation_epoch(head.id, grant, candidate)
                        .is_none(),
                    "queued is not presented"
                );
                transport.content_prepared(grant, output, candidate, 1, 1, now)?;
                target.complete(head.id);
                runtime.publish_presented_input_layers(&target);
                let epoch = runtime
                    .shell_content_presentation_epoch(head.id, grant, candidate)
                    .ok_or("no owner presentation")?;
                epochs.push(epoch);
                receipts.clear();
                receipts.push((parent, epoch));
                if count == 2 {
                    receipts.push((popup, epoch));
                }
                transport.content_presented(grant, output, candidate, epoch, 1, 1)?;
                drop(bundle);
                if matches!(candidate, 2 | 3) {
                    let dismiss = candidate == 3;
                    let point = if dismiss {
                        Point { x: 50.0, y: 50.0 }
                    } else {
                        Point { x: 3.5, y: 5.5 }
                    };
                    let press = pointer(&mut capture, &runtime, point, true);
                    let action = if dismiss {
                        let ContentPointerDisposition::OutsideDismiss(hit) = press else {
                            panic!("outside press must be consumed as dismissal");
                        };
                        ContentAction {
                            grant: hit.grant,
                            output: hit.output,
                            candidate_generation: hit.candidate_generation,
                            presentation_epoch: hit.presentation_epoch,
                            interaction_generation: hit.interaction_generation,
                            allocation: hit.allocation,
                            target_id: 0,
                            target_generation: 0,
                            action_id: 0,
                            event_id: candidate - 1,
                            kind: 2,
                            reason: 0,
                        }
                    } else {
                        assert_eq!(press, ContentPointerDisposition::Captured);
                        let ContentPointerDisposition::Activated(hit) =
                            pointer(&mut capture, &runtime, point, false)
                        else {
                            panic!("matching release must activate presented popup");
                        };
                        ContentAction {
                            grant: hit.grant,
                            output: hit.output,
                            candidate_generation: hit.candidate_generation,
                            presentation_epoch: hit.presentation_epoch,
                            interaction_generation: hit.interaction_generation,
                            allocation: hit.allocation,
                            target_id: hit.target_id,
                            target_generation: hit.target_generation,
                            action_id: hit.action_id,
                            event_id: candidate - 1,
                            kind: 1,
                            reason: 0,
                        }
                    };
                    let mut wire_action = action.clone();
                    if wrong_action_epoch {
                        wire_action.presentation_epoch += 1;
                    }
                    transport.send_content_action(
                        TransactionId::from_raw(200 + candidate),
                        &wire_action,
                    )?;
                    expected_action = Some(action);
                }
                if candidate == 4 {
                    runtime.remove_shell_component_content_on_target(
                        head.id,
                        LiveShellContentLayer::Shell,
                        grant,
                        candidate,
                        &scene,
                        Some(&mut target),
                    )?;
                    assert!(!runtime.input_projections[0].content[0].authority_current);
                    target.complete(head.id);
                    runtime.publish_presented_input_layers(&target);
                    assert!(runtime.input_projections[0].content.is_empty());
                    transport.invalidate_content_allocation(
                        TransactionId::from_raw(300),
                        parent,
                        ContentReason::AllocationLost,
                    )?;
                    parent_lost = true;
                }
            }
            if let Some((_, ack)) = transport.poll_content_action_ack()? {
                let action = expected_action.take().ok_or("unexpected action receipt")?;
                assert_eq!(
                    (
                        ack.grant,
                        ack.output,
                        ack.candidate_generation,
                        ack.presentation_epoch,
                        ack.interaction_generation,
                        ack.allocation,
                        ack.target_id,
                        ack.target_generation,
                        ack.action_id,
                        ack.event_id,
                        ack.disposition
                    ),
                    (
                        action.grant,
                        action.output,
                        action.candidate_generation,
                        action.presentation_epoch,
                        action.interaction_generation,
                        action.allocation,
                        action.target_id,
                        action.target_generation,
                        action.action_id,
                        action.event_id,
                        1
                    )
                );
                transport.retain_content_action_reservations(|_| false);
                if action.kind == 2 {
                    assert!(runtime.withdraw_shell_popout_on_target(
                        grant,
                        output,
                        popup,
                        &scene,
                        Some(&mut target)
                    )?);
                    assert!(!runtime.input_projections[0].content[0].authority_current);
                    target.complete(head.id);
                    runtime.publish_presented_input_layers(&target);
                    let binding = &runtime.input_projections[0].content[0];
                    assert!(binding.authority_current && binding.popouts.is_empty());
                    assert_eq!(binding.presentation_epoch, action.presentation_epoch);
                    transport.invalidate_content_allocation(
                        TransactionId::from_raw(299),
                        popup,
                        ContentReason::AllocationLost,
                    )?;
                    assert_eq!(
                        pointer(&mut capture, &runtime, Point { x: 50.0, y: 50.0 }, false),
                        ContentPointerDisposition::Consumed,
                        "release debt survives popout withdrawal"
                    );
                    dismissed = true;
                }
            }
            if parent_lost && transport.content_usage().is_some_and(|u| u.retiring == 8) {
                assert!(retained.is_some(), "release precedes final lease");
                retained.take();
                transport.service_content_resources(now)?;
            }
            Ok(())
        })();
        if let Err(error) = turn {
            let settled_disconnect = parent_lost
                && dismissed
                && candidate == 4
                && transport
                    .content_usage()
                    .is_some_and(|u| u == Default::default())
                && matches!(
                    error.downcast_ref::<ShellTransportError>(),
                    Some(ShellTransportError::NotConnected)
                );
            let negative_disconnect = wrong_action_epoch
                && candidate == 2
                && expected_action.is_some()
                && matches!(
                    error.downcast_ref::<ShellTransportError>(),
                    Some(ShellTransportError::NotConnected)
                );
            if !settled_disconnect && !negative_disconnect {
                return Err(error);
            }
        }
        std::thread::yield_now();
    }
    target.teardown();
    if negative {
        owner.disconnect()?;
        println!(
            "protected_popout_negative status=complete mutation=action_epoch peer_closed_without_ack=true wire=9p2000.L"
        );
        return Ok(());
    }
    assert_eq!(target.backing_owners.get(), 0);
    owner.disconnect()?;
    println!(
        "protected_popout_lifecycle status=complete candidates=4 action=1 dismissal=1 parent_loss=1 leases=released device_completion=simulated native_acceptance=false wire=9p2000.L"
    );
    Ok(())
}

fn pointer(
    capture: &mut ContentCaptureState,
    runtime: &LiveProductionVisualRuntime,
    point: Point,
    pressed: bool,
) -> ContentPointerDisposition {
    resolve_content_pointer_stack(
        capture,
        SeatId::from_raw(1),
        DeviceId::from_raw(1),
        InputEventKind::PointerButton {
            button: 0x110,
            pressed,
        },
        Some(point),
        &runtime.input_projections[0].content,
        false,
    )
}

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sophia_engine::{
    CHROME_PRIMARY_BUTTON, ChromeDescriptorTable, DescriptorOverlayCandidate,
    DescriptorOverlayEntry, PresentedChromeCaptureState, PresentedChromePointerDisposition,
    descriptor_overlay_projection, resolve_presented_chrome_pointer_event,
};
use sophia_protocol::{
    AttentionState, ChromeDescriptor, DeviceId, DisplayLabel, IconTokenId, InputEventKind,
    OutputId, Point, Rect, SeatId, ShellV1Activation, ShellV1ActivationDisposition,
    ShellV1Candidate, ShellV1CandidateOutcome, ShellV1CandidateOutcomeKind, ShellV1Descriptor,
    ShellV1DescriptorSnapshot, SurfaceId, ToplevelActionCapabilityRef, TransactionId, TrustLevel,
};
use sophia_runtime::{
    ContentEpochRegistry, ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole,
    ProtectionDomainSpec, ProtectionPath, ShellComponentTransport, SupervisedProcessKind,
    SupervisorCommand,
};

const OUTPUT: OutputId = OutputId::from_raw(1);
const FIRST: SurfaceId = SurfaceId::new(10, 1);
const SECOND: SurfaceId = SurfaceId::new(11, 1);

fn main() {
    if let Err(error) = run() {
        eprintln!("shell-descriptor-conformance-host: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let client = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: shell_descriptor_conformance_host CLIENT [--proof|--serve|--bar-proof] [CLIENT_ARGS...]")?;
    let client_mode = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "--proof".to_owned());
    if !matches!(client_mode.as_str(), "--proof" | "--serve" | "--bar-proof") {
        return Err("shell client mode must be --proof, --serve, or --bar-proof".into());
    }
    if !client.is_absolute() || !client.is_file() {
        return Err("shell client must be an absolute executable path".into());
    }
    let directory = std::env::temp_dir().join(format!(
        "sophia-shell-conformance-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )?;
    let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).map_err(|e| format!("{e:?}"))?;
    let socket = transport.socket_path().to_path_buf();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?.path(
        ProtectionPath::read_only(socket.parent().ok_or("shell socket lacks a parent")?),
    )?;
    let bar_proof = client_mode == "--bar-proof";
    let mut spec = ProcessLaunchSpec::new(client)
        .arg(&client_mode)
        .env("SOPHIA_SHELL_9P_SOCKET", &socket)
        .process_group()
        .protection_domain(domain);
    // Pass fixture/client options as argv, without interpreting or evaluating them.
    for argument in std::env::args_os().skip(3) {
        spec = spec.arg(argument);
    }
    if bar_proof {
        spec = spec.env("SOPHIA_SHELL_BAR_THICKNESS", "28");
    }
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
    transport.begin_descriptor_file_negotiation(
        &epochs,
        1,
        Duration::from_secs(5),
        sophia_runtime::ShellContentAdmissionPolicy::Unavailable,
    )?;
    wait_for(&mut transport, &mut epochs, |transport, epochs| {
        transport.poll_negotiation(epochs, 65536)
    })?;

    let (table, snapshot, surfaces) = fixture();
    if bar_proof {
        return run_bar_proof(&mut transport, &mut epochs, &mut supervisor, &snapshot);
    }
    if client_mode == "--serve" {
        tab_protocol_proof(&mut transport, &mut epochs, &snapshot)?;
        reference_protocol_proof(&mut transport, &mut epochs)?;
    }
    let candidate_transaction = TransactionId::from_raw(1);
    let candidate = transport.request_candidate(&mut epochs, candidate_transaction, &snapshot)?;
    let projection_candidate = resolve_candidate(&candidate, &snapshot, &surfaces)?;
    let projection = descriptor_overlay_projection(
        &projection_candidate,
        &table,
        Rect {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        },
    )?;
    transport.send_candidate_outcome(
        &mut epochs,
        candidate_transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch: 0,
            kind: ShellV1CandidateOutcomeKind::Prepared,
        },
    )?;
    let presentation_epoch = 12;
    transport.send_candidate_outcome(
        &mut epochs,
        candidate_transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch,
            kind: ShellV1CandidateOutcomeKind::Presented,
        },
    )?;

    let target = projection
        .targets
        .iter()
        .find(|target| Some(target.id.slot) == candidate.selected_slot)
        .ok_or("presented selection has no target")?;
    let point = Point {
        x: f64::from(target.geometry.x + 4),
        y: f64::from(target.geometry.y + 4),
    };
    let mut capture = PresentedChromeCaptureState::default();
    let press = resolve_presented_chrome_pointer_event(
        &mut capture,
        SeatId::from_raw(1),
        DeviceId::from_raw(1),
        InputEventKind::PointerButton {
            button: CHROME_PRIMARY_BUTTON,
            pressed: true,
        },
        Some(point),
        Some(OUTPUT),
        presentation_epoch,
        &projection.targets,
        Some(projection.geometry),
        false,
    )
    .map_err(|error| format!("Engine rejected shell pointer press: {error:?}"))?;
    if press != PresentedChromePointerDisposition::Captured {
        return Err("Engine did not capture the presented shell target".into());
    }
    let release = resolve_presented_chrome_pointer_event(
        &mut capture,
        SeatId::from_raw(1),
        DeviceId::from_raw(1),
        InputEventKind::PointerButton {
            button: CHROME_PRIMARY_BUTTON,
            pressed: false,
        },
        Some(point),
        Some(OUTPUT),
        presentation_epoch,
        &projection.targets,
        Some(projection.geometry),
        false,
    )
    .map_err(|error| format!("Engine rejected shell pointer release: {error:?}"))?;
    let PresentedChromePointerDisposition::Activated { action, activation } = release else {
        return Err("Engine did not activate the exact presented shell target".into());
    };
    transport.queue_activation(
        &mut epochs,
        TransactionId::from_raw(2),
        ShellV1Activation {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch,
            activation,
            action,
        },
    )?;
    let ack = transport.receive_activation_ack(&mut epochs)?;
    if ack.disposition != ShellV1ActivationDisposition::Consumed {
        return Err("shell rejected an exact activation".into());
    }

    let mut withdrawal_snapshot = snapshot;
    withdrawal_snapshot.snapshot_generation += 1;
    let withdrawal_transaction = TransactionId::from_raw(3);
    let withdrawal =
        transport.request_candidate(&mut epochs, withdrawal_transaction, &withdrawal_snapshot)?;
    if withdrawal.visible || !withdrawal.entries.is_empty() || withdrawal.selected_slot.is_some() {
        return Err("shell withdrawal retained visible targets".into());
    }
    transport.send_candidate_outcome(
        &mut epochs,
        withdrawal_transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: withdrawal.candidate_generation,
            presentation_epoch: 0,
            kind: ShellV1CandidateOutcomeKind::Prepared,
        },
    )?;
    transport.send_candidate_outcome(
        &mut epochs,
        withdrawal_transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: withdrawal.candidate_generation,
            presentation_epoch: presentation_epoch + 1,
            kind: ShellV1CandidateOutcomeKind::Presented,
        },
    )?;
    if client_mode == "--serve" {
        let mut receipt = withdrawal_snapshot.clone();
        receipt.snapshot_generation += 1;
        receipt.descriptors.clear();
        let response =
            transport.request_candidate(&mut epochs, TransactionId::from_raw(900), &receipt)?;
        if response.visible || !response.entries.is_empty() {
            return Err("final empty snapshot was not withdrawn".into());
        }
        // Exercise the admitted client's handler, not bwrap's launcher process.
        // Group termination remains the supervisor's error/unwind cleanup.
        let peer = rustix::process::Pid::from_raw(
            supervisor.peer_id().ok_or("missing admitted peer")? as i32,
        )
        .ok_or("invalid admitted peer pid")?;
        rustix::process::kill_process(peer, rustix::process::Signal::TERM)?;
    }
    finish_client(&mut transport, &mut epochs, &mut supervisor)?;
    transport.disconnect(&mut epochs)?;
    println!(
        "sophia_shell_descriptor_corpus schema=1 status=complete protected=true descriptors=2 activations=1 withdrawn=true surface_ids_disclosed=0 coordinates_disclosed=0 icons_disclosed=0"
    );
    Ok(())
}

/// Reserve, commit, withdraw -- against the real coordinator.
///
/// This is the offline half of the reservation gate: it proves the claim
/// survives the wire, that Engine admits it against a realized topology, that
/// the work area shrinks only after the bundle commits, and that a withdrawal
/// carrying no reservation restores the full area through the same path.
fn run_bar_proof(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    supervisor: &mut ProcessSupervisor,
    snapshot: &ShellV1DescriptorSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    use sophia_engine::{ShellWorkAreaCoordinator, reduce_output_work_areas};
    use sophia_protocol::Rect;

    let output = snapshot.output;
    let bounds = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let outputs = [(output, bounds)];
    let mut coordinator = ShellWorkAreaCoordinator::new();

    let transaction = TransactionId::from_raw(1);
    let candidate = transport.request_candidate(epochs, transaction, snapshot)?;
    let reservation = candidate
        .reservation
        .ok_or("shell bar candidate carried no reservation")?;
    let prepared = coordinator
        .admit(
            1,
            candidate.connection_epoch,
            candidate.candidate_generation,
            candidate.output,
            Some(reservation),
            bounds,
            &outputs,
        )
        .map_err(|refusal| format!("Engine refused the bar claim: {}", refusal.reason()))?;
    if prepared.reservation.is_none() {
        return Err("Engine admitted the bar claim as a withdrawal".into());
    }
    // Prepared is not presented: the work area may not move yet.
    if !coordinator.active_bands().is_empty() {
        return Err("a prepared bar claim reduced the work area before it presented".into());
    }
    transport.send_candidate_outcome(
        epochs,
        transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch: 0,
            kind: ShellV1CandidateOutcomeKind::Prepared,
        },
    )?;
    transport.send_candidate_outcome(
        epochs,
        transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch: 10,
            kind: ShellV1CandidateOutcomeKind::Presented,
        },
    )?;
    if !coordinator.commit(candidate.connection_epoch, candidate.candidate_generation) {
        return Err("Engine refused to commit the exact prepared bar bundle".into());
    }
    let reserved = reduce_output_work_areas(
        bounds,
        outputs.iter().copied(),
        &[],
        &coordinator.active_bands(),
    )[0]
    .work
    .ok_or("the reserved work-area reduction rejected its output")?;
    let expected = bounds.height - i32::from(reservation.thickness_px);
    if reserved.height != expected {
        return Err(format!(
            "reserved work area is {} tall, expected {expected}",
            reserved.height
        )
        .into());
    }

    let mut withdrawal_snapshot = snapshot.clone();
    withdrawal_snapshot.snapshot_generation += 1;
    let withdrawal_transaction = TransactionId::from_raw(2);
    let withdrawal =
        transport.request_candidate(epochs, withdrawal_transaction, &withdrawal_snapshot)?;
    if withdrawal.reservation.is_some() {
        return Err("shell withdrawal retained its reservation".into());
    }
    coordinator
        .admit(
            1,
            withdrawal.connection_epoch,
            withdrawal.candidate_generation,
            withdrawal.output,
            None,
            bounds,
            &outputs,
        )
        .map_err(|refusal| format!("Engine refused the withdrawal: {}", refusal.reason()))?;
    transport.send_candidate_outcome(
        epochs,
        withdrawal_transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: withdrawal.candidate_generation,
            presentation_epoch: 0,
            kind: ShellV1CandidateOutcomeKind::Prepared,
        },
    )?;
    transport.send_candidate_outcome(
        epochs,
        withdrawal_transaction,
        ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: withdrawal.candidate_generation,
            presentation_epoch: 11,
            kind: ShellV1CandidateOutcomeKind::Presented,
        },
    )?;
    if !coordinator.commit(withdrawal.connection_epoch, withdrawal.candidate_generation) {
        return Err("Engine refused to commit the withdrawal bundle".into());
    }
    if !coordinator.active_bands().is_empty() || coordinator.presented().is_some() {
        return Err("the withdrawal left a presented claim behind".into());
    }
    finish_client(transport, epochs, supervisor)?;
    transport.disconnect(epochs)?;
    println!(
        "sophia_shell_reservation_corpus schema=1 status=complete protected=true edge=bottom thickness={} reserved_height={} withdrawn=true",
        reservation.thickness_px, reserved.height,
    );
    Ok(())
}

fn resolve_candidate(
    candidate: &ShellV1Candidate,
    snapshot: &ShellV1DescriptorSnapshot,
    surfaces: &BTreeMap<u16, SurfaceId>,
) -> Result<DescriptorOverlayCandidate, Box<dyn std::error::Error>> {
    if candidate.connection_epoch != snapshot.connection_epoch
        || candidate.snapshot_generation != snapshot.snapshot_generation
        || candidate.output != snapshot.output
        || !candidate.visible
        || candidate.entries.is_empty()
    {
        return Err("shell candidate does not match its complete snapshot".into());
    }
    let descriptors = snapshot
        .descriptors
        .iter()
        .map(|descriptor| (descriptor.slot, descriptor))
        .collect::<BTreeMap<_, _>>();
    let entries = candidate
        .entries
        .iter()
        .map(|entry| {
            let descriptor = descriptors
                .get(&entry.slot)
                .ok_or("shell candidate names an unknown descriptor")?;
            if descriptor.generation != entry.generation {
                return Err("shell candidate names a stale descriptor");
            }
            Ok(DescriptorOverlayEntry {
                slot: entry.slot,
                surface: *surfaces
                    .get(&entry.slot)
                    .ok_or("shell descriptor has no private surface mapping")?,
                descriptor_generation: entry.generation,
                action: descriptor.action,
            })
        })
        .collect::<Result<Vec<_>, &str>>()?;
    Ok(DescriptorOverlayCandidate {
        projection: candidate.candidate_generation,
        generation: candidate.candidate_generation,
        output: candidate.output,
        broker_epoch: snapshot.broker_epoch,
        broker_revocation_epoch: snapshot.broker_revocation_epoch,
        shell_session_epoch: candidate.connection_epoch,
        selected_slot: candidate.selected_slot,
        entries,
    })
}

fn fixture() -> (
    ChromeDescriptorTable,
    ShellV1DescriptorSnapshot,
    BTreeMap<u16, SurfaceId>,
) {
    let descriptor = |surface, generation, label: &str, trust_level, attention| ChromeDescriptor {
        surface,
        label: Some(DisplayLabel {
            text: label.into(),
            redacted: false,
        }),
        icon: Some(IconTokenId::from_raw(generation)),
        trust_level,
        attention,
        generation,
    };
    let mut table = ChromeDescriptorTable::default();
    table.upsert(descriptor(
        FIRST,
        8,
        "Terminal",
        TrustLevel::Trusted,
        AttentionState::None,
    ));
    table.upsert(descriptor(
        SECOND,
        9,
        "Browser",
        TrustLevel::Isolated,
        AttentionState::Notice,
    ));
    let action = |slot, generation, token| ToplevelActionCapabilityRef {
        token,
        issuer_epoch: 3,
        issuer_revocation_epoch: 4,
        recipient_epoch: 1,
        target_slot: slot,
        target_generation: generation,
    };
    let snapshot = ShellV1DescriptorSnapshot {
        connection_epoch: 1,
        snapshot_generation: 10,
        output: OUTPUT,
        output_generation: 1,
        broker_epoch: 3,
        broker_revocation_epoch: 4,
        descriptors: vec![
            ShellV1Descriptor {
                slot: 1,
                generation: 8,
                label: Some(DisplayLabel {
                    text: "Terminal".into(),
                    redacted: false,
                }),
                trust_level: TrustLevel::Trusted,
                attention: AttentionState::None,
                action: action(1, 8, 101),
            },
            ShellV1Descriptor {
                slot: 2,
                generation: 9,
                label: Some(DisplayLabel {
                    text: "Browser".into(),
                    redacted: false,
                }),
                trust_level: TrustLevel::Isolated,
                attention: AttentionState::Notice,
                action: action(2, 9, 102),
            },
        ],
    };
    (
        table,
        snapshot,
        [(1, FIRST), (2, SECOND)].into_iter().collect(),
    )
}

fn wait_for<T>(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    mut poll: impl FnMut(
        &mut ShellComponentTransport,
        &mut ContentEpochRegistry,
    ) -> Result<Option<T>, sophia_runtime::ShellTransportError>,
) -> Result<T, Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = poll(transport, epochs)? {
            return Ok(value);
        }
        if std::time::Instant::now() >= deadline {
            return Err("descriptor response timed out".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn finish_client(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    supervisor: &mut ProcessSupervisor,
) -> Result<(), Box<dyn std::error::Error>> {
    let pid = rustix::process::Pid::from_raw(supervisor.child_id().ok_or("missing child")? as i32)
        .ok_or("invalid child pid")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        // Leave the leader waitable until the supervisor releases its custody.
        if let Some(status) = rustix::process::waitid(
            rustix::process::WaitId::Pid(pid),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )? {
            if status.exit_status() != Some(0) {
                return Err(format!("descriptor client failed: {status:?}").into());
            }
            supervisor.poll()?;
            return Ok(());
        }
        match transport.poll_io(epochs) {
            Ok(()) | Err(sophia_runtime::ShellTransportError::NotConnected) => {}
            Err(error) => return Err(error.into()),
        }
        if std::time::Instant::now() >= deadline {
            return Err("descriptor client did not exit cleanly".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn tab_protocol_proof(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    snapshot: &ShellV1DescriptorSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    use sophia_protocol::*;
    if !transport.supports_tabs() {
        return Err("shell did not negotiate tab descriptors".into());
    }
    let mut descriptors = snapshot.descriptors.clone();
    for d in &mut descriptors {
        d.slot += 100;
        d.action.target_slot = d.slot;
    }
    let mut tabs = ShellTabSnapshot {
        connection_epoch: 1,
        generation: 1,
        groups: vec![ShellTabGroup {
            slot: 1,
            output: OUTPUT,
            focused: true,
            selected_slot: Some(descriptors[0].slot),
            entries: descriptors,
        }],
    };
    for generation in 1..=2 {
        tabs.generation = generation;
        let tx = TransactionId::from_raw(100 + generation);
        transport.publish_tabs(epochs, tx, &tabs)?;
        let (actual, candidate) = wait_for(transport, epochs, |t, e| t.poll_tabs_candidate(e))?;
        if actual != tx
            || candidate.snapshot_generation != generation
            || candidate.groups != vec![1]
        {
            return Err("tab candidate escaped snapshot".into());
        }
        let outcome = |kind, presentation_epoch| ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch,
            kind,
        };
        if generation == 1 {
            transport.send_tabs_outcome(
                epochs,
                tx,
                outcome(ShellV1CandidateOutcomeKind::Superseded, 0),
            )?;
            continue;
        }
        transport.send_tabs_outcome(
            epochs,
            tx,
            outcome(ShellV1CandidateOutcomeKind::Prepared, 0),
        )?;
        transport.send_tabs_outcome(
            epochs,
            tx,
            outcome(ShellV1CandidateOutcomeKind::Presented, 50),
        )?;
        let event = ShellV1Activation {
            connection_epoch: 1,
            candidate_generation: candidate.candidate_generation,
            presentation_epoch: 50,
            activation: 600,
            action: tabs.groups[0].entries[1].action,
        };
        let tx = TransactionId::from_raw(110);
        transport.queue_tab_activation(epochs, tx, event)?;
        let ack = wait_for(transport, epochs, |t, e| t.poll_tab_activation_ack(e, tx))?;
        if ack.connection_epoch != event.connection_epoch
            || ack.activation != event.activation
            || ack.disposition != ShellV1ActivationDisposition::Consumed
        {
            return Err("tab activation rejected".into());
        }
        // The production file owner refuses stale presentation identities before
        // disclosure. Client-side stale rejection belongs to the client reducer tests.
        let stale = ShellV1Activation {
            activation: 601,
            presentation_epoch: 49,
            ..event
        };
        if !matches!(
            transport.queue_tab_activation(epochs, TransactionId::from_raw(111), stale),
            Err(sophia_runtime::ShellTransportError::WrongActivation)
        ) {
            return Err("host admitted stale tab presentation".into());
        }
    }
    println!(
        "sophia_tab_protocol_proof status=complete supersession=true activation=true stale_epoch_rejected_by_host=true wire=9p"
    );
    Ok(())
}

fn reference_protocol_proof(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
) -> Result<(), Box<dyn std::error::Error>> {
    use sophia_protocol::*;
    if !transport.supports_reference() {
        return Err("shell did not negotiate read-only reference sheets".into());
    }
    let catalog = ShellShortcutCatalog {
        connection_epoch: 1,
        generation: 7,
        entries: (1..=256)
            .map(|slot| ShellShortcut {
                slot,
                chord: format!("Super+{slot}"),
                action: format!("policy:action-{slot}"),
                label: None,
                group: None,
            })
            .collect(),
    };
    transport.publish_shortcuts(epochs, TransactionId::from_raw(400), &catalog)?;
    let mut presentation_epoch = 0;
    let mut seen = std::collections::BTreeSet::new();
    for (i, operation) in [
        ShellReferenceOperation::Startup,
        ShellReferenceOperation::Next,
        ShellReferenceOperation::Previous,
        ShellReferenceOperation::Dismiss,
        ShellReferenceOperation::Toggle,
    ]
    .into_iter()
    .enumerate()
    {
        let tx = TransactionId::from_raw(401 + i as u64);
        let request = ShellReferenceRequest {
            connection_epoch: 1,
            catalog_generation: 7,
            request_generation: tx.raw(),
            output: OUTPUT,
            output_generation: 1,
            presentation_epoch,
            operation,
        };
        transport.begin_reference_request(epochs, tx, request)?;
        let sophia_runtime::ShellReferenceCandidateEvent::Candidate(actual, c) =
            wait_for(transport, epochs, |t, e| t.poll_reference_candidate(e))?
        else {
            return Err("reference candidate refused".into());
        };
        if actual != tx
            || c.request_generation != tx.raw()
            || c.entries.len() != 256
            || c.visible != (operation != ShellReferenceOperation::Dismiss)
        {
            return Err("reference candidate differs from requested catalog".into());
        }
        if i == 1 && c.page != 1 || i == 2 && c.page != 0 {
            return Err("reference pagination did not follow presented page".into());
        }
        for row in &c.entries {
            seen.insert(row.slot);
        }
        let outcome = ShellReferenceOutcome {
            connection_epoch: 1,
            catalog_generation: 7,
            request_generation: tx.raw(),
            candidate_generation: c.candidate_generation,
            presentation_epoch: 0,
            page: c.page,
            pages: 5,
            kind: ShellV1CandidateOutcomeKind::Prepared,
        };
        transport.send_reference_outcome(epochs, tx, outcome)?;
        presentation_epoch = 100 + i as u64;
        transport.send_reference_outcome(
            epochs,
            tx,
            ShellReferenceOutcome {
                presentation_epoch,
                kind: ShellV1CandidateOutcomeKind::Presented,
                ..outcome
            },
        )?;
    }
    if seen.len() != 256 {
        return Err("reference omitted a configured shortcut".into());
    }
    println!(
        "sophia_reference_corpus status=complete entries=256 paging=true dismissal=true actions_disclosed=0 wire=9p"
    );
    Ok(())
}

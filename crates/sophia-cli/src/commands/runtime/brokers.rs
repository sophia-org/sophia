use super::*;
mod files;

pub(crate) fn try_run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.iter().any(|arg| arg == "metadata-broker-files-serve") {
        files::run()?;
        return Ok(true);
    }
    if args.iter().any(|arg| arg == "metadata-broker-serve") {
        run_metadata_broker_server()?;
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "runtime-brokers-smoke") {
        let portal = arg_value(args, "--portal").unwrap_or_else(|| "/usr/bin/true".to_owned());
        let metadata = arg_value(args, "--metadata").unwrap_or_else(|| "/usr/bin/true".to_owned());
        let mut supervisors = RuntimeBrokerSupervisors::new(
            ProcessLaunchSpec::new(&portal),
            ProcessLaunchSpec::new(&metadata),
        );
        let report = supervisors.start_placeholders()?;
        let mut portal_exit = report.portal_poll;
        let mut metadata_exit = report.metadata_poll;

        for _ in 0..100 {
            if portal_exit == Some(SupervisorEvent::ProcessExited)
                && metadata_exit == Some(SupervisorEvent::ProcessExited)
            {
                break;
            }
            let (portal_event, metadata_event) = supervisors.poll_all()?;
            portal_exit = portal_exit.or(portal_event);
            metadata_exit = metadata_exit.or(metadata_event);
            std::thread::sleep(Duration::from_millis(1));
        }
        supervisors.terminate_all()?;

        println!(
            "runtime-brokers-smoke portal={} metadata={} portal_start={:?} metadata_start={:?} portal_exit={:?} metadata_exit={:?}",
            portal,
            metadata,
            report.portal_start,
            report.metadata_start,
            portal_exit,
            metadata_exit
        );
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "metadata-broker-transport-smoke")
    {
        return run_metadata_broker_transport_smoke(args);
    }

    if args.iter().any(|arg| arg == "portal-broker-health-smoke") {
        return run_broker_health_smoke(BrokerKind::Portal, args);
    }

    if args.iter().any(|arg| arg == "metadata-broker-health-smoke") {
        return run_broker_health_smoke(BrokerKind::Metadata, args);
    }

    Ok(false)
}

fn run_metadata_broker_transport_smoke(
    args: &[String],
) -> Result<bool, Box<dyn std::error::Error>> {
    use sophia_engine::{ChromeDescriptorTable, MetadataChromeUpdate};
    use sophia_protocol::{
        AttentionState, BrokerV1Rejection, BrokerV1Request, BrokerV1Response, DisplayLabel,
        MetadataDisclosure, NamespaceProfile, ReducedMetadataCandidate, SurfaceId, TransactionId,
    };

    let directory = std::env::temp_dir().join(format!(
        "sophia-metadata-broker-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut transport = sophia_runtime::MetadataBrokerSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .map_err(|error| format!("metadata broker bind failed: {error:?}"))?;
    let socket = transport.socket_path().to_path_buf();
    let executable = std::env::current_exe()?;
    // The broker role admits only a peer carrying protection evidence, so the
    // domain is unconditional here rather than selected by a flag. `--protected`
    // now chooses the in-sandbox negative probe, which is the part that costs
    // host markers and a probe run; the smoke needs bwrap either way.
    let probe = args.iter().any(|arg| arg == "--protected");
    let mut hidden_paths = Vec::new();
    let domain = sophia_runtime::ProtectionDomainSpec::bubblewrap([
        sophia_runtime::ProtectionDomainRole::MetadataBroker,
    ])?
    .path(sophia_runtime::ProtectionPath::read_only(
        socket
            .parent()
            .expect("metadata broker socket has a parent"),
    ))?;
    let mut spec = ProcessLaunchSpec::new(&executable)
        .arg("metadata-broker-serve")
        .env(sophia_runtime::SOPHIA_BROKER_SOCKET_ENV, &socket)
        .process_group()
        .protection_domain(domain);
    if probe {
        let tmp_marker = std::env::temp_dir().join(format!(
            "sophia-protection-host-marker-{}",
            std::process::id()
        ));
        let home_marker = executable.with_file_name(format!(
            ".sophia-protection-host-marker-{}",
            std::process::id()
        ));
        std::fs::write(&tmp_marker, b"host-only")?;
        std::fs::write(&home_marker, b"host-only")?;
        spec = spec
            .env("SOPHIA_PROTECTION_PROBE", "required")
            .env("SOPHIA_PROTECTION_HIDDEN_TMP_PATH", &tmp_marker)
            .env("SOPHIA_PROTECTION_HIDDEN_HOME_PATH", &home_marker);
        hidden_paths.extend([tmp_marker, home_marker]);
    }
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::MetadataBroker, spec);
    supervisor
        .apply(sophia_runtime::SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::MetadataBroker,
            delay: Duration::ZERO,
        })
        .map_err(|error| format!("metadata broker spawn failed: {error:?}"))?;
    let evidence = supervisor
        .protection_evidence()
        .ok_or("metadata broker supervisor omitted its protection domain")?
        .clone();
    let peer_pid = evidence.peer_pid;
    transport
        .authorize_protected_peer(&evidence)
        .map_err(|error| format!("metadata broker authorization failed: {error:?}"))?;
    let welcome = transport
        .accept_and_negotiate(1, Duration::from_secs(5))
        .map_err(|error| format!("metadata broker negotiation failed: {error:?}"))?;

    let surface = SurfaceId::new(6, 1);
    let mut transaction = 1_u64;
    let mut request = |request: BrokerV1Request| {
        let current = TransactionId::from_raw(transaction);
        transaction = transaction.saturating_add(1);
        transport.request(current, &request)
    };
    let admitted = request(BrokerV1Request::SurfaceAdmitted {
        connection_epoch: 1,
        surface,
        profile: NamespaceProfile::Confined,
    })?;
    assert!(matches!(
        admitted,
        BrokerV1Response::PublishRule {
            rule: sophia_protocol::MetadataDisclosureRule {
                disclosure: MetadataDisclosure::None,
                ..
            },
            ..
        }
    ));
    let class_rule = request(BrokerV1Request::SetDisclosure {
        connection_epoch: 1,
        surface,
        disclosure: MetadataDisclosure::ClassOnly,
    })?;
    assert!(matches!(
        class_rule,
        BrokerV1Response::PublishRule {
            rule: sophia_protocol::MetadataDisclosureRule {
                disclosure: MetadataDisclosure::ClassOnly,
                ..
            },
            ..
        }
    ));
    let title_rejected = request(BrokerV1Request::CandidateReduced {
        connection_epoch: 1,
        candidate: ReducedMetadataCandidate {
            surface,
            label: Some(DisplayLabel {
                text: "Quarterly Salary Review.ods".to_owned(),
                redacted: false,
            }),
            disclosure: MetadataDisclosure::Full,
            generation: 1,
        },
    })?;
    assert_eq!(
        title_rejected,
        BrokerV1Response::Rejected {
            connection_epoch: 1,
            rejection: BrokerV1Rejection::DisclosureExceeded,
        }
    );
    let descriptor = request(BrokerV1Request::CandidateReduced {
        connection_epoch: 1,
        candidate: ReducedMetadataCandidate {
            surface,
            label: Some(DisplayLabel {
                text: "LibreOffice".to_owned(),
                redacted: true,
            }),
            disclosure: MetadataDisclosure::ClassOnly,
            generation: 2,
        },
    })?;
    let mut table = ChromeDescriptorTable::default();
    let BrokerV1Response::EmitDescriptor { descriptor, .. } = descriptor else {
        return Err("metadata broker did not emit a sanitized descriptor".into());
    };
    assert_eq!(
        table.apply_metadata(descriptor),
        MetadataChromeUpdate::Upserted { surface }
    );
    assert_eq!(
        table
            .get(surface)
            .and_then(|descriptor| descriptor.label.as_ref())
            .map(|label| (label.text.as_str(), label.redacted)),
        Some(("LibreOffice", true))
    );
    let attention = request(BrokerV1Request::AttentionChanged {
        connection_epoch: 1,
        surface,
        attention: AttentionState::Notice,
    })?;
    assert!(matches!(attention, BrokerV1Response::EmitDescriptor { .. }));
    let retired = request(BrokerV1Request::SurfaceRemoved {
        connection_epoch: 1,
        surface,
    })?;
    assert_eq!(
        retired,
        BrokerV1Response::RetireSurface {
            connection_epoch: 1,
            surface,
        }
    );
    table.remove_surface(surface);
    assert!(table.is_empty());

    transport.disconnect()?;
    supervisor.terminate()?;
    for hidden_path in hidden_paths {
        std::fs::remove_file(hidden_path)?;
    }
    println!(
        "metadata-broker-transport-smoke status=passed protected=true probe={} peer_pid={} revision={} secret_title_rejected=true descriptor_label=LibreOffice descriptor_redacted=true retired=true",
        probe, peer_pid, welcome.selected_revision,
    );
    Ok(true)
}

fn run_metadata_broker_server() -> Result<(), Box<dyn std::error::Error>> {
    use sophia_broker::{MetadataBroker, MetadataBrokerCommand, MetadataBrokerEvent};
    use sophia_protocol::{BrokerV1Rejection, BrokerV1Request, BrokerV1Response, TransactionId};

    run_required_protection_probe()?;
    let socket = std::env::var_os(sophia_runtime::SOPHIA_BROKER_SOCKET_ENV)
        .ok_or("metadata broker requires SOPHIA_BROKER_SOCKET")?;
    let mut transport = sophia_runtime::MetadataBrokerClientTransport::connect(socket)?;
    let connection_epoch = transport.connection_epoch();
    let default_class_only = std::env::var_os("SOPHIA_BROKER_DEFAULT_DISCLOSURE").as_deref()
        == Some(std::ffi::OsStr::new("class-only"));
    let mut broker = MetadataBroker::new();
    loop {
        let (transaction, request) = match transport.receive() {
            Ok(request) => request,
            Err(sophia_runtime::BrokerTransportError::Io(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let result = match request {
            BrokerV1Request::SurfaceAdmitted {
                surface, profile, ..
            } => {
                let admitted =
                    broker.update(MetadataBrokerEvent::SurfaceAdmitted { surface, profile });
                if default_class_only && admitted.is_ok() {
                    broker.set_disclosure(surface, sophia_protocol::MetadataDisclosure::ClassOnly)
                } else {
                    admitted
                }
            }
            BrokerV1Request::CandidateReduced { candidate, .. } => {
                broker.update(MetadataBrokerEvent::CandidateReduced(candidate))
            }
            BrokerV1Request::AttentionChanged {
                surface, attention, ..
            } => broker.update(MetadataBrokerEvent::AttentionChanged { surface, attention }),
            BrokerV1Request::SurfaceRemoved { surface, .. } => {
                broker.update(MetadataBrokerEvent::SurfaceRemoved { surface })
            }
            BrokerV1Request::SetDisclosure {
                surface,
                disclosure,
                ..
            } => broker.set_disclosure(surface, disclosure),
        };
        let response = match result {
            Ok(commands) => one_broker_response(connection_epoch, commands)?,
            Err(rejection) => BrokerV1Response::Rejected {
                connection_epoch,
                rejection: match rejection {
                    sophia_broker::MetadataBrokerRejection::UnknownSurface => {
                        BrokerV1Rejection::UnknownSurface
                    }
                    sophia_broker::MetadataBrokerRejection::StaleGeneration => {
                        BrokerV1Rejection::StaleGeneration
                    }
                    sophia_broker::MetadataBrokerRejection::CapacityExhausted => {
                        BrokerV1Rejection::CapacityExhausted
                    }
                    sophia_broker::MetadataBrokerRejection::DisclosureExceeded => {
                        BrokerV1Rejection::DisclosureExceeded
                    }
                },
            },
        };
        respond_to_broker_request(&mut transport, transaction, response)?;
    }

    fn one_broker_response(
        connection_epoch: u64,
        commands: Vec<MetadataBrokerCommand>,
    ) -> Result<BrokerV1Response, Box<dyn std::error::Error>> {
        let mut commands = commands.into_iter();
        let response = match commands.next() {
            Some(MetadataBrokerCommand::PublishRule(rule)) => BrokerV1Response::PublishRule {
                connection_epoch,
                rule,
            },
            Some(MetadataBrokerCommand::EmitDescriptor { descriptor, action }) => {
                BrokerV1Response::EmitDescriptor {
                    connection_epoch,
                    descriptor,
                    action,
                }
            }
            Some(MetadataBrokerCommand::RetireSurface { surface }) => {
                BrokerV1Response::RetireSurface {
                    connection_epoch,
                    surface,
                }
            }
            None => BrokerV1Response::NoChange { connection_epoch },
        };
        if commands.next().is_some() {
            return Err("one metadata broker request emitted multiple wire commands".into());
        }
        Ok(response)
    }

    fn respond_to_broker_request(
        transport: &mut sophia_runtime::MetadataBrokerClientTransport,
        transaction: TransactionId,
        response: BrokerV1Response,
    ) -> Result<(), Box<dyn std::error::Error>> {
        transport.respond(transaction, &response)?;
        Ok(())
    }
}

fn run_required_protection_probe() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SOPHIA_PROTECTION_PROBE").as_deref()
        != Some(std::ffi::OsStr::new("required"))
    {
        return Ok(());
    }
    for forbidden in ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"] {
        if std::env::var_os(forbidden).is_some() {
            return Err(format!("protection domain retained forbidden {forbidden}").into());
        }
    }
    for (variable, label) in [
        ("SOPHIA_PROTECTION_HIDDEN_TMP_PATH", "host /tmp"),
        ("SOPHIA_PROTECTION_HIDDEN_HOME_PATH", "host home"),
    ] {
        let hidden = std::env::var_os(variable)
            .ok_or_else(|| format!("protection probe omitted its {label} path"))?;
        if std::path::Path::new(&hidden).exists() {
            return Err(format!("protection domain exposed its {label} marker").into());
        }
    }
    let target = "1.1.1.1:53".parse()?;
    if std::net::TcpStream::connect_timeout(&target, Duration::from_millis(100)).is_ok() {
        return Err("protection domain retained outbound network access".into());
    }
    let own_fd_directory = format!("/proc/{}/fd", std::process::id());
    let mut unexpected = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd")? {
        let entry = entry?;
        let Ok(fd) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if fd <= 2 {
            continue;
        }
        let target = std::fs::read_link(entry.path()).ok();
        if target.as_deref() == Some(std::path::Path::new(&own_fd_directory)) {
            continue;
        }
        unexpected.push((fd, target));
    }
    if !unexpected.is_empty() {
        return Err(
            format!("protection domain inherited unexpected descriptors: {unexpected:?}").into(),
        );
    }
    Ok(())
}

fn run_broker_health_smoke(
    broker: BrokerKind,
    _args: &[String],
) -> Result<bool, Box<dyn std::error::Error>> {
    // The metadata broker reports its own state now. The portal broker still has no
    // health source of its own, so it keeps a stated placeholder rather than
    // borrowing the metadata broker's -- a health line that describes a different
    // component is worse than one that admits it is a stand-in.
    let (state, message) = match broker {
        BrokerKind::Metadata => {
            let broker = sophia_broker::MetadataBroker::new();
            (
                BrokerHealthState::Ready,
                Some(format!("metadata broker ready, surfaces={}", broker.len())),
            )
        }
        BrokerKind::Portal => (
            BrokerHealthState::Ready,
            Some("placeholder ready".to_owned()),
        ),
    };
    let packet = BrokerHealthPacket::new(broker, state, 1, message)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let frame = encode_broker_health_frame(&packet)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let decoded = decode_broker_health_frame(&frame)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let message_len = decoded.message.as_deref().map(str::len).unwrap_or(0);
    let mut runtime = SessionRuntimeLoop::default();
    let runtime_report =
        runtime.step_observations([SessionRuntimeObservation::BrokerHealthChanged {
            broker: decoded.broker,
            state: decoded.state,
            generation: decoded.generation,
            status_message_len: message_len,
        }])?;
    let health = match decoded.broker {
        BrokerKind::Portal => runtime.state().portal_broker_health,
        BrokerKind::Metadata => runtime.state().metadata_broker_health,
    };
    let command = runtime_report
        .commands
        .first()
        .copied()
        .unwrap_or(SessionRuntimeCommand::None);

    println!(
        "{}-broker-health-smoke broker={:?} state={:?} generation={} message_len={} frame_bytes={} runtime_health={:?} runtime_command={:?}",
        broker_label(decoded.broker),
        decoded.broker,
        decoded.state,
        decoded.generation,
        message_len,
        frame.len(),
        health,
        command
    );
    Ok(true)
}

fn broker_label(broker: BrokerKind) -> &'static str {
    match broker {
        BrokerKind::Portal => "portal",
        BrokerKind::Metadata => "metadata",
    }
}

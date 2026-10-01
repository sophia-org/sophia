//! Independent C SDK codecs/session against the production descriptor export.
//! Transport owners are real; protection evidence and presentation are driven
//! by this harness. No Engine work-area commit or protected launch is claimed.
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[path = "support/shell_files_oracle/process.rs"]
mod process;

const EPOCH: u64 = 41;
const WAIT: Duration = Duration::from_secs(40);
const PHASES: [&str; 12] = [
    "ready",
    "descriptor-candidate",
    "descriptor-prepared",
    "descriptor-presented",
    "descriptor-ack",
    "tabs-candidate",
    "tabs-done",
    "reference-candidate",
    "reference-done",
    "launcher-candidate",
    "launcher-presented",
    "launcher-ack",
];
fn tx(value: u64) -> TransactionId {
    TransactionId::from_raw(value)
}
fn action(slot: u16) -> ToplevelActionCapabilityRef {
    ToplevelActionCapabilityRef {
        token: u64::from(slot) + 2,
        issuer_epoch: 4,
        issuer_revocation_epoch: 5,
        recipient_epoch: EPOCH,
        target_slot: slot,
        target_generation: 6,
    }
}
fn descriptor(slot: u16) -> ShellV1Descriptor {
    ShellV1Descriptor {
        slot,
        generation: 6,
        label: Some(DisplayLabel {
            text: "Item".into(),
            redacted: false,
        }),
        trust_level: TrustLevel::Trusted,
        attention: AttentionState::None,
        action: action(slot),
    }
}
fn descriptor_activation() -> ShellV1Activation {
    ShellV1Activation {
        connection_epoch: EPOCH,
        candidate_generation: 11,
        presentation_epoch: 100,
        activation: 21,
        action: action(1),
    }
}
fn launcher_activation() -> ShellLauncherActivation {
    ShellLauncherActivation {
        connection_epoch: EPOCH,
        catalog_generation: 40,
        request_generation: 40,
        candidate_generation: 41,
        presentation_epoch: 100,
        activation: 51,
        slot: 2,
    }
}
fn outcome(generation: u64, kind: ShellV1CandidateOutcomeKind) -> ShellV1CandidateOutcome {
    ShellV1CandidateOutcome {
        connection_epoch: EPOCH,
        candidate_generation: generation,
        presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
            100
        } else {
            0
        },
        kind,
    }
}
struct Fixture {
    transport: ShellComponentTransport,
    epochs: ContentEpochRegistry,
    negotiated: bool,
    phase: usize,
    combined: bool,
}
impl Fixture {
    fn new(path: &Path, pid: u32, combined: bool) -> Self {
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            path,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: pid,
                peer_pid: pid,
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        let policy = if combined {
            transport
                .reserve_content(
                    &mut epochs,
                    ContentLimits::prototype(ContentGrant {
                        connection_epoch: EPOCH,
                        content_grant_epoch: 7,
                    }),
                )
                .unwrap();
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            }
        } else {
            ShellContentAdmissionPolicy::Unavailable
        };
        transport
            .begin_descriptor_file_negotiation(&epochs, EPOCH, WAIT, policy)
            .unwrap();
        Self {
            transport,
            epochs,
            negotiated: false,
            phase: 0,
            combined,
        }
    }
    fn tick(&mut self) {
        if self.negotiated {
            match self.transport.poll_io(&mut self.epochs) {
                Ok(()) => {}
                Err(ShellTransportError::NotConnected) if self.phase == PHASES.len() + 1 => {}
                Err(error) => panic!("descriptor C phase {}: {error:?}", self.phase),
            }
        } else if let Some(welcome) = self
            .transport
            .poll_negotiation(&mut self.epochs, 65536)
            .unwrap()
        {
            assert_eq!(welcome.connection_epoch, EPOCH);
            assert_eq!(welcome.selected_revision, 8);
            assert_eq!(
                welcome.capabilities,
                if self.combined { 2047 } else { 1663 }
            );
            assert_eq!(self.transport.content_limits().is_some(), self.combined);
            self.negotiated = true;
        }
    }
    fn phase(&mut self, name: &str) -> bool {
        assert!(self.negotiated);
        assert_eq!(name, PHASES.get(self.phase).copied().unwrap_or("done"));
        use ShellV1CandidateOutcomeKind::{Prepared, Presented};
        match name {
            "ready" => {
                self.transport
                    .begin_candidate_request(
                        &mut self.epochs,
                        tx(10),
                        &ShellV1DescriptorSnapshot {
                            connection_epoch: EPOCH,
                            snapshot_generation: 10,
                            output: OutputId::from_raw(8),
                            output_generation: 9,
                            broker_epoch: 4,
                            broker_revocation_epoch: 5,
                            descriptors: (1..=16).map(descriptor).collect(),
                        },
                    )
                    .unwrap();
                assert_eq!(
                    self.transport
                        .content_accounting(&self.epochs)
                        .response_records,
                    2
                );
            }
            "descriptor-candidate" => {
                let Some(candidate) = self.transport.poll_candidate(&mut self.epochs).unwrap()
                else {
                    return false;
                };
                assert_eq!(
                    candidate,
                    ShellV1Candidate {
                        connection_epoch: EPOCH,
                        snapshot_generation: 10,
                        candidate_generation: 11,
                        output: OutputId::from_raw(8),
                        visible: true,
                        selected_slot: Some(1),
                        reservation: Some(ShellV1WorkAreaReservation {
                            edge: ShellV1ReservationEdge::Top,
                            thickness_px: 24
                        }),
                        entries: (1..=16)
                            .map(|slot| ShellV1CandidateEntry {
                                slot,
                                generation: 6
                            })
                            .collect(),
                    }
                );
                self.refuse_early_activation();
                self.transport
                    .send_candidate_outcome(&mut self.epochs, tx(10), outcome(11, Prepared))
                    .unwrap();
            }
            "descriptor-prepared" => {
                assert_eq!(
                    self.transport
                        .content_accounting(&self.epochs)
                        .response_records,
                    1
                );
                self.refuse_early_activation();
                self.transport
                    .send_candidate_outcome(&mut self.epochs, tx(10), outcome(11, Presented))
                    .unwrap();
            }
            "descriptor-presented" => {
                assert_eq!(
                    self.transport
                        .content_accounting(&self.epochs)
                        .response_records,
                    0
                );
                self.transport
                    .queue_activation(&mut self.epochs, tx(20), descriptor_activation())
                    .unwrap();
            }
            "descriptor-ack" => {
                let Some(ack) = self
                    .transport
                    .poll_activation_ack(&mut self.epochs)
                    .unwrap()
                else {
                    return false;
                };
                assert_eq!(
                    ack,
                    ShellV1ActivationAck {
                        connection_epoch: EPOCH,
                        activation: 21,
                        disposition: ShellV1ActivationDisposition::Consumed
                    }
                );
                assert_eq!(self.transport.descriptor_unmatched_acks(), 1);
                self.transport
                    .publish_tabs(
                        &mut self.epochs,
                        tx(25),
                        &ShellTabSnapshot {
                            connection_epoch: EPOCH,
                            generation: 25,
                            groups: (1..=1024_u16)
                                .map(|slot| ShellTabGroup {
                                    slot: u64::from(slot),
                                    output: OutputId::from_raw(8),
                                    focused: slot == 1,
                                    selected_slot: Some(slot * 2 - 1),
                                    entries: vec![descriptor(slot * 2 - 1), descriptor(slot * 2)],
                                })
                                .collect(),
                        },
                    )
                    .unwrap();
            }
            "tabs-candidate" => {
                let Some((transaction, candidate)) = self
                    .transport
                    .poll_tabs_candidate(&mut self.epochs)
                    .unwrap()
                else {
                    return false;
                };
                assert_eq!(transaction, tx(25));
                assert_eq!(
                    candidate,
                    ShellTabCandidate {
                        connection_epoch: EPOCH,
                        snapshot_generation: 25,
                        candidate_generation: 26,
                        groups: (1..=1024).collect()
                    }
                );
                for kind in [Prepared, Presented] {
                    self.transport
                        .send_tabs_outcome(&mut self.epochs, tx(25), outcome(26, kind))
                        .unwrap();
                }
            }
            "tabs-done" => {
                self.transport
                    .publish_shortcuts(
                        &mut self.epochs,
                        tx(30),
                        &ShellShortcutCatalog {
                            connection_epoch: EPOCH,
                            generation: 30,
                            entries: (1..=256)
                                .map(|slot| ShellShortcut {
                                    slot,
                                    chord: "Super+q".into(),
                                    action: "session:quit".into(),
                                    label: Some("Action".into()),
                                    group: None,
                                })
                                .collect(),
                        },
                    )
                    .unwrap();
                self.transport
                    .begin_reference_request(
                        &mut self.epochs,
                        tx(30),
                        ShellReferenceRequest {
                            connection_epoch: EPOCH,
                            catalog_generation: 30,
                            request_generation: 30,
                            output: OutputId::from_raw(8),
                            output_generation: 9,
                            presentation_epoch: 0,
                            operation: ShellReferenceOperation::Toggle,
                        },
                    )
                    .unwrap();
            }
            "reference-candidate" => {
                let Some(event) = self
                    .transport
                    .poll_reference_candidate(&mut self.epochs)
                    .unwrap()
                else {
                    return false;
                };
                let ShellReferenceCandidateEvent::Candidate(transaction, candidate) = event else {
                    panic!("reference refused")
                };
                assert_eq!(transaction, tx(30));
                assert_eq!(
                    candidate,
                    ShellReferenceCandidate {
                        connection_epoch: EPOCH,
                        catalog_generation: 30,
                        request_generation: 30,
                        candidate_generation: 31,
                        output: OutputId::from_raw(8),
                        visible: true,
                        page: 0,
                        style: ShellReferenceStyle {
                            body_size: 12,
                            title_size: 16,
                            padding: 4,
                            row_gap: 2,
                            key_gap: 4,
                            column_gap: 4,
                            border: 1,
                            margin: 4,
                            columns: 1,
                            colors: [0xff000000; 6],
                            title: "Shortcuts".into()
                        },
                        entries: (1..=256)
                            .map(|slot| ShellReferenceEntry {
                                slot,
                                key: "Super+q".into(),
                                label: "Action".into()
                            })
                            .collect(),
                    }
                );
                for kind in [Prepared, Presented] {
                    self.transport
                        .send_reference_outcome(
                            &mut self.epochs,
                            tx(30),
                            ShellReferenceOutcome {
                                connection_epoch: EPOCH,
                                catalog_generation: 30,
                                request_generation: 30,
                                candidate_generation: 31,
                                presentation_epoch: if kind == Presented { 100 } else { 0 },
                                page: 0,
                                pages: 1,
                                kind,
                            },
                        )
                        .unwrap();
                }
            }
            "reference-done" => {
                self.transport
                    .publish_launcher_catalog(
                        &self.epochs,
                        tx(40),
                        &ShellApplicationCatalog {
                            connection_epoch: EPOCH,
                            generation: 40,
                            entries: (1..=32)
                                .map(|slot| ShellApplicationDescriptor {
                                    slot,
                                    available: true,
                                    label: "Editor".into(),
                                    keywords: "text".into(),
                                })
                                .collect(),
                        },
                    )
                    .unwrap();
                self.transport
                    .begin_launcher_request(
                        &mut self.epochs,
                        tx(40),
                        &ShellLauncherRequest {
                            connection_epoch: EPOCH,
                            catalog_generation: 40,
                            request_generation: 40,
                            output: OutputId::from_raw(8),
                            output_generation: 9,
                            presentation_epoch: 0,
                            operation: ShellLauncherOperation::Open,
                            query: String::new(),
                        },
                    )
                    .unwrap();
            }
            "launcher-candidate" => {
                let Some(event) = self
                    .transport
                    .poll_launcher_candidate(&mut self.epochs)
                    .unwrap()
                else {
                    return false;
                };
                let ShellLauncherCandidateEvent::Candidate(transaction, candidate) = event else {
                    panic!("launcher refused")
                };
                assert_eq!(transaction, tx(40));
                assert_eq!(
                    candidate,
                    ShellLauncherCandidate {
                        connection_epoch: EPOCH,
                        catalog_generation: 40,
                        request_generation: 40,
                        candidate_generation: 41,
                        output: OutputId::from_raw(8),
                        visible: true,
                        selected: 2,
                        entries: (1..=32).collect(),
                        font_size: 12,
                        colors: [0xff000000; 4]
                    }
                );
                for kind in [Prepared, Presented] {
                    self.transport
                        .send_launcher_outcome(
                            &mut self.epochs,
                            tx(40),
                            ShellLauncherOutcome {
                                connection_epoch: EPOCH,
                                request_generation: 40,
                                candidate_generation: 41,
                                presentation_epoch: if kind == Presented { 100 } else { 0 },
                                kind,
                            },
                        )
                        .unwrap();
                }
            }
            "launcher-presented" => {
                self.transport
                    .queue_launcher_activation(&mut self.epochs, tx(50), launcher_activation())
                    .unwrap();
            }
            "launcher-ack" => {
                let Some(ack) = self
                    .transport
                    .poll_launcher_activation_ack(&mut self.epochs)
                    .unwrap()
                else {
                    return false;
                };
                assert_eq!(
                    ack,
                    (
                        tx(50),
                        ShellLauncherActivationAck {
                            activation: launcher_activation(),
                            consumed: true
                        }
                    )
                );
                self.transport
                    .send_launch_outcome(
                        &mut self.epochs,
                        tx(50),
                        ShellLaunchOutcome {
                            activation: launcher_activation(),
                            status: ShellLaunchStatus::Started,
                        },
                    )
                    .unwrap();
            }
            "done" => {
                assert_eq!(
                    self.transport
                        .content_accounting(&self.epochs)
                        .response_records,
                    0
                );
            }
            _ => panic!("unexpected phase {name}"),
        }
        self.phase += 1;
        true
    }
    fn refuse_early_activation(&mut self) {
        assert_eq!(
            self.transport
                .queue_activation(&mut self.epochs, tx(20), descriptor_activation()),
            Err(ShellTransportError::WrongActivation)
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.transport.disconnect(&mut self.epochs).unwrap();
        assert!(
            self.transport
                .collect_content_accounting(&mut self.epochs)
                .quiescent()
        );
    }
}
fn compile(repo: &Path, root: &Path) -> PathBuf {
    let source = repo.join("vendor/c-desktop-sdk/source/src");
    let output = root.join("descriptor-peer");
    let mut command = Command::new("timeout");
    command
        .args(["-s", "KILL", "90"])
        .arg(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-O1",
            "-UNDEBUG",
            "-I",
        ])
        .arg(&source);
    for directory in ["nine_p", "shell_files", "shell_session"] {
        let mut files = std::fs::read_dir(source.join(directory))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
            .collect::<Vec<_>>();
        files.sort();
        command.args(files);
    }
    let result = command
        .arg(repo.join("crates/sophia-runtime/tests/support/shell_descriptor_c_peer.c"))
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    output
}
#[test]
fn independent_c_session_exchanges_descriptor_families_with_the_production_export() {
    let path = std::env::temp_dir().join(format!("descriptor-c-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let scratch = process::Scratch(path);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = compile(&repo, &scratch.0);
    for (combined, name) in [(false, "metadata"), (true, "combined")] {
        let root = scratch.0.join(name);
        std::fs::create_dir(&root).unwrap();
        let socket_dir = root.join("shell");
        let control_path = root.join("control.sock");
        let listener = UnixListener::bind(&control_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stdout = root.join("stdout");
        let stderr = root.join("stderr");
        let mut child = process::ChildGuard(
            Command::new(&binary)
                .arg(socket_dir.join("shell.sock"))
                .arg(&control_path)
                .arg(name)
                .stdin(Stdio::piped())
                .stdout(Stdio::from(std::fs::File::create(&stdout).unwrap()))
                .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()))
                .spawn()
                .unwrap(),
        );
        let mut fixture = Fixture::new(&socket_dir, child.0.id(), combined);
        assert_eq!(
            fixture.transport.socket_path(),
            socket_dir.join("shell.sock")
        );
        child.0.stdin.take().unwrap().write_all(b"G").unwrap();
        let mut control: Option<UnixStream> = None;
        let mut bytes = Vec::new();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                start.elapsed() < WAIT,
                "C descriptor deadline, phase {}: {}",
                fixture.phase,
                std::fs::read_to_string(&stderr).unwrap()
            );
            fixture.tick();
            if control.is_none() {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(true).unwrap();
                        control = Some(stream);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("{error}"),
                }
            }
            if let Some(stream) = control.as_mut() {
                if bytes.last() != Some(&b'\n') {
                    let mut read = [0; 128];
                    match stream.read(&mut read) {
                        Ok(0) if fixture.phase == PHASES.len() + 1 => {}
                        Ok(0) => panic!(
                            "C control disconnected: {}",
                            std::fs::read_to_string(&stderr).unwrap()
                        ),
                        Ok(count) => bytes.extend_from_slice(&read[..count]),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(error) => panic!("{error}"),
                    }
                    assert!(bytes.len() <= 128);
                }
                if bytes.last() == Some(&b'\n')
                    && fixture.phase(std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap())
                {
                    stream.write_all(b"G").unwrap();
                    bytes.clear();
                }
            }
            for log in [&stdout, &stderr] {
                assert!(std::fs::metadata(log).unwrap().len() < 65536);
            }
            std::thread::sleep(Duration::from_micros(100));
        };
        assert!(
            status.success(),
            "{name}, phase {}: {}",
            fixture.phase,
            std::fs::read_to_string(&stderr).unwrap()
        );
        assert_eq!(fixture.phase, PHASES.len() + 1);
        assert_eq!(
            std::fs::read_to_string(stdout).unwrap(),
            format!(
                "descriptor_c status=pass profile={name} snapshots=4 candidates=4 activations=2\n"
            )
        );
        assert!(std::fs::read(stderr).unwrap().is_empty());
    }
}

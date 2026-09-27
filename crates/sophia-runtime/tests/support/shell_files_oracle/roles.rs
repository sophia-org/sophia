//! Real transport/content owners; catalog/indicator admission is a scripted
//! Session boundary. This harness does not test Session launch policy.
use sophia_protocol::*;
use sophia_runtime::*;
use std::path::Path;
use std::time::Duration;

#[path = "role_service.rs"]
mod service;
#[path = "role_values.rs"]
mod values;

pub const NAMES: [&str; 5] = [
    "bar",
    "launcher",
    "dock",
    "launcher-permit-revoked",
    "dock-permit-revoked",
];
pub struct Fixture {
    pub name: &'static str,
    transport: ShellComponentTransport,
    registry: ContentEpochRegistry,
    grant: ContentGrant,
    native: bool,
    bar: bool,
    state: u8,
    catalog_generation: u64,
    indicator_generation: u64,
    permit: u64,
    hold_candidates: bool,
    hold_activation: bool,
    prepared: Option<u64>,
    presented: u64,
    event_id: u64,
    last_action: Option<ContentAction>,
    input_acks: usize,
    admissions: usize,
    indicator_watermark: u64,
    revoked: bool,
}
fn tx(n: u64) -> TransactionId {
    TransactionId::from_raw(n)
}
fn output() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 1,
    }
}
impl Fixture {
    pub fn new(root: &Path, name: &'static str, index: usize) -> Self {
        let transport = ShellComponentTransport::bind_for_supervised_uid(
            root.join(name),
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        std::fs::write(
            root.join(format!("{name}.endpoint")),
            transport.socket_path().as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        Self {
            name,
            transport,
            registry: ContentEpochRegistry::new(64 * 1024 * 1024).unwrap(),
            grant: ContentGrant {
                connection_epoch: 40 + index as u64,
                content_grant_epoch: 1,
            },
            native: name.starts_with("launcher"),
            bar: name == "bar",
            state: 0,
            catalog_generation: 3,
            indicator_generation: 3,
            permit: 0,
            hold_candidates: false,
            hold_activation: false,
            prepared: None,
            presented: 0,
            event_id: 10,
            last_action: None,
            input_acks: 0,
            admissions: 0,
            indicator_watermark: 0,
            revoked: false,
        }
    }
    pub fn authorize(&mut self, pid: u32) {
        self.transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: pid,
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        let profile = if self.native {
            ContentStoreProfile::NativeLauncher
        } else if self.bar {
            ContentStoreProfile::Legacy
        } else {
            ContentStoreProfile::PersistentCatalog
        };
        self.transport
            .reserve_content_with_profile(
                &mut self.registry,
                ContentLimits::prototype(self.grant),
                profile,
            )
            .unwrap();
    }
    pub fn phase(&mut self, phase: &str) -> Result<bool, String> {
        self.phase_inner(phase)
            .map_err(|e| format!("{} phase {phase}: {e}", self.name))
    }
    fn phase_inner(&mut self, phase: &str) -> Result<bool, ShellTransportError> {
        match phase {
            "start" => {
                assert_eq!(self.state, 0);
                self.transport.begin_file_negotiation(
                    &self.registry,
                    self.grant.connection_epoch,
                    Duration::from_secs(5),
                    ShellContentAdmissionPolicy::Granted {
                        discrete_input: !self.bar,
                    },
                )?;
                self.state = 1;
            }
            "publish" | "republish" | "same-generation" | "maximum" | "compact" => {
                if self.bar {
                    self.indicator_generation = if phase == "republish" { 4 } else { 3 };
                    let snapshot = self.indicators(phase == "same-generation");
                    self.transport.publish_indicators(
                        &self.registry,
                        tx(20 + self.indicator_generation),
                        &snapshot,
                    )?;
                } else {
                    self.catalog_generation = match phase {
                        "republish" => 4,
                        "maximum" => 5,
                        "compact" => 6,
                        _ => 3,
                    };
                    let catalog = self.persistent_catalog(phase == "maximum");
                    self.transport.publish_catalog(
                        &self.registry,
                        tx(20 + self.catalog_generation),
                        &catalog,
                    )?;
                }
            }
            "opening" => {
                let opening = self.opening();
                self.transport
                    .publish_native_launcher_opening(&self.registry, tx(30), opening)?;
            }
            "outputs" => {
                self.transport.publish_content_output_facts(
                    &mut self.registry,
                    tx(31),
                    3,
                    vec![ContentOutputFactsEntry {
                        output: output(),
                        local_width: 128,
                        local_height: 64,
                        scale_numerator: 1,
                        scale_denominator: 1,
                        scale_generation: 5,
                    }],
                )?;
            }
            "hold-candidates" => self.hold_candidates = true,
            "release-candidates" => self.hold_candidates = false,
            "owner-drained" => {
                self.service()?;
            }
            "hold-activation" => self.hold_activation = true,
            "release-activation" => self.hold_activation = false,
            "present" => {
                let Some(generation) = self.prepared.take() else {
                    return Ok(false);
                };
                self.transport.content_presented(
                    &mut self.registry,
                    self.grant,
                    output(),
                    generation,
                    100 + generation,
                    7,
                    8,
                )?;
                self.presented = generation;
                if self.native {
                    self.transport
                        .install_native_launcher_focus(&mut self.registry, tx(300 + generation))?;
                }
            }
            "text" | "accept" => {
                let focus = self
                    .transport
                    .native_launcher_focus()
                    .expect("presented focus");
                let (kind, text) = if phase == "text" {
                    (NativeLauncherInputKind::Text, "a")
                } else {
                    (NativeLauncherInputKind::Accept, "")
                };
                self.transport.issue_native_launcher_input(
                    &mut self.registry,
                    focus,
                    tx(400 + self.input_acks as u64),
                    kind,
                    text,
                    20_000,
                )?;
            }
            "ack-consumed" => return Ok(self.input_acks >= 1),
            "stale-ack-consumed" => return Ok(self.input_acks >= 3),
            "close" => {
                let opening = self.opening();
                self.transport.close_native_launcher(
                    &mut self.registry,
                    opening,
                    tx(500),
                    ContentReason::Revoked,
                )?;
            }
            "action" => {
                assert!(self.presented > 0);
                self.event_id += 1;
                let action = ContentAction {
                    grant: self.grant,
                    output: output(),
                    candidate_generation: self.presented,
                    presentation_epoch: 100 + self.presented,
                    interaction_generation: 4,
                    allocation: ContentAllocationId {
                        id: 1,
                        generation: 1,
                    },
                    target_id: 1,
                    target_generation: 1,
                    action_id: 1,
                    event_id: self.event_id,
                    kind: 1,
                    reason: 0,
                };
                self.transport.send_content_action(
                    &mut self.registry,
                    tx(600 + self.event_id),
                    &action,
                )?;
                self.last_action = Some(action);
            }
            "one-admission" => assert_eq!(self.admissions, 1),
            _ => panic!("unknown role phase {phase}"),
        }
        Ok(true)
    }
    pub fn tick(&mut self) -> Result<(), String> {
        if let Err(error) = self.service() {
            let fatal = self.name.ends_with("permit-revoked")
                && matches!(
                    error,
                    ShellTransportError::ContentCandidate(ContentCandidateError::Stale)
                );
            if !fatal && !matches!(error, ShellTransportError::NotConnected) {
                self.transport.disconnect(&mut self.registry).unwrap();
                self.state = 3;
                return Err(format!("{} owner error: {error:?}", self.name));
            }
            self.revoked = fatal;
            self.transport.disconnect(&mut self.registry).unwrap();
            self.state = 3;
        }
        Ok(())
    }
    pub fn cleanup(&mut self) {
        self.transport.disconnect(&mut self.registry).unwrap();
        self.registry.collect();
        assert!(
            self.transport
                .content_accounting(&self.registry)
                .quiescent(),
            "{} retained content",
            self.name
        );
    }
    pub fn assert_finished(&self) {
        if self.name.ends_with("-small") {
            assert_eq!(
                self.admissions, 0,
                "small-buffer fixture admitted an action"
            );
            return;
        }
        if self.name.ends_with("permit-revoked") {
            assert!(self.revoked, "{} not revoked", self.name)
        } else {
            assert_eq!(self.admissions, 1, "{} admission count", self.name)
        }
    }
}

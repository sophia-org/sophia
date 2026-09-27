use sophia_protocol::*;
use sophia_runtime::*;
use std::path::Path;
use std::time::Duration;

pub const NAMES: [&str; 8] = [
    "main",
    "refused",
    "unservable",
    "custody",
    "malformed",
    "stream",
    "missing",
    "cancelled",
];
pub struct Fixture {
    pub name: &'static str,
    pub transport: ShellComponentTransport,
    registry: ContentEpochRegistry,
    grant: ContentGrant,
    state: u8,
    generation: u64,
    permit: u64,
    hold_candidates: bool,
    pub action_acked: bool,
    pub presented: bool,
    pub revoked: bool,
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
        let registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        Self {
            name,
            transport,
            registry,
            grant: ContentGrant {
                connection_epoch: 17 + index as u64,
                content_grant_epoch: 1,
            },
            state: 0,
            generation: 0,
            permit: 0,
            hold_candidates: false,
            action_acked: false,
            presented: false,
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
        self.transport
            .reserve_content(&mut self.registry, ContentLimits::prototype(self.grant))
            .unwrap();
    }
    pub fn phase(&mut self, phase: &str) -> Result<bool, String> {
        match phase {
            "start" => {
                if self.state != 0 {
                    return Err("duplicate start".into());
                }
                let policy = if self.name == "refused" {
                    ShellContentAdmissionPolicy::Denied
                } else {
                    ShellContentAdmissionPolicy::Granted {
                        discrete_input: true,
                    }
                };
                self.transport
                    .begin_file_negotiation(
                        &self.registry,
                        self.grant.connection_epoch,
                        Duration::from_secs(5),
                        policy,
                    )
                    .map_err(|e| e.to_string())?;
                self.state = 1;
            }
            "publish" | "republish" => {
                self.generation = if phase == "publish" { 3 } else { 4 };
                self.transport
                    .publish_content_output_facts(
                        &mut self.registry,
                        TransactionId::from_raw(5),
                        self.generation,
                        vec![ContentOutputFactsEntry {
                            output: output(),
                            local_width: if self.generation == 3 { 64 } else { 128 },
                            local_height: 64,
                            scale_numerator: 1,
                            scale_denominator: 1,
                            scale_generation: 5,
                        }],
                    )
                    .map_err(|e| e.to_string())?;
            }
            "hold-candidates" => self.hold_candidates = true,
            "pressure" => {
                self.generation += 1;
                let generation = self.generation + 10;
                self.transport
                    .publish_content_output_facts(
                        &mut self.registry,
                        TransactionId::from_raw(generation),
                        generation,
                        vec![ContentOutputFactsEntry {
                            output: output(),
                            local_width: 64,
                            local_height: 64,
                            scale_numerator: 1,
                            scale_denominator: 1,
                            scale_generation: 5,
                        }],
                    )
                    .map_err(|e| e.to_string())?;
            }
            "release-candidates" => self.hold_candidates = false,
            "action" => {
                if !self.presented {
                    return Ok(false);
                }
                self.transport
                    .send_content_action(
                        &mut self.registry,
                        TransactionId::from_raw(150),
                        &ContentAction {
                            grant: self.grant,
                            output: output(),
                            candidate_generation: 1,
                            presentation_epoch: 9,
                            interaction_generation: 4,
                            allocation: ContentAllocationId {
                                id: 1,
                                generation: 1,
                            },
                            target_id: 1,
                            target_generation: 1,
                            action_id: 1,
                            event_id: 11,
                            kind: 1,
                            reason: 0,
                        },
                    )
                    .map_err(|e| e.to_string())?;
            }
            "action-acked" => return Ok(self.action_acked),
            _ => return Err(format!("unknown phase {phase}")),
        }
        Ok(true)
    }
    fn service(&mut self) -> Result<(), ShellTransportError> {
        if self.state == 0 || self.state == 3 {
            return Ok(());
        }
        if self.state == 1 {
            if self
                .transport
                .poll_negotiation(&mut self.registry, 65536)?
                .is_some()
            {
                self.state = 2;
            }
            return Ok(());
        }
        // A deterministic owner clock avoids making scheduler delay a permit
        // expiry. Transport read/ack deadlines still run on real time.
        self.transport
            .service_content_allocation_requests(&mut self.registry, &[], 10)?;
        while let Some((_, request)) = self
            .transport
            .next_content_allocation_request(&self.registry)
        {
            if request.output != output() {
                self.transport.reject_content_allocation(
                    &mut self.registry,
                    request.allocation_request_id,
                    ContentAllocationError::OutputLost,
                )?;
            } else {
                self.transport.grant_content_allocation(
                    &mut self.registry,
                    request.allocation_request_id,
                    ContentAllocationSnapshot {
                        native_opening: None,
                        output: output(),
                        allocation: ContentAllocationId {
                            id: 1,
                            generation: 1,
                        },
                        scale_generation: 5,
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
            }
        }
        self.transport
            .service_content_resources(&mut self.registry, 10)?;
        let allocations = self.transport.content_allocation_snapshots(&self.registry);
        self.transport
            .service_content_demands(&mut self.registry, &[output()], &allocations)?;
        while let Some((tx, demand)) = self.transport.next_content_demand(&self.registry) {
            self.permit += 1;
            self.transport.grant_content_demand(
                &mut self.registry,
                tx,
                demand.output,
                self.permit,
                10,
            )?;
        }
        if !self.hold_candidates {
            let context = ContentCandidateContext {
                output: output(),
                facts_generation: self.generation,
                interaction_generation: 4,
                allocations: &allocations,
            };
            self.transport
                .service_content_candidates(&mut self.registry, &[context], 10)?;
            if let Some((output, generation)) =
                self.transport.next_content_submission(&self.registry)
            {
                let render = self.transport.begin_content_submission(
                    &mut self.registry,
                    output,
                    generation,
                    10,
                )?;
                assert_eq!(
                    render
                        .resource(ContentResourceId {
                            id: 5,
                            generation: 1
                        })
                        .unwrap()
                        .bytes(),
                    &[0; 8]
                );
                self.transport.content_prepared(
                    &mut self.registry,
                    self.grant,
                    output,
                    generation,
                    7,
                    8,
                    10,
                )?;
                self.transport.content_presented(
                    &mut self.registry,
                    self.grant,
                    output,
                    generation,
                    9,
                    7,
                    8,
                )?;
                drop(render);
                self.presented = true;
            }
        }
        if let Some((tx, ack)) = self.transport.poll_content_action_ack(&mut self.registry)? {
            assert_eq!(tx, TransactionId::from_raw(151));
            assert_eq!(
                ack,
                ContentActionAck {
                    grant: self.grant,
                    output: output(),
                    candidate_generation: 1,
                    presentation_epoch: 9,
                    interaction_generation: 4,
                    allocation: ContentAllocationId {
                        id: 1,
                        generation: 1
                    },
                    target_id: 1,
                    target_generation: 1,
                    action_id: 1,
                    event_id: 11,
                    disposition: 1
                }
            );
            self.transport.retain_content_action_reservations(|_| false);
            self.action_acked = true;
        }
        Ok(())
    }
    pub fn tick(&mut self) {
        if let Err(error) = self.service() {
            let expected = match self.name {
                "refused" => matches!(error, ShellTransportError::ContentAdmissionRefused(_)),
                "unservable" => matches!(error, ShellTransportError::MissingCapability),
                "missing" | "cancelled" => matches!(
                    error,
                    ShellTransportError::ContentCandidate(ContentCandidateError::Stale)
                ),
                _ => false,
            };
            if !expected && !matches!(error, ShellTransportError::NotConnected) {
                panic!("{} owner error: {error}", self.name);
            }
            self.revoked = expected;
            self.transport.disconnect(&mut self.registry).unwrap();
            self.state = 3;
        }
    }
    pub fn cleanup(&mut self) {
        self.transport.disconnect(&mut self.registry).unwrap();
        self.registry.collect();
        assert!(
            self.transport
                .content_accounting(&self.registry)
                .quiescent(),
            "{} retained content after cleanup",
            self.name
        );
    }
}

//! Real transport, content and native launcher owners for the C SDK's live
//! native session peer. Session decisions (publication, presentation, input
//! issue, activation admission, close, pixel removal) are scripted here: this
//! fixture makes no launch-policy or physical-renderer claim. Owner contexts
//! use the production values the contract states: catalog generation 1 and
//! interaction generation 1.
//!
//! Timing boundary: content owners run on a frozen clock (NOW) and input is
//! issued at a fixed fixture timestamp (ISSUED), not the peer's
//! CLOCK_MONOTONIC. This intentionally avoids timing coverage: no input-ack,
//! action-ack, permit, allocation or presentation expiry is exercised, and no
//! production timing claim is made.
use sophia_protocol::*;
use sophia_runtime::*;
use std::path::Path;
use std::time::Duration;

const NOW: u64 = 10;
const ISSUED: u64 = 20_000;
const FACTS: u64 = 3;
const CATALOG: u64 = 1;
const INTERACTION: u64 = 1;

pub struct Fixture {
    transport: ShellComponentTransport,
    registry: ContentEpochRegistry,
    grant: ContentGrant,
    state: u8,
    permit: u64,
    prepared: Option<u64>,
    input_acks: usize,
    unmatched_acks: usize,
    activations: usize,
    admissions: usize,
    pointer: usize,
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
fn allocation_id() -> ContentAllocationId {
    ContentAllocationId {
        id: 1,
        generation: 1,
    }
}
impl Fixture {
    pub fn new(root: &Path) -> Self {
        Self {
            transport: ShellComponentTransport::bind_for_supervised_uid(
                root.join("export"),
                rustix::process::geteuid().as_raw(),
            )
            .unwrap(),
            registry: ContentEpochRegistry::new(64 * 1024 * 1024).unwrap(),
            grant: ContentGrant {
                connection_epoch: 60,
                content_grant_epoch: 1,
            },
            state: 0,
            permit: 0,
            prepared: None,
            input_acks: 0,
            unmatched_acks: 0,
            activations: 0,
            admissions: 0,
            pointer: 0,
        }
    }
    pub fn socket_path(&self) -> &Path {
        self.transport.socket_path()
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
            .reserve_content_with_profile(
                &mut self.registry,
                ContentLimits::prototype(self.grant),
                ContentStoreProfile::NativeLauncher,
            )
            .unwrap();
    }
    fn opening(&self) -> NativeLauncherOpening {
        NativeLauncherOpening {
            grant: self.grant,
            opening: 7,
            output: output(),
            catalog_generation: CATALOG,
            state_revision: 1,
        }
    }
    fn catalog(&self) -> ShellPersistentCatalog {
        ShellPersistentCatalog {
            catalog: ShellApplicationCatalog {
                connection_epoch: self.grant.connection_epoch,
                generation: CATALOG,
                entries: (1..=3)
                    .map(|slot| ShellApplicationDescriptor {
                        slot,
                        available: slot != 3,
                        label: format!("app{slot}"),
                        keywords: String::new(),
                    })
                    .collect(),
            },
            identities: Default::default(),
        }
    }
    fn allocation(&self) -> ContentAllocationSnapshot {
        ContentAllocationSnapshot {
            native_opening: Some(7),
            output: output(),
            allocation: allocation_id(),
            scale_generation: 5,
            scale_numerator: 1,
            scale_denominator: 1,
            role: 3,
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
            allowed_reservation_extent: 0,
        }
    }
    pub fn phase(&mut self, phase: &str) -> Result<bool, String> {
        self.phase_inner(phase)
            .map_err(|e| format!("native phase {phase}: {e}"))
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
                        discrete_input: true,
                    },
                )?;
                self.state = 1;
            }
            "publish" => {
                let catalog = self.catalog();
                self.transport
                    .publish_catalog(&self.registry, tx(21), &catalog)?;
            }
            "outputs" => {
                self.transport.publish_content_output_facts(
                    &mut self.registry,
                    tx(31),
                    FACTS,
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
            "opening" => {
                let opening = self.opening();
                self.transport
                    .publish_native_launcher_opening(&self.registry, tx(30), opening)?;
            }
            // Scripted presentation observation, then the owner's focus mint.
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
                self.transport
                    .install_native_launcher_focus(&mut self.registry, tx(300 + generation))?;
            }
            "text" | "accept" => {
                let focus = self
                    .transport
                    .native_launcher_focus()
                    .expect("presented focus");
                let (kind, text, n) = if phase == "text" {
                    (NativeLauncherInputKind::Text, "a", 400)
                } else {
                    (NativeLauncherInputKind::Accept, "", 401)
                };
                let issued = self.transport.issue_native_launcher_input(
                    &mut self.registry,
                    focus,
                    tx(n),
                    kind,
                    text,
                    ISSUED,
                )?;
                assert!(issued.is_some(), "input held instead of issued");
            }
            // Barriers: exactly the records sent so far, nothing extra.
            "ack-consumed" => {
                if self.input_acks < 1 {
                    return Ok(false);
                }
                assert_eq!(
                    (self.input_acks, self.unmatched_acks, self.activations),
                    (1, 0, 0)
                );
            }
            "one-admission" => {
                if self.input_acks < 2 || self.activations < 1 {
                    return Ok(false);
                }
                assert_eq!(
                    (
                        self.input_acks,
                        self.unmatched_acks,
                        self.activations,
                        self.admissions,
                        self.pointer
                    ),
                    (2, 0, 1, 1, 0)
                );
            }
            "close" => {
                let opening = self.opening();
                self.transport.close_native_launcher(
                    &mut self.registry,
                    opening,
                    tx(500),
                    ContentReason::Revoked,
                )?;
            }
            // Scripted: the old pixels are gone. The real owner invalidates.
            "invalidate" => {
                self.transport.invalidate_content_allocation(
                    &mut self.registry,
                    tx(510),
                    allocation_id(),
                    ContentReason::Revoked,
                )?;
            }
            _ => panic!("unknown native phase {phase}"),
        }
        Ok(true)
    }
    pub fn tick(&mut self) -> Result<(), String> {
        if let Err(error) = self.service() {
            if !matches!(error, ShellTransportError::NotConnected) {
                self.transport.disconnect(&mut self.registry).unwrap();
                self.state = 3;
                return Err(format!("native owner error: {error:?}"));
            }
            self.transport.disconnect(&mut self.registry).unwrap();
            self.state = 3;
        }
        Ok(())
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
        self.transport.poll_io(&mut self.registry)?;
        if let Some(closed) = self.transport.native_launcher_closed_opening() {
            // Resource records stay serviceable after close.
            self.transport
                .service_closed_native_content(&mut self.registry, closed, NOW)?;
            self.transport
                .service_closed_native_input(&mut self.registry, closed)?;
            return Ok(());
        }
        let Some((opening, revision)) = self.transport.native_launcher_state() else {
            return Ok(());
        };
        let catalog = self.catalog().catalog;
        let allocations = self.transport.content_allocation_snapshots(&self.registry);
        let context = ContentCandidateContext {
            output: output(),
            facts_generation: FACTS,
            interaction_generation: INTERACTION,
            allocations: &allocations,
        };
        let native_context = NativeLauncherCandidateContext {
            opening,
            state_revision: revision,
            catalog: &catalog,
        };
        self.transport.service_native_launcher_content(
            &mut self.registry,
            context,
            native_context,
            NOW,
        )?;
        while let Some((_, request)) = self
            .transport
            .next_content_allocation_request(&self.registry)
        {
            let allocation = self.allocation();
            self.transport.grant_content_allocation(
                &mut self.registry,
                request.allocation_request_id,
                allocation,
                &[],
            )?;
        }
        while let Some((transaction, demand)) = self.transport.next_content_demand(&self.registry) {
            self.permit += 1;
            self.transport.grant_content_demand(
                &mut self.registry,
                transaction,
                demand.output,
                self.permit,
                NOW,
            )?;
        }
        if self.prepared.is_none()
            && let Some((_, generation)) = self.transport.next_content_submission(&self.registry)
        {
            let render = self.transport.begin_native_launcher_submission(
                &mut self.registry,
                generation,
                context,
                native_context,
                NOW,
            )?;
            assert_eq!(
                render
                    .resource(ContentResourceId {
                        id: 5,
                        generation: 1
                    })
                    .unwrap()
                    .bytes()
                    .len(),
                8
            );
            self.transport.content_prepared(
                &mut self.registry,
                self.grant,
                output(),
                generation,
                7,
                8,
                NOW,
            )?;
            drop(render);
            self.prepared = Some(generation);
        }
        while let Some((_, _, matched)) = self
            .transport
            .poll_native_launcher_input_ack(&mut self.registry)?
        {
            if matched {
                self.input_acks += 1;
            } else {
                self.unmatched_acks += 1;
            }
        }
        if let Some((transaction, activation)) = self
            .transport
            .poll_native_launcher_activation(&mut self.registry)?
        {
            self.activations += 1;
            let decision = match self.transport.native_launcher_activation_eligibility(
                transaction,
                &activation,
                &catalog,
                ISSUED,
            )? {
                NativeLauncherActivationEligibility::Rejected(d) => d,
                NativeLauncherActivationEligibility::Pointer => {
                    self.pointer += 1;
                    NativeLauncherActivationDecision::Stale
                }
                // Scripted Session queue decision.
                NativeLauncherActivationEligibility::Keyboard => {
                    self.admissions += 1;
                    NativeLauncherActivationDecision::Admitted
                }
            };
            self.transport.finish_native_launcher_activation(
                &self.registry,
                transaction,
                &activation,
                decision,
            )?;
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
            "native fixture retained content"
        );
    }
    pub fn assert_finished(&self) {
        assert_eq!(
            (
                self.input_acks,
                self.unmatched_acks,
                self.activations,
                self.admissions,
                self.pointer
            ),
            (2, 0, 1, 1, 0)
        );
    }
}

//! Real ShellComponentTransport (native launcher profile) and its real content,
//! candidate, allocation and native launcher control owners, serving the live
//! Bemenu executable. Session decisions are scripted here and scoped: catalog
//! and output-fact publication, allocation geometry (the peer's own request),
//! demand permits, Prepared/Presented, input issue, activation admission,
//! close and allocation invalidation. No launch policy is exercised (Admitted
//! is the Session queue decision, not an application start) and no physical
//! rendering claim is made: the uploaded pixels are compared with each other,
//! never with a golden image or a display.
//!
//! Clocks: the content owners run on a frozen clock (NOW), so no candidate,
//! allocation, permit, preparation, presentation or resource expiry is
//! exercised. Input is issued at real host CLOCK_MONOTONIC microseconds
//! (strictly increasing), the clock Bemenu's scheduling hints use; no ack is
//! ever withheld, so the action/input ack timeout and its reason-6 close are
//! not exercised either.
use sophia_protocol::*;
use sophia_runtime::*;
use std::path::Path;
use std::time::Duration;

const NOW: u64 = 10;
const FACTS: u64 = 3;
const CATALOG: u64 = 1;
const INTERACTION: u64 = 1;
const SCALE_GENERATION: u64 = 5;
pub const EPOCH: u64 = 60;

/// Host CLOCK_MONOTONIC in microseconds, strictly increasing across issues:
/// the clock Bemenu's CLOCK_MONOTONIC scheduling hints are computed in.
struct Clock {
    last: u64,
}
impl Clock {
    fn new() -> Self {
        Self { last: 0 }
    }
    fn now_usec(&mut self) -> u64 {
        let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let now = u64::try_from(now.tv_sec).unwrap() * 1_000_000
            + u64::try_from(now.tv_nsec).unwrap() / 1000;
        self.last = now.max(self.last + 1);
        self.last
    }
}

/// One candidate as the renderer received it, with copies of its pixels.
pub struct Shown {
    pub generation: u64,
    pub opening: u64,
    pub revision: u64,
    pub rows: Vec<u16>,
    pub selected: u16,
    pub pixels: Vec<Vec<u8>>,
}

pub struct Fixture {
    transport: ShellComponentTransport,
    registry: ContentEpochRegistry,
    grant: ContentGrant,
    clock: Clock,
    live: bool,
    transaction: u64,
    permit: u64,
    allocations: u64,
    prepared: Option<u64>,
    pub shown: Vec<Shown>,
    pub granted: Vec<ContentAllocationId>,
    pub input_acks: usize,
    pub unmatched_acks: usize,
    pub activations: Vec<NativeLauncherActivation>,
    pub admissions: usize,
    pub pointer: usize,
}
fn output() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 1,
    }
}
fn catalog() -> ShellPersistentCatalog {
    ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: EPOCH,
            generation: CATALOG,
            entries: ["alpha", "bravo", "charlie"]
                .into_iter()
                .zip(1..)
                .map(|(label, slot)| ShellApplicationDescriptor {
                    slot,
                    available: true,
                    label: label.to_owned(),
                    keywords: String::new(),
                })
                .collect(),
        },
        identities: Default::default(),
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
                connection_epoch: EPOCH,
                content_grant_epoch: 1,
            },
            clock: Clock::new(),
            live: false,
            transaction: 1000,
            permit: 0,
            allocations: 0,
            prepared: None,
            shown: Vec::new(),
            granted: Vec::new(),
            input_acks: 0,
            unmatched_acks: 0,
            activations: Vec::new(),
            admissions: 0,
            pointer: 0,
        }
    }
    pub fn socket_path(&self) -> &Path {
        self.transport.socket_path()
    }
    fn tx(&mut self) -> TransactionId {
        self.transaction += 1;
        TransactionId::from_raw(self.transaction)
    }
    /// The real supervisor evidence, before the peer's queued connect is accepted.
    pub fn authorize(&mut self, evidence: &ProtectionDomainEvidence) {
        self.transport.authorize_protected_peer(evidence).unwrap();
        self.transport
            .reserve_content_with_profile(
                &mut self.registry,
                ContentLimits::prototype(self.grant),
                ContentStoreProfile::NativeLauncher,
            )
            .unwrap();
        self.transport
            .begin_file_negotiation(
                &self.registry,
                EPOCH,
                Duration::from_secs(15),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            )
            .unwrap();
    }
    pub fn live(&self) -> bool {
        self.live
    }
    pub fn prepared(&self) -> Option<&Shown> {
        self.prepared
            .and_then(|g| self.shown.iter().find(|s| s.generation == g))
    }
    pub fn publish(&mut self) {
        let tx = self.tx();
        self.transport
            .publish_catalog(&self.registry, tx, &catalog())
            .unwrap();
        let tx = self.tx();
        self.transport
            .publish_content_output_facts(
                &mut self.registry,
                tx,
                FACTS,
                vec![ContentOutputFactsEntry {
                    output: output(),
                    local_width: 1280,
                    local_height: 720,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    scale_generation: SCALE_GENERATION,
                }],
            )
            .unwrap();
    }
    pub fn opening(&self, opening: u64) -> NativeLauncherOpening {
        NativeLauncherOpening {
            grant: self.grant,
            opening,
            output: output(),
            catalog_generation: CATALOG,
            state_revision: 1,
        }
    }
    pub fn open(&mut self, opening: u64) {
        let tx = self.tx();
        let opening = self.opening(opening);
        self.transport
            .publish_native_launcher_opening(&self.registry, tx, opening)
            .unwrap();
    }
    /// Scripted presentation observation, then the owner's own focus mint.
    pub fn present(&mut self) -> NativeLauncherBinding {
        let generation = self.prepared.take().expect("a prepared candidate");
        self.transport
            .content_presented(
                &mut self.registry,
                self.grant,
                output(),
                generation,
                100 + generation,
                7,
                8,
            )
            .unwrap();
        let tx = self.tx();
        let focus = self
            .transport
            .install_native_launcher_focus(&mut self.registry, tx)
            .unwrap();
        assert_eq!(self.transport.native_launcher_focus(), Some(focus));
        assert_eq!(focus.candidate_generation, generation);
        assert_eq!(focus.presentation_epoch, 100 + generation);
        focus
    }
    pub fn input(&mut self, kind: NativeLauncherInputKind, text: &str) {
        let focus = self.transport.native_launcher_focus().expect("focus");
        let tx = self.tx();
        let issued = self.clock.now_usec();
        let event = self
            .transport
            .issue_native_launcher_input(&mut self.registry, focus, tx, kind, text, issued)
            .unwrap();
        assert!(event.is_some(), "input held instead of issued");
    }
    pub fn close(&mut self, opening: u64) {
        let tx = self.tx();
        let opening = self.opening(opening);
        self.transport
            .close_native_launcher(&mut self.registry, opening, tx, ContentReason::Revoked)
            .unwrap();
        // Scripted: the old pixels are gone. The real owner invalidates.
        let allocation = *self.granted.last().expect("granted allocation");
        let tx = self.tx();
        self.transport
            .invalidate_content_allocation(
                &mut self.registry,
                tx,
                allocation,
                ContentReason::Revoked,
            )
            .unwrap();
    }
    /// Retained stores, credits and FIFO for the closed opening are empty:
    /// the peer released its resources; nothing was disposed by disconnect.
    pub fn settled(&self, opening: u64) -> bool {
        self.transport.native_launcher_closed_opening() == Some(self.opening(opening))
            && self
                .transport
                .closed_native_owners_settled(&self.registry, self.opening(opening))
                .unwrap()
    }
    pub fn tick(&mut self) -> Result<(), String> {
        self.service().map_err(|e| format!("owner error: {e:?}"))
    }
    fn service(&mut self) -> Result<(), ShellTransportError> {
        if !self.live {
            self.live = self
                .transport
                .poll_negotiation(&mut self.registry, 65536)?
                .is_some();
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
        let catalog = catalog().catalog;
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
        // Scripted Session geometry: exactly what the peer asked for.
        while let Some((_, request)) = self
            .transport
            .next_content_allocation_request(&self.registry)
        {
            self.allocations += 1;
            let allocation = ContentAllocationId {
                id: self.allocations,
                generation: 1,
            };
            let snapshot = ContentAllocationSnapshot {
                native_opening: Some(opening.opening),
                output: output(),
                allocation,
                scale_generation: SCALE_GENERATION,
                scale_numerator: 1,
                scale_denominator: 1,
                role: request.role,
                edge: request.edge,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: request.desired_width,
                    height: request.desired_height,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: request.desired_width,
                    height: request.desired_height,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: 0,
            };
            self.transport.grant_content_allocation(
                &mut self.registry,
                request.allocation_request_id,
                snapshot,
                &[],
            )?;
            self.granted.push(allocation);
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
            let binding = render.native_launcher.expect("native binding");
            assert!(!render.placements.is_empty(), "candidate without pixels");
            let pixels = render
                .placements
                .iter()
                .map(|placement| {
                    let lease = render
                        .resource(placement.resource)
                        .expect("placed resource is leased");
                    let description = lease.description();
                    assert!(description.width_px > 0 && description.height_px > 0);
                    assert_eq!(lease.bytes().len() as u64, description.total_bytes);
                    lease.bytes().to_vec()
                })
                .collect::<Vec<_>>();
            self.transport.content_prepared(
                &mut self.registry,
                self.grant,
                output(),
                generation,
                7,
                8,
                NOW,
            )?;
            // Scripted renderer retirement: the pixels were copied above.
            drop(render);
            self.shown.push(Shown {
                generation,
                opening: binding.opening,
                revision: binding.state_revision,
                rows: binding.rows().to_vec(),
                selected: binding.selected,
                pixels,
            });
            self.prepared = Some(generation);
        }
        while let Some((_, ack, matched)) = self
            .transport
            .poll_native_launcher_input_ack(&mut self.registry)?
        {
            if matched && ack.disposition == 1 {
                self.input_acks += 1;
            } else {
                self.unmatched_acks += 1;
            }
        }
        if let Some((transaction, activation)) = self
            .transport
            .poll_native_launcher_activation(&mut self.registry)?
        {
            self.activations.push(activation);
            let now = self.clock.now_usec();
            let decision = match self.transport.native_launcher_activation_eligibility(
                transaction,
                &activation,
                &catalog,
                now,
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
            "bemenu fixture retained content"
        );
    }
}

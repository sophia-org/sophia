//! t100 reconnect join. Real private sockets, registry, Session service order,
//! backend intake/queue and retained byte consumers. Protection evidence and
//! the concrete native submission boundary are supplied; copy/flip completion
//! is simulated by the backend's existing Target. No device or protected child.
use super::*;
use crate::live_session::{
    PersistentXtermSessionConfig, SessionLaunchQueue, component_catalog, component_service,
};
use sophia_backend_live::{
    LiveProductionCpuScene, LiveShellContentFrame, LiveShellContentLayer,
    session_content_fixture::SessionContentFixture,
};
use sophia_engine::{CompositorContentImage, CompositorNodeId, HeadlessOutput};
use sophia_protocol::*;
use sophia_runtime::{
    ContentAllocationSnapshot, ContentCandidateContext, ContentRenderBundle, ContentResourceLease,
};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[path = "component_reconnect/wire.rs"]
mod wire;
use super::super::content::reconnect_fixture::prepare;

const OUTPUT: ContentOutputId = ContentOutputId {
    id: 1,
    generation: 1,
};
const RESOURCE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};
const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn output() -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 64,
            height: 32,
        },
        scale: 1,
    }
}

struct Harness {
    owner: ShellComponentSession,
    backend: SessionContentFixture,
    root: std::path::PathBuf,
    catalog: component_catalog::ComponentCatalog,
    launches: SessionLaunchQueue,
    config: PersistentXtermSessionConfig,
}

impl Harness {
    fn new() -> Self {
        Self::with_dock(false)
    }

    fn with_dock(has_dock: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "t100-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let roles = [
            ("panel", ShellComponentRole::Bar),
            ("menu", ShellComponentRole::ApplicationLauncher),
            ("dock", ShellComponentRole::Dock),
        ];
        let selections: Vec<_> = roles[..if has_dock { 3 } else { 2 }]
            .iter()
            .copied()
            .map(|(id, role)| ShellComponentConfig {
                id: id.into(),
                role,
                executable: "/nonexistent-t100-component".into(),
                config: None,
                reservation: (has_dock && role != ShellComponentRole::ApplicationLauncher)
                    .then_some(sophia_config::ShellComponentReservation {
                        edge: if role == ShellComponentRole::Bar {
                            sophia_config::ShellComponentEdge::Top
                        } else {
                            sophia_config::ShellComponentEdge::Bottom
                        },
                        max_thickness: 8,
                    }),
                gpu: ShellGpuMode::Denied,
            })
            .collect();
        let mut owner = ShellComponentSession::prepare(
            &selections,
            8,
            None,
            &root,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
        owner.set_presentation_available(true).unwrap();
        let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/fixtures/mixed_output_probe.kdl");
        Self {
            owner,
            backend: SessionContentFixture::new(&[output()]).unwrap(),
            root,
            catalog: Default::default(),
            launches: Default::default(),
            config: PersistentXtermSessionConfig::from_args(&[format!(
                "--desktop-profile={}",
                profile.display()
            )])
            .unwrap(),
        }
    }

    fn connect(
        &mut self,
        slot: usize,
    ) -> (ComponentConnectionKey, UnixStream, ContentResourceLease) {
        self.connect_extent(slot, 1)
    }

    fn connect_extent(
        &mut self,
        slot: usize,
        extent: u32,
    ) -> (ComponentConnectionKey, UnixStream, ContentResourceLease) {
        let (key, mut peer) = wire::connect(&mut self.owner, slot);
        let lease = self
            .owner
            .with_service(key, |_, transport| wire::upload(transport, &mut peer))
            .unwrap();
        if slot == 0 {
            self.owner
                .with_service(key, |_, transport| {
                    wire::allocate_extent(transport, &mut peer, extent)
                })
                .unwrap();
        }
        (key, peer, lease)
    }

    fn candidate(&mut self, key: ComponentConnectionKey, peer: &mut UnixStream, generation: u64) {
        self.owner
            .with_service(key, |service, transport| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("panel")
                };
                let bundle = wire::candidate(transport, peer, generation);
                prepare(
                    &mut bar.content,
                    transport,
                    &mut self.backend,
                    output(),
                    bundle,
                );
                wire::outcome(transport, peer, generation, 1);
            })
            .unwrap();
    }

    fn complete(&mut self, key: ComponentConnectionKey, peer: &mut UnixStream, generation: u64) {
        self.backend.simulate_completion(output().id);
        self.owner
            .with_service(key, |service, transport| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("panel")
                };
                assert!(
                    bar.observe_presentation(transport, self.backend.runtime())
                        .unwrap()
                );
                assert!(
                    !bar.observe_presentation(transport, self.backend.runtime())
                        .unwrap()
                );
                wire::outcome(transport, peer, generation, 2);
            })
            .unwrap();
    }

    fn issue_current(
        &mut self,
        key: ComponentConnectionKey,
        peer: &mut UnixStream,
    ) -> ContentAction {
        let publication = sophia_engine::PolicyIndicatorPublication {
            generation: 1,
            connection_epoch: Some(1),
            tab_groups: vec![],
            output_statuses: vec![],
            indicators: vec![PolicyProjectionIndicator {
                output: output().id,
                slot: 0,
                indicator: 1,
                action: Some(WmActionId::from_raw(1)),
                state_bits: 0,
                label: "workspace".into(),
            }],
        };
        self.owner
            .with_service(key, |service, transport| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("panel")
                };
                bar.service_indicators(transport, Some(&publication), Some(output().id))
                    .unwrap();
                transport.poll_io().unwrap();
            })
            .unwrap();
        let snapshot = crate::shell_indicator_projection::indicator_snapshot(
            &publication,
            Some(output().id),
            key.grant.connection_epoch,
        );
        for _ in encode_shell_indicator_snapshot(TransactionId::from_raw(1), &snapshot).unwrap() {
            wire::read(peer);
        }
        let target = self.backend.runtime().input_projections()[0]
            .content
            .iter()
            .find(|binding| binding.grant == key.grant)
            .unwrap()
            .targets[0]
            .clone();
        let event = component_service::issue_component_activation(
            &mut self.owner,
            target.clone(),
            self.backend.runtime(),
            &mut self.catalog,
        )
        .unwrap()
        .expect("current target must issue");
        self.owner
            .with_service(key, |_, transport| transport.poll_io().unwrap())
            .unwrap();
        let (_, ShellContentRecord::Action(action)) =
            decode_shell_content_frame(&wire::read(peer)).unwrap()
        else {
            panic!("issued action")
        };
        assert_eq!(
            (
                action.grant,
                action.event_id,
                action.target_id,
                action.presentation_epoch
            ),
            (
                key.grant,
                event,
                target.target_id,
                target.presentation_epoch
            )
        );
        action
    }

    fn service(&mut self) {
        // This is the production owner-loop entry, including both settlement
        // calls around role service, not a recreated sequence of callbacks.
        component_service::service_components(
            &mut self.owner,
            &mut self.catalog,
            self.backend.runtime_mut(),
            &LiveProductionCpuScene::new(output().size),
            None,
            &[output()],
            &mut None,
            true,
            &mut self.launches,
            &mut Vec::new(),
            &self.config,
            Path::new("/nonexistent-t100-xauthority"),
            &mut None,
        )
        .unwrap();
    }

    fn debt(
        &mut self,
        key: ComponentConnectionKey,
        peer: &mut UnixStream,
        neighbor: ContentResourceLease,
    ) {
        self.candidate(key, peer, 1);
        self.backend.simulate_submit(output().id);
        self.complete(key, peer, 1);
        assert!(!self.owner.work_area_bands().is_empty());
        // A neighboring layer has independent native work outstanding. The
        // panel can submit one replacement while that real queue owner holds
        // the output, but cannot submit two candidates for one output.
        self.backend
            .admit(neighbor_frame(neighbor), LiveShellContentLayer::Launcher)
            .unwrap();
        self.backend.simulate_submit(output().id);
        self.candidate(key, peer, 2);
        assert_eq!(self.backend.claims().len(), 1);
        assert_eq!(self.backend.claims()[0].2, key.grant);
    }
}

fn neighbor_frame(lease: ContentResourceLease) -> LiveShellContentFrame {
    let grant = lease.description().grant;
    LiveShellContentFrame {
        output: output().id,
        content_output: OUTPUT,
        grant,
        candidate_generation: 1,
        interaction_generation: 1,
        popouts: vec![],
        targets: vec![],
        allocations: vec![],
        images: vec![CompositorContentImage {
            node: CompositorNodeId::ShellContent {
                grant,
                output: output().id,
                candidate: 1,
                surface: 0,
                placement: 0,
            },
            generation: 1,
            output_size_px: output().size,
            geometry_px: Rect {
                x: 3,
                y: 3,
                width: 1,
                height: 1,
            },
            size_px: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: u32::from_le_bytes(*b"AR24"),
            resource: lease,
        }],
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn component_disconnect_settles_real_debt_without_disposing_pixels_or_neighbor() {
    let mut h = Harness::new();
    let (old, mut peer, retained) = h.connect(0);
    let (neighbor, neighbor_peer, neighbor_pixels) = h.connect(1);
    h.debt(old, &mut peer, neighbor_pixels.clone());
    let bands = h.owner.work_area_bands();
    let old_target = h.backend.runtime().input_projections()[0].content[0].targets[0].clone();
    h.issue_current(old, &mut peer);
    drop(peer);
    h.service();
    assert_eq!(
        h.owner.phase(old).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert!(
        h.backend.claims().is_empty(),
        "production service must settle the old grant"
    );
    assert_eq!(h.owner.pending_revocations(), 0);
    assert_eq!(h.owner.work_area_bands(), bands);
    assert_eq!(retained.bytes(), &[1, 2, 3, 255]);
    assert!(h.backend.copied_backings() > 0);
    assert!(h.owner.with_service(old, |_, _| ()).is_err());
    assert_eq!(
        component_service::issue_component_activation(
            &mut h.owner,
            old_target,
            h.backend.runtime(),
            &mut h.catalog
        )
        .unwrap(),
        None
    );
    assert_eq!(
        h.owner.phase(neighbor).unwrap(),
        ComponentConnectionPhase::Connected
    );
    assert_eq!(neighbor_pixels.bytes(), &[1, 2, 3, 255]);
    let accounting = h.owner.accounting();
    assert!(!accounting.quiescent());
    for _ in 0..3 {
        // Suppress automatic retry timing only; settlement still runs through
        // the unchanged production service on an otherwise idle visit.
        h.owner.retry_at[0] = Some(Instant::now() + Duration::from_secs(60));
        h.service();
        assert_eq!(h.owner.pending_revocations(), 0);
        assert_eq!(h.owner.accounting(), accounting);
    }
    // Finishing the old submitted copy supplies no receipt to a closed peer.
    h.backend.simulate_completion(output().id);
    assert!(h.owner.with_service(old, |_, _| ()).is_err());
    h.owner.request_shutdown().unwrap();
    h.owner
        .settle_revocations(Some(h.backend.runtime_mut()))
        .unwrap();
    drop((neighbor_peer, neighbor_pixels));
    h.backend.teardown();
    // The independent byte owner prevents a false quiescent accounting result.
    assert!(!h.owner.collect().quiescent());
    drop(retained);
}

#[test]
fn component_revocation_waits_for_runtime_and_retries_without_a_peer_message() {
    let mut h = Harness::new();
    let (old, mut peer, retained) = h.connect(0);
    let (_neighbor, _neighbor_peer, neighbor_pixels) = h.connect(1);
    h.debt(old, &mut peer, neighbor_pixels.clone());
    h.owner.stop(old).unwrap();
    drop(peer);
    assert_eq!(h.owner.settle_revocations(None).unwrap(), 0);
    assert_eq!(h.owner.pending_revocations(), 1);
    assert_eq!(h.backend.claims()[0].2, old.grant);
    h.owner.retry_at[0] = Some(Instant::now() + Duration::from_secs(60));
    h.service();
    assert_eq!(h.owner.pending_revocations(), 0);
    assert!(h.backend.claims().is_empty());
    assert_eq!(
        h.owner
            .settle_revocations(Some(h.backend.runtime_mut()))
            .unwrap(),
        0
    );
    assert_eq!(retained.bytes(), &[1, 2, 3, 255]);
}

#[test]
fn replacement_reuses_numbers_without_inheriting_completion_actions_or_consumers() {
    // The supported three-role profile reserves smaller per-role allowances.
    // Two full role reservations plus retained bytes correctly refuse overlap;
    // this positive control uses real policy headroom, not enlarged test limits.
    let mut h = Harness::with_dock(true);
    let (old, mut peer, retained) = h.connect(0);
    let (neighbor, mut neighbor_peer, neighbor_pixels) = h.connect(1);
    h.debt(old, &mut peer, neighbor_pixels.clone());
    let bands = h.owner.work_area_bands();
    let old_target = h.backend.runtime().input_projections()[0].content[0].targets[0].clone();
    h.issue_current(old, &mut peer);
    drop(peer);
    h.service();
    assert!(h.backend.claims().is_empty());
    let (replacement, mut peer, replacement_pixels) = h.connect_extent(0, 2);
    assert_ne!(old.grant, replacement.grant);
    assert_eq!(
        h.owner.work_area_bands(),
        bands,
        "negotiation must not shrink the work area"
    );
    h.candidate(replacement, &mut peer, 1);
    assert_eq!(h.backend.claims()[0].2, replacement.grant);
    assert_eq!(
        h.backend
            .runtime()
            .shell_content_presentation_epoch(output().id, replacement.grant, 1),
        None
    );
    assert!(h.owner.stop(old).is_err());
    // Repeated settlement cannot erase the successor's distinct obligation.
    assert_eq!(
        h.owner
            .settle_revocations(Some(h.backend.runtime_mut()))
            .unwrap(),
        0
    );
    assert_eq!(h.backend.claims()[0].2, replacement.grant);
    h.backend.simulate_completion(output().id);
    // The old frame contains numerically equal candidate 1, under the OLD grant.
    assert_eq!(
        h.backend
            .runtime()
            .shell_content_presentation_epoch(output().id, replacement.grant, 1),
        None
    );
    h.owner
        .with_service(replacement, |service, transport| {
            let ShellComponentService::Bar(bar) = service else {
                panic!("panel")
            };
            assert!(
                !bar.observe_presentation(transport, h.backend.runtime())
                    .unwrap()
            );
            // A duplicate old completion never settles the new candidate. A real
            // late terminal for a disconnected submitted candidate may settle its
            // old store; that is not a receipt or authority for this connection.
            assert!(
                transport
                    .content_presented(old.grant, OUTPUT, 1, old_target.presentation_epoch, 1, 1)
                    .is_err()
            );
        })
        .unwrap();
    assert_eq!(
        component_service::issue_component_activation(
            &mut h.owner,
            old_target,
            h.backend.runtime(),
            &mut h.catalog
        )
        .unwrap(),
        None
    );
    assert_eq!(h.owner.work_area_bands(), bands);
    let neighbor_next = h
        .owner
        .with_service(neighbor, |_, transport| {
            wire::upload_id(
                transport,
                &mut neighbor_peer,
                ContentResourceId {
                    id: 2,
                    generation: 1,
                },
            )
        })
        .unwrap();
    assert_eq!(neighbor_next.bytes(), &[1, 2, 3, 255]);
    assert!(h.backend.retry().unwrap());
    assert!(h.backend.claims().is_empty());
    h.backend.simulate_submit(output().id);
    h.complete(replacement, &mut peer, 1);
    assert_ne!(
        h.owner.work_area_bands(),
        bands,
        "the completed replacement must publish its new extent"
    );
    h.issue_current(replacement, &mut peer);
    h.owner
        .with_service(replacement, |_, transport| {
            let epoch = h
                .backend
                .runtime()
                .shell_content_presentation_epoch(output().id, replacement.grant, 1)
                .unwrap();
            assert!(
                transport
                    .content_presented(replacement.grant, OUTPUT, 1, epoch, 1, 1)
                    .is_err(),
                "a second terminal must refuse"
            );
        })
        .unwrap();
    // Final transfer drops the actual backend owners. One independently held
    // old source still prevents quiescence until that consumer itself ends.
    h.owner.request_shutdown().unwrap();
    h.owner
        .settle_revocations(Some(h.backend.runtime_mut()))
        .unwrap();
    h.backend.teardown();
    drop((
        peer,
        neighbor_peer,
        neighbor_pixels,
        neighbor_next,
        replacement_pixels,
    ));
    let backend = std::mem::replace(
        &mut h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    let (settled, accounting) = h
        .owner
        .finish_after_backend_drop(backend)
        .unwrap_or_else(|_| panic!("final fixture owner transfer"));
    assert!(
        settled > 0,
        "old submitted candidate has an undeliverable terminal"
    );
    assert!(!accounting.quiescent());
    assert_eq!(accounting.active_epochs, 0);
    assert_eq!(accounting.memory.resident + accounting.memory.retiring, 4);
    assert_eq!(accounting.memory.backing, 4);
    assert_eq!(retained.bytes(), &[1, 2, 3, 255]);
    drop(retained);
    assert!(h.owner.collect().quiescent());
    assert!(h.owner.collect().quiescent());
}

#[test]
fn legacy_disconnect_keeps_its_distinct_revocation_join_and_deferred_claim() {
    use super::super::LiveMetadataShell;
    use sophia_runtime::{ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole};
    use std::io::Write;
    let mut h = Harness::new();
    let (_neighbor, _neighbor_peer, neighbor_pixels) = h.connect(1);
    let mut legacy = LiveMetadataShell::prepare(
        "/bin/false",
        Some(8),
        true,
        true,
        ShellGpuMode::Denied,
        None,
        None,
    )
    .unwrap();
    // Independent fixtures use disjoint grant identities in this shared backend.
    legacy.next_connection_epoch = 9;
    legacy
        .transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    let mut peer = UnixStream::connect(legacy.transport.socket_path()).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    peer.write_all(&wire::hello(false)).unwrap();
    let welcome = legacy
        .transport
        .accept_and_negotiate_with_content_policy(
            9,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
    legacy
        .finish_negotiation(std::process::id(), welcome.selected_revision, 9, "fixture")
        .unwrap();
    wire::read(&mut peer);
    wire::read(&mut peer);
    let retained = wire::upload(&mut legacy.transport.connection(), &mut peer);
    wire::allocate(&mut legacy.transport.connection(), &mut peer);
    for generation in [1, 2] {
        let bundle = wire::candidate(&mut legacy.transport.connection(), &mut peer, generation);
        prepare(
            &mut legacy.content,
            &mut legacy.transport.connection(),
            &mut h.backend,
            output(),
            bundle,
        );
        wire::outcome(&mut legacy.transport.connection(), &mut peer, generation, 1);
        if generation == 1 {
            h.backend.simulate_submit(output().id);
            h.backend.simulate_completion(output().id);
            assert!(
                legacy
                    .observe_content_presentation(h.backend.runtime())
                    .unwrap()
            );
            wire::outcome(&mut legacy.transport.connection(), &mut peer, generation, 2);
            h.backend
                .admit(
                    neighbor_frame(neighbor_pixels.clone()),
                    LiveShellContentLayer::Launcher,
                )
                .unwrap();
            h.backend.simulate_submit(output().id);
        }
    }
    let bands = legacy.work_area_bands();
    assert!(!bands.is_empty());
    assert_eq!(h.backend.claims().len(), 1);
    drop(peer);
    // Recovery while presentation is paused uses the production legacy
    // retirement path without attempting a protected process restart.
    legacy.recover_transport("fixture_disconnect").unwrap();
    let absent = legacy.settle_revoked_content_grants(None);
    assert_eq!((absent.grants, absent.retained), (0, 1));
    let settled = legacy.settle_revoked_content_grants(Some(h.backend.runtime_mut()));
    assert_eq!(
        (settled.grants, settled.claims, settled.retained),
        (1, 1, 0)
    );
    assert!(h.backend.claims().is_empty());
    assert_eq!(legacy.work_area_bands(), bands);
    assert_eq!(retained.bytes(), &[1, 2, 3, 255]);
    assert_eq!(
        legacy
            .settle_revoked_content_grants(Some(h.backend.runtime_mut()))
            .grants,
        0
    );
}

#[test]
#[ignore = "t100 pinned red: two-role reservations plus retained old panel block reconnect"]
fn two_role_panel_reconnect_progresses_after_all_old_native_work_completes() {
    let mut h = Harness::new();
    let (old, mut peer, retained) = h.connect(0);
    let (neighbor, _neighbor_peer, neighbor_pixels) = h.connect(1);
    h.debt(old, &mut peer, neighbor_pixels.clone());
    drop((peer, retained)); // No test-owned old byte consumer masks progress.
    h.service();
    assert!(h.backend.claims().is_empty());
    h.backend.simulate_completion(output().id);
    h.backend.retry().unwrap();
    if h.backend.queued(output().id) {
        h.backend.simulate_submit(output().id);
        h.backend.simulate_completion(output().id);
    }
    for pass in 0..4 {
        // Advance only the retry eligibility, not owner state or budgets. Each
        // actual service visit retries reserve_attempt and runs collection.
        h.owner.retry_at[0] = Some(Instant::now() - Duration::from_secs(1));
        h.service();
        let accounting = h.owner.collect();
        eprintln!("t100 two-role retry={pass} accounting={accounting:?}");
        assert_eq!(
            h.owner.phase(neighbor).unwrap(),
            ComponentConnectionPhase::Connected
        );
    }
    // The executable is deliberately absent. Reaching its preparation would
    // still mint a fresh attempt, proving reservation can progress.
    assert_ne!(
        h.owner.attempt(0),
        Some(old),
        "old backend pixels retain 4 bytes; a full replacement reservation never fits"
    );
}

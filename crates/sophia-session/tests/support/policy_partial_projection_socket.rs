//! A real Hagia answering a request that names one output, materialized the way
//! a live session materializes it.
//!
//! A pointer drag asks the window manager about the output the drag is on and
//! nothing else. The WM answers completely for that output and says nothing
//! about the others, which is the contract. What broke was the step after:
//! reconciliation produced content only for the named output while the live
//! proposal iterated every committed one, so a second monitor with windows on
//! it turned an ordinary drag into a session failure.
//!
//! The reducer stage alone cannot show this -- it stages a proposal without
//! materializing layers -- so the assertions here run the proposal a real Hagia
//! produced through `reconcile_public_policy_proposal` and
//! `public_live_proposal` and look at the layers that come out.

use crate::live_session::{
    LivePolicySettlementIdentity, LiveWmProposal, LiveWmProposalSource, PersistentLiveLayout,
    public_live_proposal, reconcile_public_policy_proposal,
};
use sophia_protocol::*;
use std::collections::BTreeMap;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);
const LEFT: OutputId = OutputId::from_raw(1);
const RIGHT: OutputId = OutputId::from_raw(2);

fn left_bounds() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 960,
    }
}

fn right_bounds() -> Rect {
    Rect {
        x: 1280,
        y: 0,
        width: 1280,
        height: 960,
    }
}

fn surface_on(output: OutputId, index: u32, bounds: Rect) -> PolicySurfaceSnapshot {
    PolicySurfaceSnapshot {
        surface: SurfaceId::new(index, 1),
        generation: 1,
        current_output: Some(output),
        kind: PolicySurfaceKind::Toplevel,
        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        exact_size: None,
        requested_state: PolicyPresentationState::default(),
        current_state: PolicyPresentationState::default(),
        transient_owner: None,
        geometry: bounds,
    }
}

/// A cached layer as startup leaves one: pixels observed, no WM placement yet.
fn cached_layer(surface: SurfaceId, geometry: Rect) -> LayerSnapshot {
    LayerSnapshot {
        translation: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source: BufferSource::CpuBuffer { handle: 1 },
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        output: None,
        input_region: None,
    }
}

/// Adopt what a commit installs. Without this the layout never gains the
/// retained output owners a later partial cycle has to preserve.
fn install(layout: &mut PersistentLiveLayout, proposal: &LiveWmProposal) {
    for layer in &proposal.layers {
        layout.layers.insert(layer.surface, layer.clone());
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct PartialFixture {
    child: ChildGuard,
    transport: sophia_runtime::PolicyWmSessionTransport,
    reducer: sophia_engine::PolicyProjectionReducer,
    transaction: u64,
    actions: Vec<PolicyActionRegistration>,
}

impl PartialFixture {
    fn new(binary: std::ffi::OsString, directory: &std::path::Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let mut transport = sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid(
            directory.join("policy"),
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let profile = directory.join("desktop.kdl");
        std::fs::write(
            &profile,
            "schema 1\npolicy { focus-follows-mouse #true; }\n",
        )
        .unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut command = Command::new(binary);
        // The inherited installed session must not reach the child; it would
        // point this Hagia at another session's sockets and profile.
        for (name, _) in std::env::vars_os() {
            let name = name.to_string_lossy().into_owned();
            if name.starts_with("SOPHIA_") || name.starts_with("HAGIA_") {
                command.env_remove(name);
            }
        }
        let child = ChildGuard(
            command
                .arg(format!("--config={}", profile.display()))
                .arg(format!("--socket={}", transport.socket_path().display()))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        transport.authorize_supervised_pid(child.0.id()).unwrap();
        transport.accept_and_negotiate(1, WAIT).unwrap();
        let sophia_runtime::PolicyClientEvent::Configuration {
            transaction,
            configuration,
        } = transport.receive_client_event_within(WAIT).unwrap()
        else {
            panic!("missing configuration")
        };
        transport
            .send_configuration_outcome(
                transaction,
                configuration.generation,
                PolicyProjectionOutcome::Committed,
            )
            .unwrap();

        // Both monitors are populated. An empty second output would let the
        // materialization pass for the wrong reason.
        let scene = PolicySceneSnapshot {
            generation: 1,
            active_output: LEFT,
            outputs: vec![
                PolicyOutputSnapshot {
                    policy_key: None,
                    output: LEFT,
                    generation: 1,
                    focus: Some(SurfaceId::new(11, 1)),
                    bounds: left_bounds(),
                    work_area: left_bounds(),
                },
                PolicyOutputSnapshot {
                    policy_key: None,
                    output: RIGHT,
                    generation: 1,
                    focus: Some(SurfaceId::new(21, 1)),
                    bounds: right_bounds(),
                    work_area: right_bounds(),
                },
            ],
            surfaces: vec![
                surface_on(LEFT, 11, left_bounds()),
                surface_on(RIGHT, 21, right_bounds()),
            ],
            session_operations: vec![],
        };
        let mut reducer = sophia_engine::PolicyProjectionReducer::new(scene).unwrap();
        reducer.connect(1).unwrap();
        Self {
            child,
            transport,
            reducer,
            transaction: 100,
            actions: configuration
                .actions
                .into_iter()
                .filter(|action| action.session_operation_slot.is_none())
                .collect(),
        }
    }

    /// One settled cycle whose request names exactly `outputs`.
    fn cycle(
        &mut self,
        outputs: Vec<OutputId>,
        cause: PolicyRequestCause,
    ) -> (PolicyProjectionProposal, PolicyProjectionRequest) {
        let request = self
            .reducer
            .issue_request_with_cause(outputs, cause)
            .unwrap();
        let snapshot = encode_wm_v1_policy_snapshot(
            TransactionId::from_raw(self.transaction),
            1,
            self.reducer.scene(),
            &self.actions,
            &[],
            self.transport.selected_capabilities(),
        )
        .unwrap();
        self.transport
            .send_snapshot(
                snapshot.transaction,
                &snapshot.begin,
                &snapshot.chunks,
                &snapshot.end,
            )
            .unwrap();
        self.transport
            .send_projection_request(TransactionId::from_raw(self.transaction + 1), &request)
            .unwrap();
        self.transaction += 2;
        let proposal = loop {
            match self.transport.receive_client_event_within(WAIT).unwrap() {
                sophia_runtime::PolicyClientEvent::Projection(
                    sophia_runtime::QueuedPolicyProjection::Admitted(transfer),
                ) => break decode_wm_v1_policy_projection(&transfer.into_wire_transfer()).unwrap(),
                sophia_runtime::PolicyClientEvent::ProjectionPending => {}
                event => panic!("unexpected event: {event:?}"),
            }
        };
        (proposal, request)
    }
}

impl Drop for PartialFixture {
    fn drop(&mut self) {
        let _ = self.transport.disconnect();
        let _ = self.child.0.kill();
    }
}

/// One complete settlement, the way the owner loop performs it: reconcile the
/// answer, stage the reconciled policy, materialize the layers, then commit
/// that same reconciled policy. Settling the raw proposal instead would settle
/// something production never commits.
fn run(
    fixture: &mut PartialFixture,
    layout: &PersistentLiveLayout,
    outputs: Vec<OutputId>,
    cause: PolicyRequestCause,
    source: LiveWmProposalSource,
) -> LiveWmProposal {
    let (proposal, request) = fixture.cycle(outputs, cause);
    let chrome = sophia_engine::SurfaceChromeStyle::default();
    let bounds: BTreeMap<OutputId, Rect> = fixture
        .reducer
        .scene()
        .outputs
        .iter()
        .map(|output| (output.output, output.bounds))
        .collect();
    let reconciled =
        reconcile_public_policy_proposal(layout, &proposal, &bounds, &bounds, chrome).unwrap();
    let staged = fixture.reducer.stage_proposal(&reconciled.policy).unwrap();
    // Staging answers for every committed output even when the request named
    // one. That asymmetry is what this regression is about.
    assert_eq!(staged.projections().len(), 2);
    let projections = staged.projections();
    let live = public_live_proposal(
        layout,
        reconciled.policy.active_output,
        projections,
        reconciled.policy.transaction,
        source,
        LivePolicySettlementIdentity {
            connection_epoch: request.connection_epoch,
            request_id: request.request_id,
            scene_generation: request.scene_generation,
            transaction: reconciled.policy.transaction,
            expect_session_operation: false,
            session_operation: false,
        },
        &reconciled,
    )
    .expect("a request naming one output must not fail on the outputs it did not name");
    let outcome = fixture.reducer.commit_staged(staged);
    assert_eq!(outcome, PolicyProjectionOutcome::Committed);
    fixture
        .transport
        .send_projection_outcome(
            reconciled.policy.transaction,
            request.request_id,
            fixture.reducer.scene().generation,
            outcome,
        )
        .unwrap();
    live
}

#[test]
fn hagia_real_partial_pointer_projection_preserves_untouched_output() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    // Owned, per-process, and removed on the way out; the crate does not take
    // a temp-directory dependency for tests.
    let directory = std::env::temp_dir().join(format!(
        "sophia-partial-projection-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut fixture = PartialFixture::new(binary, &directory);
    let chrome = sophia_engine::SurfaceChromeStyle::default();

    let left_surface = SurfaceId::new(11, 1);
    let right_surface = SurfaceId::new(21, 1);
    let mut layout = PersistentLiveLayout::default();
    layout
        .layers
        .insert(left_surface, cached_layer(left_surface, left_bounds()));
    layout
        .layers
        .insert(right_surface, cached_layer(right_surface, right_bounds()));

    // A full cycle first, installed. Until a commit has placed them, the layers
    // carry no output owner and the guard a partial cycle has to satisfy is not
    // yet armed -- the regression would pass for the wrong reason.
    let seeded = run(
        &mut fixture,
        &layout,
        vec![LEFT, RIGHT],
        PolicyRequestCause::SceneChanged,
        LiveWmProposalSource::Relayout,
    );
    install(&mut layout, &seeded);
    for surface in [left_surface, right_surface] {
        assert!(
            layout.layers[&surface].output.is_some(),
            "the opening full cycle has to give every layer its output owner"
        );
    }

    for (output, target, area) in [
        (LEFT, left_surface, left_bounds()),
        (RIGHT, right_surface, right_bounds()),
    ] {
        let untouched = if output == LEFT {
            right_surface
        } else {
            left_surface
        };
        if output == RIGHT {
            // Activation is a full cycle's business. Naming only the right
            // output while the left one is active would be an invalid request,
            // so the real Hagia is asked to move focus there first.
            let moved = run(
                &mut fixture,
                &layout,
                vec![LEFT, RIGHT],
                PolicyRequestCause::PointerFocus {
                    output: RIGHT,
                    target: Some(right_surface),
                },
                LiveWmProposalSource::Focus(right_surface),
            );
            install(&mut layout, &moved);
        }

        for kind in [PolicyInteractionKind::Move, PolicyInteractionKind::Resize] {
            for phase in [PolicyInteractionPhase::Begin, PolicyInteractionPhase::End] {
                let geometry = Rect {
                    x: area.x + 64,
                    y: area.y + 48,
                    width: 400,
                    height: 300,
                };
                let before = layout.layers[&untouched].clone();
                let live = run(
                    &mut fixture,
                    &layout,
                    vec![output],
                    PolicyRequestCause::Interaction {
                        phase,
                        kind,
                        axis: PolicyInteractionAxis::None,
                        target,
                        geometry,
                    },
                    LiveWmProposalSource::PointerGesture {
                        surface: target,
                        mode: match kind {
                            PolicyInteractionKind::Resize => WmPointerGestureMode::Resize,
                            _ => WmPointerGestureMode::Move,
                        },
                    },
                );

                let kept = live
                    .layers
                    .iter()
                    .find(|layer| layer.surface == untouched)
                    .unwrap_or_else(|| {
                        panic!("{kind:?} {phase:?} on {output:?} dropped the other monitor")
                    });
                assert_eq!(
                    kept, &before,
                    "an output the request never named keeps the layer it had committed"
                );
                assert!(
                    !live.requested_sizes.contains_key(&untouched),
                    "an untouched surface is not reconfigured by a drag on another monitor"
                );
                let dragged = live
                    .layers
                    .iter()
                    .find(|layer| layer.surface == target)
                    .expect("the dragged window is placed");
                assert_eq!(
                    dragged.geometry,
                    sophia_engine::content_surface_geometry(geometry, chrome).unwrap(),
                    "the gesture's geometry is what the drag installs"
                );
                install(&mut layout, &live);
            }
        }
    }

    // Narrowing is the request's to choose. A cause naming both outputs still
    // replaces both.
    let full = run(
        &mut fixture,
        &layout,
        vec![LEFT, RIGHT],
        PolicyRequestCause::SceneChanged,
        LiveWmProposalSource::Relayout,
    );
    for surface in [left_surface, right_surface] {
        assert!(
            full.layers.iter().any(|layer| layer.surface == surface),
            "a request naming both outputs materializes both"
        );
    }
    drop(fixture);
    let _ = std::fs::remove_dir_all(&directory);
}

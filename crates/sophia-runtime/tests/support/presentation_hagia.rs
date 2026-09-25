//! Paired t241/h002 control: the compiled Hagia (SOPHIA_HAGIA_BIN) over the
//! real PolicyWmSessionTransport against the Engine policy reducer, with the
//! generic WM presentation capabilities negotiated as a mechanism fixture.
//! Receipts here are explicit transport fixtures, not evidence of an actual
//! presented frame; completion from retired frames belongs to the session.
//! Action ids are found by their catalog names; Sophia production code
//! carries no overview semantics.

use super::*;
use sophia_engine::PolicyProjectionReducer;
use sophia_protocol::{
    PolicyPresentation, PolicyPresentationIdentity, PolicyPresentationMode,
    PolicyPresentationOutcome, PolicyPresentationReceipt, PolicyProjectionProposal,
    SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS, SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES, WmActionId,
    validate_policy_presentation_shape,
};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};

const OVERVIEW_ACTIONS: [&str; 9] = [
    "toggle-overview",
    "close-overview",
    "overview-confirm",
    "overview-left",
    "overview-right",
    "overview-up",
    "overview-down",
    "overview-workspace-prev",
    "overview-workspace-next",
];

struct HagiaPolicy {
    transport: PolicyWmSessionTransport,
    child: Child,
    directory: std::path::PathBuf,
    epoch: u64,
    catalog: BTreeMap<String, WmActionId>,
    next_transaction: u64,
}

impl Drop for HagiaPolicy {
    fn drop(&mut self) {
        let _ = self.transport.disconnect();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

enum Settle {
    Commit,
    TimeOut,
}

impl HagiaPolicy {
    /// Starts Hagia on a fresh socket under `epoch` and settles its
    /// configuration, recording the action catalog by name.
    fn start(binary: &std::ffi::OsStr, epoch: u64) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-hagia-presentation-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = PolicyWmSessionTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let profile = directory.join("desktop.kdl");
        std::fs::write(
            &profile,
            "schema 1\npolicy { focus-follows-mouse #false; }\n",
        )
        .unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut command = Command::new(binary);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("SOPHIA_")
                || name.to_string_lossy().starts_with("HAGIA_")
            {
                command.env_remove(name);
            }
        }
        let child = command
            .arg(format!("--config={}", profile.display()))
            .arg(format!("--socket={}", transport.socket_path().display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        transport.authorize_supervised_pid(child.id()).unwrap();
        transport
            .accept_and_negotiate(epoch, Duration::from_secs(4))
            .unwrap();
        let required =
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
        assert_eq!(
            transport.selected_capabilities() & required,
            required,
            "the fixture negotiates the generic presentation mechanism"
        );
        let sophia_runtime::PolicyClientEvent::Configuration {
            transaction,
            configuration,
        } = transport
            .receive_client_event_within(Duration::from_secs(4))
            .unwrap()
        else {
            panic!("missing configuration");
        };
        let catalog = configuration
            .actions
            .iter()
            .map(|registration| (registration.name.clone(), registration.action))
            .collect::<BTreeMap<_, _>>();
        for name in OVERVIEW_ACTIONS {
            let registration = configuration
                .actions
                .iter()
                .find(|registration| registration.name == name)
                .unwrap_or_else(|| panic!("{name} is missing from the negotiated catalog"));
            assert_eq!(
                registration.session_operation_slot, None,
                "{name} is a pure policy action"
            );
        }
        transport
            .send_configuration_outcome(
                transaction,
                configuration.generation,
                PolicyProjectionOutcome::Committed,
            )
            .unwrap();
        Self {
            transport,
            child,
            directory,
            epoch,
            catalog,
            next_transaction: 100,
        }
    }

    fn action(&self, name: &str) -> WmActionId {
        self.catalog[name]
    }

    fn transaction(&mut self) -> TransactionId {
        self.next_transaction += 1;
        TransactionId::from_raw(self.next_transaction)
    }

    /// One policy cycle: snapshot, request with `cause`, Hagia's proposal
    /// staged by the reducer, then committed or timed out and settled.
    fn cycle(
        &mut self,
        reducer: &mut PolicyProjectionReducer,
        cause: PolicyRequestCause,
        settle: Settle,
    ) -> PolicyProjectionProposal {
        let outputs = reducer
            .scene()
            .outputs
            .iter()
            .map(|output| output.output)
            .collect::<Vec<_>>();
        let request = reducer.issue_request_with_cause(outputs, cause).unwrap();
        let snapshot_transaction = self.transaction();
        let snapshot = encode_wm_v1_policy_snapshot(
            snapshot_transaction,
            self.epoch,
            reducer.scene(),
            &[],
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
        let request_transaction = self.transaction();
        self.transport
            .send_projection_request(request_transaction, &request)
            .unwrap();
        let proposal = loop {
            match self
                .transport
                .receive_client_event_within(Duration::from_secs(4))
                .unwrap()
            {
                sophia_runtime::PolicyClientEvent::Projection(
                    QueuedPolicyProjection::Admitted(transfer),
                ) => break decode_wm_v1_policy_projection(&transfer.into_wire_transfer()).unwrap(),
                sophia_runtime::PolicyClientEvent::ProjectionPending => {}
                _ => panic!("unexpected client event"),
            }
        };
        if let Some(presentation) = &proposal.presentation {
            validate_policy_presentation_shape(presentation).unwrap();
        }
        let staged = reducer.stage_proposal(&proposal).unwrap();
        let outcome = match settle {
            Settle::Commit => reducer.commit_staged(staged),
            Settle::TimeOut => reducer.timeout(request.request_id),
        };
        assert_eq!(
            outcome,
            match settle {
                Settle::Commit => PolicyProjectionOutcome::Committed,
                Settle::TimeOut => PolicyProjectionOutcome::TimedOut,
            }
        );
        self.transport
            .send_projection_outcome(
                proposal.transaction,
                request.request_id,
                reducer.scene().generation,
                outcome,
            )
            .unwrap();
        proposal
    }

    /// A receipt fixture for one output of `presentation`.
    fn receipt(&mut self, presentation: &PolicyPresentation, outcome: PolicyPresentationOutcome) {
        let output = presentation.outputs[0];
        let transaction = self.transaction();
        self.transport
            .send_presentation_receipt(
                transaction,
                PolicyPresentationReceipt {
                    connection_epoch: self.epoch,
                    publication_generation: presentation.generation,
                    output: output.output,
                    output_generation: output.generation,
                    presentation_epoch: 1,
                    outcome,
                },
            )
            .unwrap();
    }
}

/// Two outputs, one toplevel on each.
fn two_output_scene() -> PolicySceneSnapshot {
    let mut scene = scene();
    let left = scene.outputs[0];
    let mut right = left;
    right.output = OutputId::from_raw(2);
    right.bounds.x = left.bounds.width;
    right.work_area = right.bounds;
    let mut window = scene.surfaces[0];
    window.surface = SurfaceId::new(4, 1);
    window.current_output = Some(right.output);
    window.geometry.x += left.bounds.width;
    right.focus = Some(window.surface);
    scene.outputs.push(right);
    scene.surfaces.push(window);
    scene
}

fn keyboard_identity(presentation: &PolicyPresentation) -> PolicyPresentationIdentity {
    let output = presentation
        .keyboard_output
        .expect("the overview names its keyboard output");
    PolicyPresentationIdentity {
        publication_generation: presentation.generation,
        output,
        output_generation: presentation
            .outputs
            .iter()
            .find(|record| record.output == output)
            .unwrap()
            .generation,
        presentation_epoch: 1,
        target_id: 0,
        target_generation: 0,
    }
}

/// The negotiated catalog carries the overview's pure actions; toggling
/// publishes valid generic records on every output without moving any
/// client; a repaint cycle republishes the same identities; a keyboard
/// action and a pointer activation, each naming exactly the published
/// identity, are accepted, and confirming closes the presentation.
#[test]
fn hagia_overview_publishes_generic_records_and_accepts_exact_targeted_actions() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    let mut hagia = HagiaPolicy::start(&binary, 1);
    let mut reducer = PolicyProjectionReducer::new(two_output_scene()).unwrap();
    reducer.connect(1).unwrap();
    let ordinary = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    assert!(ordinary.presentation.is_none());
    let geometry_before = reducer.scene().surfaces.clone();

    let toggle = hagia.action("toggle-overview");
    let opened = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::Action {
            activation_serial: 1,
            action: toggle,
        },
        Settle::Commit,
    );
    let presentation = opened.presentation.clone().expect("toggle publishes");
    assert_eq!(
        opened.outputs, ordinary.outputs,
        "no client is moved or resized"
    );
    assert_eq!(reducer.scene().surfaces, geometry_before);
    assert_eq!(
        presentation.outputs.len(),
        reducer.scene().outputs.len(),
        "every output"
    );
    for output in &reducer.scene().outputs {
        let record = presentation
            .outputs
            .iter()
            .find(|record| record.output == output.output)
            .expect("an output record per output");
        assert_eq!(record.generation, output.generation);
        if record.mode == PolicyPresentationMode::ReplaceApplications {
            assert_eq!(record.coverage, output.bounds);
        }
    }
    let surfaces = reducer
        .scene()
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<Vec<_>>();
    assert!(!presentation.instances.is_empty());
    assert!(
        presentation
            .instances
            .iter()
            .all(|instance| surfaces.contains(&instance.source))
    );
    assert_eq!(reducer.presentation_publication(), Some((1, &presentation)));

    // A repaint cycle: nothing structural changed, so nothing is renumbered.
    let repainted = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    assert_eq!(repainted.presentation.as_ref(), Some(&presentation));

    // A keyboard action naming exactly the published identity.
    let right = hagia.action("overview-right");
    assert!(
        presentation
            .bindings
            .iter()
            .any(|binding| binding.action == right)
    );
    let moved = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::PresentationAction {
            activation_serial: 2,
            action: right,
            identity: keyboard_identity(&presentation),
        },
        Settle::Commit,
    );
    let moved = moved
        .presentation
        .expect("navigation keeps the overview open");
    assert!(moved.generation >= presentation.generation);

    // A pointer activation naming exactly one published target.
    let confirm = hagia.action("overview-confirm");
    let target = moved
        .instances
        .iter()
        .find(|instance| instance.action == Some(confirm))
        .expect("a focusable preview is a confirm target");
    let output_generation = moved
        .outputs
        .iter()
        .find(|record| record.output == target.output)
        .unwrap()
        .generation;
    let confirmed = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::PresentationAction {
            activation_serial: 3,
            action: confirm,
            identity: PolicyPresentationIdentity {
                publication_generation: moved.generation,
                output: target.output,
                output_generation,
                presentation_epoch: 1,
                target_id: target.id,
                target_generation: target.generation,
            },
        },
        Settle::Commit,
    );
    assert!(
        confirmed.presentation.is_none(),
        "confirming closes the overview"
    );
    assert_eq!(reducer.presentation_publication(), None);
}

/// A publication whose proposal times out never becomes authoritative: the
/// reducer keeps the committed one, and Hagia, rolled back to its committed
/// state, republishes exactly that on the next cycle.
#[test]
fn hagia_overview_timeout_retains_the_committed_publication() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    let mut hagia = HagiaPolicy::start(&binary, 1);
    let mut reducer = PolicyProjectionReducer::new(two_output_scene()).unwrap();
    reducer.connect(1).unwrap();
    hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    let toggle = hagia.action("toggle-overview");
    let committed = hagia
        .cycle(
            &mut reducer,
            PolicyRequestCause::Action {
                activation_serial: 1,
                action: toggle,
            },
            Settle::Commit,
        )
        .presentation
        .expect("toggle publishes");
    let right = hagia.action("overview-right");
    hagia.cycle(
        &mut reducer,
        PolicyRequestCause::PresentationAction {
            activation_serial: 2,
            action: right,
            identity: keyboard_identity(&committed),
        },
        Settle::TimeOut,
    );
    assert_eq!(reducer.presentation_publication(), Some((1, &committed)));
    let next = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    assert_eq!(next.presentation.as_ref(), Some(&committed));
}

/// A Revoked receipt for the current publication closes it on the next
/// cycle, and the reducer's local revocation removes the authority at once.
#[test]
fn hagia_overview_revoked_receipt_closes_on_the_next_cycle() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    let mut hagia = HagiaPolicy::start(&binary, 1);
    let mut reducer = PolicyProjectionReducer::new(two_output_scene()).unwrap();
    reducer.connect(1).unwrap();
    hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    let toggle = hagia.action("toggle-overview");
    let presentation = hagia
        .cycle(
            &mut reducer,
            PolicyRequestCause::Action {
                activation_serial: 1,
                action: toggle,
            },
            Settle::Commit,
        )
        .presentation
        .expect("toggle publishes");
    hagia.receipt(&presentation, PolicyPresentationOutcome::Presented);
    reducer.revoke_presentation();
    assert_eq!(reducer.presentation_publication(), None);
    let right = hagia.action("overview-right");
    assert!(
        reducer
            .issue_request_with_cause(
                reducer
                    .scene()
                    .outputs
                    .iter()
                    .map(|output| output.output)
                    .collect(),
                PolicyRequestCause::PresentationAction {
                    activation_serial: 2,
                    action: right,
                    identity: keyboard_identity(&presentation),
                },
            )
            .is_err(),
        "a revoked publication's identity is refused locally"
    );
    hagia.receipt(&presentation, PolicyPresentationOutcome::Revoked);
    let next = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    assert!(
        next.presentation.is_none(),
        "the revoked overview is closed"
    );
}

/// A new connection epoch starts with no publication; the old epoch's
/// identities carry no authority while none is published; a restarted WM
/// begins closed; the new publication is qualified by the new epoch.
#[test]
fn hagia_overview_reconnect_starts_closed_and_reuses_no_authority() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    let mut reducer = PolicyProjectionReducer::new(two_output_scene()).unwrap();
    reducer.connect(1).unwrap();
    let stale = {
        let mut hagia = HagiaPolicy::start(&binary, 1);
        hagia.cycle(
            &mut reducer,
            PolicyRequestCause::SceneChanged,
            Settle::Commit,
        );
        let toggle = hagia.action("toggle-overview");
        let presentation = hagia
            .cycle(
                &mut reducer,
                PolicyRequestCause::Action {
                    activation_serial: 1,
                    action: toggle,
                },
                Settle::Commit,
            )
            .presentation
            .expect("toggle publishes");
        (
            hagia.action("overview-right"),
            keyboard_identity(&presentation),
        )
    };
    reducer.disconnect(1);
    assert_eq!(reducer.presentation_publication(), None);
    reducer.connect(2).unwrap();
    let stale_cause = PolicyRequestCause::PresentationAction {
        activation_serial: 9,
        action: stale.0,
        identity: stale.1,
    };
    assert!(
        reducer
            .issue_request_with_cause(vec![stale.1.output], stale_cause)
            .is_err(),
        "an old epoch's identity has no publication to act on"
    );
    let mut hagia = HagiaPolicy::start(&binary, 2);
    let first = hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    assert!(first.presentation.is_none(), "a restarted WM begins closed");
    let toggle = hagia.action("toggle-overview");
    let reopened = hagia
        .cycle(
            &mut reducer,
            PolicyRequestCause::Action {
                activation_serial: 10,
                action: toggle,
            },
            Settle::Commit,
        )
        .presentation
        .expect("toggle publishes again");
    let (epoch, published) = reducer.presentation_publication().unwrap();
    assert_eq!(
        (epoch, published),
        (2, &reopened),
        "qualified by the new epoch"
    );
    // Reuse by coinciding numbers is the open gap pinned (ignored, red) by
    // `hagia_overview_old_epoch_identity_is_refused_after_reconnect`.
    let _ = stale_cause;
}

/// Boundary-only, and red by design at this layer, so ignored: the
/// reducer alone checks publication membership, not epochs. After a
/// reconnect the restarted WM's counters begin again at 1, the reopened
/// publication has the same generation, output and output generation as the
/// old one, and `PolicyPresentationIdentity` carries no connection epoch, so
/// a keyboard identity captured under epoch 1 is current for epoch 2 in the
/// reducer. The joined session admission closes it: `PresentedPolicyState`
/// keeps `next_presentation_epoch` across revocation and reconnect, so the
/// reopened publication gets a fresh presentation epoch, and
/// `PresentedPolicyAction` carries the authenticated connection epoch,
/// checked before queueing and again against the receipt epoch (t245, with
/// its own Engine control). This test documents the reducer boundary; it is
/// not a claim about the joined session, and must not be un-ignored as one.
#[test]
#[ignore = "boundary-only: the reducer alone does not distinguish epochs; joined session admission (t245) is required and closes it"]
fn hagia_overview_old_epoch_identity_is_refused_after_reconnect() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    let mut reducer = PolicyProjectionReducer::new(two_output_scene()).unwrap();
    reducer.connect(1).unwrap();
    let stale = {
        let mut hagia = HagiaPolicy::start(&binary, 1);
        hagia.cycle(
            &mut reducer,
            PolicyRequestCause::SceneChanged,
            Settle::Commit,
        );
        let toggle = hagia.action("toggle-overview");
        let presentation = hagia
            .cycle(
                &mut reducer,
                PolicyRequestCause::Action {
                    activation_serial: 1,
                    action: toggle,
                },
                Settle::Commit,
            )
            .presentation
            .expect("toggle publishes");
        PolicyRequestCause::PresentationAction {
            activation_serial: 2,
            action: hagia.action("overview-right"),
            identity: keyboard_identity(&presentation),
        }
    };
    reducer.disconnect(1);
    reducer.connect(2).unwrap();
    let mut hagia = HagiaPolicy::start(&binary, 2);
    hagia.cycle(
        &mut reducer,
        PolicyRequestCause::SceneChanged,
        Settle::Commit,
    );
    let toggle = hagia.action("toggle-overview");
    hagia.cycle(
        &mut reducer,
        PolicyRequestCause::Action {
            activation_serial: 3,
            action: toggle,
        },
        Settle::Commit,
    );
    assert!(
        !reducer.presentation_cause_is_current(stale),
        "an identity captured under the old epoch acts on the new publication"
    );
}

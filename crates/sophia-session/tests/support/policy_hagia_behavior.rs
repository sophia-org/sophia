//! Non-native behaviour coverage through the same frozen normal Hagia over
//! current IPC and over files: tabs, the scroller's translation group, pointer
//! interactions, floating and fullscreen with a stale refusal and retry, a
//! timed-out action and its continuation, views, output actions, topology
//! with and without workspace assignments, launch contexts and pointer focus.
//!
//! Evidence class: real Hagia proposals through the production workers and
//! adapters, with a test-owned canonical reducer deciding every outcome, as in
//! the revision-1 corpus. The scene follows each committed proposal the way
//! Session reports it (placed outputs and presentation, per-output focus and
//! the active output, with a new generation on any change); it is not LivePublicPolicyState layout or native
//! settlement. Launch contexts are worker inputs, not an authenticated X
//! child; that Session join is separate.
use super::corpus_parity::CorpusWorker;
use super::*;
use sophia_protocol::{
    LayoutNodeCapabilities, OutputId, PolicyInteractionAxis, PolicyInteractionKind,
    PolicyInteractionPhase, PolicyLaunchContext, PolicyOutputSnapshot, PolicyPresentationState,
    PolicyProjectionOutcome, PolicyProjectionProposal, PolicyRequestCause, PolicySceneSnapshot,
    PolicySurfaceKind, PolicySurfacePlacement, PolicySurfaceSnapshot, SurfaceConstraints,
};

const FIRST: OutputId = OutputId::from_raw(1);
const SECOND: OutputId = OutputId::from_raw(2);
const A: SurfaceId = SurfaceId::new(3, 1);
const B: SurfaceId = SurfaceId::new(4, 1);
const C: SurfaceId = SurfaceId::new(5, 1);
const CHILD: SurfaceId = SurfaceId::new(6, 1);

const POINTER_FOCUS_DESKTOP: &str = "schema 1\nshell { enabled #false; }\nsession { terminal \"terminal\"; browser \"brave-origin\"; startup \"panel\"; }\npolicy { focus-follows-mouse #true; }\n";

#[derive(Debug, PartialEq)]
struct Step {
    name: &'static str,
    proposal: PolicyProjectionProposal,
    outcome: PolicyProjectionOutcome,
}

fn output(output: OutputId, generation: u64, x: i32, key: u64) -> PolicyOutputSnapshot {
    let bounds = Rect {
        x,
        y: 0,
        width: 1200,
        height: 800,
    };
    PolicyOutputSnapshot {
        policy_key: Some(key),
        output,
        generation,
        focus: None,
        bounds,
        work_area: bounds,
    }
}

fn surface(surface: SurfaceId, output: OutputId) -> PolicySurfaceSnapshot {
    PolicySurfaceSnapshot {
        surface,
        generation: u64::from(surface.generation()),
        current_output: Some(output),
        kind: PolicySurfaceKind::Toplevel,
        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: SurfaceConstraints {
            min_size: Some(Size {
                width: 100,
                height: 80,
            }),
            max_size: None,
        },
        exact_size: None,
        requested_state: PolicyPresentationState::default(),
        current_state: PolicyPresentationState::default(),
        transient_owner: None,
        geometry: Rect {
            x: if output == SECOND { 1200 } else { 0 },
            y: 0,
            width: 600,
            height: 800,
        },
    }
}

/// Two outputs with stable policy keys; A and B on the first, C on the second.
fn initial_scene() -> PolicySceneSnapshot {
    PolicySceneSnapshot {
        generation: 1,
        active_output: FIRST,
        outputs: vec![output(FIRST, 1, 0, 101), output(SECOND, 1, 1200, 102)],
        surfaces: vec![surface(A, FIRST), surface(B, FIRST), surface(C, SECOND)],
        session_operations: Vec::new(),
    }
}

fn placement(
    proposal: &PolicyProjectionProposal,
    surface: SurfaceId,
) -> Option<(OutputId, PolicySurfacePlacement)> {
    proposal.outputs.iter().find_map(|output| {
        output
            .placements
            .iter()
            .find(|placement| placement.surface == surface)
            .map(|placement| (output.output, placement.clone()))
    })
}

fn focus(proposal: &PolicyProjectionProposal, output: OutputId) -> Option<SurfaceId> {
    proposal
        .outputs
        .iter()
        .find(|projection| projection.output == output)
        .and_then(|projection| projection.focus)
}

fn translated(proposal: &PolicyProjectionProposal, output: OutputId) -> BTreeSet<SurfaceId> {
    proposal
        .translation_groups
        .iter()
        .filter(|group| group.output == output)
        .flat_map(|group| group.members.iter().copied())
        .collect()
}

/// One stateful sequence. The scene is Session's report: after each commit
/// it takes the proposal's placed outputs and presentation, per-output focus
/// and active output, and any change is a new scene generation.
struct Sequence<'a> {
    corpus: &'a CorpusWorker,
    reducer: sophia_engine::PolicyProjectionReducer,
    scene: PolicySceneSnapshot,
    steps: Vec<Step>,
    serial: u64,
}

impl<'a> Sequence<'a> {
    fn new(corpus: &'a CorpusWorker) -> Self {
        let scene = initial_scene();
        let mut reducer = sophia_engine::PolicyProjectionReducer::new(scene.clone()).unwrap();
        reducer.connect(corpus.epoch).unwrap();
        Self {
            corpus,
            reducer,
            scene,
            steps: Vec::new(),
            serial: 500,
        }
    }

    /// Replaces the scene with a changed one under the next generation.
    fn change(&mut self, change: impl FnOnce(&mut PolicySceneSnapshot)) {
        let mut next = self.scene.clone();
        change(&mut next);
        next.generation = self.scene.generation + 1;
        self.scene = next;
    }

    fn follow(&mut self, proposal: &PolicyProjectionProposal) {
        let mut next = self.scene.clone();
        next.active_output = proposal.active_output;
        // Session reports each output's committed focus; Hagia clears focus
        // on an output whose snapshot names none.
        for output in &mut next.outputs {
            if let Some(projection) = proposal
                .outputs
                .iter()
                .find(|projection| projection.output == output.output)
            {
                output.focus = projection.focus;
            }
        }
        // Session also reports each placed surface's committed presentation;
        // Hagia re-applies it from every snapshot (currentStateBits).
        for surface in &mut next.surfaces {
            if let Some((on, placed)) = placement(proposal, surface.surface) {
                surface.current_output = Some(on);
                surface.current_state = placed.presentation;
            }
        }
        if next.active_output != self.scene.active_output
            || next.outputs != self.scene.outputs
            || next.surfaces != self.scene.surfaces
        {
            next.generation = self.scene.generation + 1;
            self.scene = next;
        }
    }

    fn commit(
        &mut self,
        name: &'static str,
        cause: PolicyRequestCause,
        launch_origins: Vec<PolicyLaunchContext>,
    ) -> PolicyProjectionProposal {
        let proposal = self.settle(
            name,
            cause,
            launch_origins,
            PolicyProjectionOutcome::Committed,
            |reducer, proposal| reducer.apply_proposal(proposal),
        );
        self.follow(&proposal);
        proposal
    }

    fn settle(
        &mut self,
        name: &'static str,
        cause: PolicyRequestCause,
        launch_origins: Vec<PolicyLaunchContext>,
        expected: PolicyProjectionOutcome,
        decide: impl FnOnce(
            &mut sophia_engine::PolicyProjectionReducer,
            &PolicyProjectionProposal,
        ) -> PolicyProjectionOutcome,
    ) -> PolicyProjectionProposal {
        let transaction = 1_000 + u64::try_from(self.steps.len()).unwrap() * 2;
        let scene = self.scene.clone();
        let (proposal, outcome) = self.corpus.cycle(
            name,
            &mut self.reducer,
            &scene,
            cause,
            transaction,
            launch_origins,
            decide,
        );
        assert_eq!(outcome, expected, "{name}: {proposal:?}");
        self.steps.push(Step {
            name,
            proposal: proposal.clone(),
            outcome,
        });
        proposal
    }

    fn action(&mut self, action: WmActionId) -> PolicyRequestCause {
        self.serial += 1;
        PolicyRequestCause::Action {
            activation_serial: self.serial,
            action,
        }
    }
}

fn catalog_action(wm: &LiveWmSession, name: &str) -> WmActionId {
    wm.public
        .as_ref()
        .unwrap()
        .actions
        .iter()
        .find(|registration| registration.name == name)
        .unwrap_or_else(|| panic!("configured Hagia catalog lacks {name}"))
        .action
}

fn interaction(
    phase: PolicyInteractionPhase,
    kind: PolicyInteractionKind,
    geometry: Rect,
) -> PolicyRequestCause {
    PolicyRequestCause::Interaction {
        phase,
        kind,
        axis: PolicyInteractionAxis::None,
        target: A,
        geometry,
    }
}

fn record(identity: &mut std::fs::File, label: &str, selected: u64, steps: &[Step]) {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{selected:#x}\n{steps:#?}").as_bytes())
    );
    writeln!(
        identity,
        "behavior_corpus={label} steps={}\nselected_capabilities={selected:#x}\nobservations_sha256={digest}\nsnapshot_vocabulary=host-exact (no actions, no session operations); configuration catalog admitted separately\nscene=follows committed placements and presentation, per-output focus and active output, new generation on change\nfinal_outcome=every covered outcome is followed by a real Hagia terminal proposal; the terminal cycle outcome itself is not claimed consumed\nreducer=test-owned canonical\ntopology=unassigned: Session re-homing is adopted, a returned output moves nothing back; assigned: an output re-identified under the same policy key keeps its view\nlaunch_context=worker input; not an authenticated X child\nlayout_settlement=false\nnative_settlement=false",
        steps.len()
    )
    .unwrap();
}

fn run_actions(case: &str, transport: WmTransportSelection) -> (u64, Vec<Step>) {
    with_normal_hagia_transport(case, transport, |wm, _, _, _, _, identity| {
        let names = [
            "focus-next",
            "layout-i3",
            "split-tree-layout-tabbed",
            "layout-scroller",
            "toggle-floating",
            "toggle-fullscreen",
            "move-to-view-next",
            "focus-view-next",
            "focus-view-prev",
            "move-to-output-next",
            "focus-output-next",
        ];
        let actions = names
            .iter()
            .map(|name| (*name, catalog_action(wm, name)))
            .collect::<BTreeMap<_, _>>();
        let corpus = CorpusWorker::take(wm);
        let selected = corpus.selected;
        for bit in [
            sophia_protocol::SOPHIA_WM_CAPABILITY_POINTER_INTERACTIONS,
            sophia_protocol::SOPHIA_WM_CAPABILITY_TAB_GROUPS,
            sophia_protocol::SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS,
            sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
            sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS,
        ] {
            assert_ne!(selected & bit, 0, "admitted selection lacks {bit:#x}");
        }
        let mut run = Sequence::new(&corpus);

        let baseline = run.commit("baseline", PolicyRequestCause::SceneChanged, Vec::new());
        assert_eq!(placement(&baseline, A).unwrap().0, FIRST);
        assert_eq!(placement(&baseline, B).unwrap().0, FIRST);
        assert_eq!(placement(&baseline, C).unwrap().0, SECOND);
        run.commit(
            "focus-a",
            PolicyRequestCause::Focus { target: A },
            Vec::new(),
        );

        // Tabs: a tabbed split tree groups the first output's windows, and the
        // chosen member follows focus.
        let cause = run.action(actions["layout-i3"]);
        run.commit("layout-i3", cause, Vec::new());
        let cause = run.action(actions["split-tree-layout-tabbed"]);
        let tabbed = run.commit("tabbed", cause, Vec::new());
        let group = tabbed
            .tab_groups
            .iter()
            .find(|group| group.output == FIRST)
            .expect("tabbed layout publishes a tab group on the first output");
        assert_eq!(
            group.members.iter().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from([A, B])
        );
        assert_eq!(
            group.selected,
            Some(A),
            "the focused window is the chosen tab"
        );
        assert_eq!(focus(&tabbed, FIRST), Some(A));
        let cause = run.action(actions["focus-next"]);
        let next_tab = run.commit("tab-focus-next", cause, Vec::new());
        let group = next_tab
            .tab_groups
            .iter()
            .find(|group| group.output == FIRST)
            .expect("tab group persists");
        assert_eq!(group.selected, Some(B), "the chosen tab follows focus");
        assert_eq!(focus(&next_tab, FIRST), Some(B));

        // The scroller's translation group: its camera translates every tiled
        // window on the output, and no tab group remains there.
        let cause = run.action(actions["layout-scroller"]);
        let scroller = run.commit("layout-scroller", cause, Vec::new());
        assert!(
            scroller
                .tab_groups
                .iter()
                .all(|group| group.output != FIRST)
        );
        assert_eq!(translated(&scroller, FIRST), BTreeSet::from([A, B]));

        // Pointer interactions float A at the interaction geometry and take it
        // out of the translated strip.
        let moved = Rect {
            x: 100,
            y: 120,
            width: 400,
            height: 300,
        };
        for (name, phase, geometry) in [
            (
                "move-begin",
                PolicyInteractionPhase::Begin,
                Rect {
                    x: 60,
                    y: 80,
                    ..moved
                },
            ),
            (
                "move-update",
                PolicyInteractionPhase::Update,
                Rect {
                    x: 80,
                    y: 100,
                    ..moved
                },
            ),
            ("move-end", PolicyInteractionPhase::End, moved),
        ] {
            let proposal = run.commit(
                name,
                interaction(phase, PolicyInteractionKind::Move, geometry),
                Vec::new(),
            );
            assert_eq!(
                placement(&proposal, A).unwrap().1.geometry,
                geometry,
                "{name}"
            );
            assert!(!translated(&proposal, FIRST).contains(&A), "{name}");
            assert!(translated(&proposal, FIRST).contains(&B), "{name}");
        }
        for (name, phase, geometry) in [
            ("resize-begin", PolicyInteractionPhase::Begin, moved),
            (
                "resize-update",
                PolicyInteractionPhase::Update,
                Rect {
                    width: 460,
                    height: 330,
                    ..moved
                },
            ),
            (
                "resize-end",
                PolicyInteractionPhase::End,
                Rect {
                    width: 520,
                    height: 360,
                    ..moved
                },
            ),
        ] {
            let proposal = run.commit(
                name,
                interaction(phase, PolicyInteractionKind::Resize, geometry),
                Vec::new(),
            );
            assert_eq!(
                placement(&proposal, A).unwrap().1.geometry,
                geometry,
                "{name}"
            );
        }

        // Floating on and off for B.
        run.commit(
            "focus-b",
            PolicyRequestCause::Focus { target: B },
            Vec::new(),
        );
        let cause = run.action(actions["toggle-floating"]);
        let floated = run.commit("float-b", cause, Vec::new());
        assert!(!translated(&floated, FIRST).contains(&B));
        let cause = run.action(actions["toggle-floating"]);
        let tiled = run.commit("tile-b", cause, Vec::new());
        assert!(translated(&tiled, FIRST).contains(&B));

        // Fullscreen, a stale refusal of the toggle back, then its retry.
        let cause = run.action(actions["toggle-fullscreen"]);
        let full = run.commit("fullscreen-b", cause, Vec::new());
        let (on, placed) = placement(&full, B).unwrap();
        assert_eq!(on, FIRST);
        assert!(placed.presentation.fullscreen);
        assert_eq!(placed.geometry, run.scene.outputs[0].bounds);
        let mut successor = run.scene.clone();
        successor.generation += 1;
        let refused_successor = successor.clone();
        let cause = run.action(actions["toggle-fullscreen"]);
        run.settle(
            "unfullscreen-stale",
            cause,
            Vec::new(),
            PolicyProjectionOutcome::RejectedStale,
            move |reducer, proposal| {
                reducer.observe_scene(refused_successor).unwrap();
                reducer.apply_proposal(proposal)
            },
        );
        run.scene = successor;
        let still = run.commit("after-stale", PolicyRequestCause::SceneChanged, Vec::new());
        assert!(
            placement(&still, B).unwrap().1.presentation.fullscreen,
            "a refused toggle leaves B fullscreen"
        );
        let cause = run.action(actions["toggle-fullscreen"]);
        let retried = run.commit("unfullscreen-retry", cause, Vec::new());
        assert!(!placement(&retried, B).unwrap().1.presentation.fullscreen);
        assert!(translated(&retried, FIRST).contains(&B));

        // A timed-out action changes nothing that the next cycle keeps.
        let before = focus(&retried, FIRST);
        let cause = run.action(actions["focus-next"]);
        run.settle(
            "focus-next-timeout",
            cause,
            Vec::new(),
            PolicyProjectionOutcome::TimedOut,
            |reducer, proposal| reducer.timeout(proposal.request_id),
        );
        let continued = run.commit(
            "after-timeout",
            PolicyRequestCause::SceneChanged,
            Vec::new(),
        );
        assert_eq!(
            focus(&continued, FIRST),
            before,
            "a timed-out action leaves focus unchanged"
        );

        // Views: moving B to the next view follows it there (Hagia's
        // moveFocusedToViewRelative activates the target view and focuses the
        // window), leaving A on the first view; the view switches show each.
        let cause = run.action(actions["move-to-view-next"]);
        let moved_view = run.commit("move-b-to-view-next", cause, Vec::new());
        assert_eq!(
            placement(&moved_view, B).unwrap().0,
            FIRST,
            "the view followed B"
        );
        assert_eq!(focus(&moved_view, FIRST), Some(B));
        assert!(
            placement(&moved_view, A).is_none(),
            "A stayed on the first view"
        );
        let cause = run.action(actions["focus-view-prev"]);
        let first_view = run.commit("focus-view-prev", cause, Vec::new());
        assert_eq!(placement(&first_view, A).unwrap().0, FIRST);
        assert!(placement(&first_view, B).is_none(), "B is on the next view");
        let cause = run.action(actions["focus-view-next"]);
        let next_view = run.commit("focus-view-next", cause, Vec::new());
        assert_eq!(placement(&next_view, B).unwrap().0, FIRST);
        assert!(placement(&next_view, A).is_none());
        let cause = run.action(actions["focus-view-prev"]);
        let back = run.commit("focus-view-prev-again", cause, Vec::new());
        assert_eq!(placement(&back, A).unwrap().0, FIRST);

        // Output actions: the focused window moves outputs, then focus follows.
        let focused = focus(&back, FIRST).expect("the first output has focus");
        let cause = run.action(actions["move-to-output-next"]);
        let moved_output = run.commit("move-to-output-next", cause, Vec::new());
        assert_eq!(placement(&moved_output, focused).unwrap().0, SECOND);
        let cause = run.action(actions["focus-output-next"]);
        let switched = run.commit("focus-output-next", cause, Vec::new());
        assert_ne!(switched.active_output, moved_output.active_output);

        // Topology without workspace assignments: Session re-homes the lost
        // output's windows onto the first, and Hagia adopts a reported
        // current_output as the window's home (adoptWindowOutput, from
        // reconcile). Outputs match by handle only, so the returned output is
        // not a reason to move anything back.
        run.change(|scene| {
            scene.outputs.retain(|output| output.output != SECOND);
            scene.active_output = FIRST;
            for surface in &mut scene.surfaces {
                if surface.current_output == Some(SECOND) {
                    surface.current_output = Some(FIRST);
                }
            }
        });
        let lost = run.commit("output-lost", PolicyRequestCause::SceneChanged, Vec::new());
        assert!(lost.outputs.iter().all(|output| output.output != SECOND));
        assert_eq!(placement(&lost, C).unwrap().0, FIRST);
        run.change(|scene| scene.outputs.push(output(SECOND, 2, 1200, 102)));
        let returned = run.commit(
            "output-returned",
            PolicyRequestCause::SceneChanged,
            Vec::new(),
        );
        assert!(
            returned
                .outputs
                .iter()
                .any(|output| output.output == SECOND)
        );
        assert_eq!(
            placement(&returned, C).unwrap().0,
            FIRST,
            "C keeps its adopted home"
        );
        assert_eq!(placement(&returned, focused).unwrap().0, FIRST);

        // Launch context: real actions put C back on the second output and
        // the first output back in charge; a child launched from C then opens
        // on C's output, not the active one.
        run.commit(
            "focus-c",
            PolicyRequestCause::Focus { target: C },
            Vec::new(),
        );
        let cause = run.action(actions["move-to-output-next"]);
        let c_moved = run.commit("move-c-to-second", cause, Vec::new());
        assert_eq!(placement(&c_moved, C).unwrap().0, SECOND);
        let refocused = run.commit(
            "focus-a-again",
            PolicyRequestCause::Focus { target: A },
            Vec::new(),
        );
        assert_eq!(refocused.active_output, FIRST);
        assert_eq!(placement(&refocused, C).unwrap().0, SECOND);
        let context = refocused
            .launch_contexts
            .iter()
            .find(|context| context.surface == C)
            .copied()
            .expect("Hagia publishes a launch context for C");
        assert_eq!(context.epoch, corpus.epoch);
        run.change(|scene| scene.surfaces.push(surface(CHILD, FIRST)));
        assert_eq!(run.scene.active_output, FIRST);
        let child = run.commit(
            "launch-from-c",
            PolicyRequestCause::SceneChanged,
            vec![PolicyLaunchContext {
                surface: CHILD,
                epoch: context.epoch,
                token: context.token,
            }],
        );
        assert_eq!(
            placement(&child, CHILD).map(|(on, _)| on),
            Some(SECOND),
            "the child opens at its launcher's output"
        );

        // Hagia answering one more cycle proves it consumed every covered
        // outcome; this terminal cycle's own outcome is not claimed consumed.
        let terminal = run.commit("terminal", PolicyRequestCause::SceneChanged, Vec::new());
        assert_eq!(placement(&terminal, CHILD).map(|(on, _)| on), Some(SECOND));
        assert!(wm.supervisor.peer_id().is_some());
        record(identity, "actions", selected, &run.steps);
        let steps = std::mem::take(&mut run.steps);
        drop(run);
        drop(corpus);
        (selected, steps)
    })
}

fn run_pointer_focus(case: &str, transport: WmTransportSelection) -> (u64, Vec<Step>) {
    with_normal_hagia_transport_desktop(
        case,
        transport,
        Some(POINTER_FOCUS_DESKTOP),
        |wm, _, _, _, _, identity| {
            let corpus = CorpusWorker::take(wm);
            let selected = corpus.selected;
            assert_ne!(
                selected & sophia_protocol::SOPHIA_WM_CAPABILITY_POINTER_FOCUS,
                0,
                "the focus-follows-mouse profile selects pointer focus"
            );
            let mut run = Sequence::new(&corpus);
            run.commit("baseline", PolicyRequestCause::SceneChanged, Vec::new());
            let hovered = run.commit(
                "hover-c",
                PolicyRequestCause::PointerFocus {
                    output: SECOND,
                    target: Some(C),
                },
                Vec::new(),
            );
            assert_eq!(hovered.active_output, SECOND);
            assert_eq!(focus(&hovered, SECOND), Some(C));
            let empty = run.commit(
                "hover-empty-first",
                PolicyRequestCause::PointerFocus {
                    output: FIRST,
                    target: None,
                },
                Vec::new(),
            );
            assert_eq!(empty.active_output, FIRST);
            let hovered = run.commit(
                "hover-b",
                PolicyRequestCause::PointerFocus {
                    output: FIRST,
                    target: Some(B),
                },
                Vec::new(),
            );
            assert_eq!(focus(&hovered, FIRST), Some(B));
            // Consumption proof for the last covered outcome, as above.
            let terminal = run.commit("terminal", PolicyRequestCause::SceneChanged, Vec::new());
            assert_eq!(focus(&terminal, FIRST), Some(B));
            record(identity, "pointer-focus", selected, &run.steps);
            let steps = std::mem::take(&mut run.steps);
            drop(run);
            drop(corpus);
            (selected, steps)
        },
    )
}

const ASSIGNED_DESKTOP: &str = "schema 1\nshell { enabled #false; }\nsession { terminal \"terminal\"; browser \"brave-origin\"; startup \"panel\"; }\npolicy { workspace 1 output-key=101; workspace 2 output-key=102; }\n";

fn run_assignment(case: &str, transport: WmTransportSelection) -> (u64, Vec<Step>) {
    with_normal_hagia_transport_desktop(
        case,
        transport,
        Some(ASSIGNED_DESKTOP),
        |wm, _, _, _, _, identity| {
            let corpus = CorpusWorker::take(wm);
            let selected = corpus.selected;
            let outputs = sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS
                | sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS;
            assert_eq!(
                selected & outputs,
                outputs,
                "assignments select output keys"
            );
            let mut run = Sequence::new(&corpus);
            let baseline = run.commit("baseline", PolicyRequestCause::SceneChanged, Vec::new());
            assert_eq!(placement(&baseline, C).unwrap().0, SECOND);
            let view = baseline
                .translation_groups
                .iter()
                .find(|group| group.output == SECOND)
                .expect("the second output's scroller view is translated")
                .group;
            // Session re-identifies the second output under a new id with the
            // same policy key, and reports C on it. With assignments Hagia
            // matches outputs by key (reconcile), so the same logical output
            // and its view carry on; the translation group id is the view id.
            let renamed = OutputId::from_raw(3);
            run.change(|scene| {
                scene.outputs.retain(|output| output.output != SECOND);
                scene.outputs.push(output(renamed, 1, 1200, 102));
                for surface in &mut scene.surfaces {
                    if surface.current_output == Some(SECOND) {
                        surface.current_output = Some(renamed);
                    }
                }
            });
            let moved = run.commit(
                "output-renamed",
                PolicyRequestCause::SceneChanged,
                Vec::new(),
            );
            assert_eq!(placement(&moved, C).unwrap().0, renamed);
            let group = moved
                .translation_groups
                .iter()
                .find(|group| group.output == renamed)
                .expect("the renamed output keeps a translated view");
            assert_eq!(group.group, view, "the policy key kept the same view");
            assert!(group.members.contains(&C));
            // Consumption proof for the last covered outcome, as above.
            let terminal = run.commit("terminal", PolicyRequestCause::SceneChanged, Vec::new());
            assert_eq!(placement(&terminal, C).unwrap().0, renamed);
            record(identity, "assignment", selected, &run.steps);
            let steps = std::mem::take(&mut run.steps);
            drop(run);
            drop(corpus);
            (selected, steps)
        },
    )
}

fn assert_same(label: &str, ipc: (u64, Vec<Step>), files: (u64, Vec<Step>)) {
    assert_eq!(
        ipc.0, files.0,
        "{label}: transports admitted different selections"
    );
    assert_eq!(ipc.1.len(), files.1.len(), "{label}");
    for (ipc, files) in ipc.1.iter().zip(&files.1) {
        assert_eq!(ipc, files, "{label}: transport changed step {}", ipc.name);
    }
    eprintln!(
        "hagia_behavior_coverage_parity corpus={label} steps={} selected_capabilities={:#x} observations_sha256={:x} status=pass layout_settlement=false native_settlement=false",
        ipc.1.len(),
        ipc.0,
        Sha256::digest(format!("{:#x}\n{:#?}", ipc.0, ipc.1).as_bytes())
    );
}

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn normal_hagia_behavior_coverage_matches_over_current_ipc_and_files() {
    let ipc = run_actions("behavior-ipc", WmTransportSelection::CurrentIpc);
    let files = run_actions("behavior-files", WmTransportSelection::NineP2000L);
    assert_same("actions", ipc, files);
    let ipc = run_assignment("assignment-ipc", WmTransportSelection::CurrentIpc);
    let files = run_assignment("assignment-files", WmTransportSelection::NineP2000L);
    assert_same("assignment", ipc, files);
    let ipc = run_pointer_focus("pointer-focus-ipc", WmTransportSelection::CurrentIpc);
    let files = run_pointer_focus("pointer-focus-files", WmTransportSelection::NineP2000L);
    assert_same("pointer-focus", ipc, files);
}

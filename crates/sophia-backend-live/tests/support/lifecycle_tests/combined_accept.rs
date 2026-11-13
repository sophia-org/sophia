//! G3-4: the visual runtime's half of the combined accept (AUTHORITY-STEP3
//! F2 phase 1 and step 1) on the G3-3 fixture, with a recording native half.
//! The setter's preflight and the retirement-claim checks run before the
//! native preflight and refuse without writing; a native refusal leaves the
//! runtime as it was; the commit installs the candidate and binds its claim
//! to the plan's native frame without queueing a render.
use super::super::super::compositor_graphics::CombinedAcceptError;
use super::*;
use crate::{
    CombinedAccept, CombinedAcceptNative, CombinedAcceptRefusal, LiveProductionNativeFrameId,
};
use std::cell::RefCell;

const FRAME: u64 = 51;
const JOB: u64 = 7;

/// The native half: answers as told and records what it was asked.
struct Native {
    service: bool,
    refusal: Option<CombinedAcceptRefusal>,
    calls: RefCell<Vec<&'static str>>,
}

impl Native {
    fn new() -> Self {
        Self {
            service: true,
            refusal: None,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl CombinedAcceptNative for Native {
    type Head = ();
    type Plan = u64;

    fn frame_service_available(&self) -> bool {
        self.calls.borrow_mut().push("frame_service");
        self.service
    }

    fn check_combined_accept(
        &self,
        _head: &(),
        _accept: &CombinedAccept,
    ) -> Result<(), CombinedAcceptRefusal> {
        // The declared refusal stays the native preflight's, as before.
        self.calls.borrow_mut().push("check");
        Ok(())
    }

    fn preflight_combined_accept(
        &self,
        _head: &(),
        accept: CombinedAccept,
    ) -> Result<u64, CombinedAcceptRefusal> {
        self.calls.borrow_mut().push("preflight");
        self.refusal.map_or(Ok(accept.job), Err)
    }

    fn commit_combined_accept(&mut self, _head: &(), plan: u64) {
        assert_eq!(plan, JOB, "the commit consumes its own preflight's plan");
        self.calls.get_mut().push("commit");
    }
}

fn accept(output: HeadlessOutput) -> CombinedAccept {
    CombinedAccept {
        job: JOB,
        frame: LiveProductionNativeFrameId::from_raw(FRAME),
        checksum: FRAME,
        snapshot: sophia_engine::OutputFrameDamageSnapshot {
            damage_history: Default::default(),
            output,
            surfaces: Vec::new(),
            compositor_display_list: sophia_engine::CompositorDamageList::empty(output.id),
            software_cursor: None,
        },
        correlation: crate::LiveRendererFrameCorrelation {
            native: None,
            request: None,
            trace: None,
            direct_scanout: None,
        },
        output_format: None,
    }
}

type Claims =
    BTreeMap<(OutputId, LiveProductionNativeFrameId), BTreeMap<ShellContentKey, ContentGrant>>;

fn claims(fixture: &Fixture) -> Claims {
    fixture.runtime.queued_shell_retirements.clone()
}

#[test]
fn an_accepted_candidate_is_installed_with_its_claim_and_no_render() {
    let mut fixture = fixture();
    let mut native = Native::new();
    let candidate = fixture.candidate.clone();
    let key = (fixture.output.id, SHELL);
    let dock = fixture
        .runtime
        .shell_content
        .get(&(fixture.output.id, DOCK))
        .cloned();
    let next = fixture.target.next;
    assert!(
        fixture
            .runtime
            .accept_combined_shell_content(
                candidate.clone(),
                SHELL,
                &mut native,
                &(),
                accept(fixture.output),
            )
            .is_ok()
    );
    assert_eq!(
        native.calls(),
        ["frame_service", "check", "preflight", "commit"]
    );
    assert_eq!(
        fixture
            .runtime
            .shell_content
            .get(&key)
            .map(|owned| &owned.frame),
        Some(&candidate)
    );
    assert_eq!(
        fixture
            .runtime
            .queued_shell_retirements
            .get(&(
                fixture.output.id,
                LiveProductionNativeFrameId::from_raw(FRAME)
            ))
            .and_then(|bound| bound.get(&key)),
        Some(&candidate.grant),
        "the claim is bound to the plan's native frame"
    );
    assert!(fixture.runtime.retained_projection_retirements.is_empty());
    assert_eq!(
        fixture
            .runtime
            .shell_content
            .get(&(fixture.output.id, DOCK))
            .cloned(),
        dock,
        "the installed Dock is untouched"
    );
    assert_eq!(fixture.target.next, next, "nothing reached the target");
}

/// A refusal before the native preflight: it writes nothing, never asks the
/// native half to preflight or commit, and hands the frame back.
fn runtime_refused(
    name: &str,
    layer: LiveShellContentLayer,
    change: impl FnOnce(&mut Fixture, &mut Native, &mut CombinedAccept) -> LiveShellContentFrame,
    expected: impl FnOnce(&CombinedAcceptError) -> bool,
) {
    let mut fixture = fixture();
    let mut native = Native::new();
    let mut accept = accept(fixture.output);
    let frame = change(&mut fixture, &mut native, &mut accept);
    let before = (observed(&fixture), claims(&fixture));
    let Err(failed) = fixture.runtime.accept_combined_shell_content(
        frame.clone(),
        layer,
        &mut native,
        &(),
        accept,
    ) else {
        panic!("{name}: must refuse");
    };
    assert!(expected(&failed.error), "{name}: its own refusal");
    assert_eq!(failed.frame, frame, "{name}: the frame comes back");
    assert_eq!(
        native.calls(),
        ["frame_service"],
        "{name}: no native preflight"
    );
    assert!(
        (observed(&fixture), claims(&fixture)) == before,
        "{name}: nothing written"
    );
}

#[test]
fn a_runtime_refusal_writes_nothing_and_asks_no_native_preflight() {
    runtime_refused(
        "quiesced native presentation",
        SHELL,
        |fixture, native, _| {
            native.service = false;
            fixture.candidate.clone()
        },
        |error| {
            matches!(error, CombinedAcceptError::Runtime(message)
                if message.to_string().contains("native presentation is quiesced"))
        },
    );
    runtime_refused(
        "an installed frame",
        DOCK,
        |fixture, _, _| {
            fixture.runtime.shell_content[&(fixture.output.id, DOCK)]
                .frame
                .clone()
        },
        |error| matches!(error, CombinedAcceptError::Unchanged),
    );
    runtime_refused(
        "a plan for another output",
        SHELL,
        |fixture, _, accept| {
            accept.snapshot.output = outputs()[1];
            fixture.candidate.clone()
        },
        |error| matches!(error, CombinedAcceptError::Output),
    );
    runtime_refused(
        "a native frame that already carries claims",
        SHELL,
        |fixture, _, _| {
            let output = fixture.output.id;
            let grant = fixture.candidate.grant;
            fixture
                .runtime
                .queued_shell_retirements
                .entry((output, LiveProductionNativeFrameId::from_raw(FRAME)))
                .or_default()
                .insert((output, DOCK), grant);
            fixture.candidate.clone()
        },
        |error| matches!(error, CombinedAcceptError::RetirementClaim),
    );
    runtime_refused(
        "an unbound retirement claim for the key",
        SHELL,
        |fixture, _, _| {
            let key = (fixture.output.id, SHELL);
            let grant = fixture.candidate.grant;
            fixture
                .runtime
                .retained_projection_retirements
                .insert(key, grant);
            fixture.candidate.clone()
        },
        |error| matches!(error, CombinedAcceptError::RetirementClaim),
    );
}

#[test]
fn a_native_refusal_leaves_the_runtime_unchanged() {
    let mut fixture = fixture();
    let mut native = Native::new();
    native.refusal = Some(CombinedAcceptRefusal::PendingTransition);
    let before = (observed(&fixture), claims(&fixture));
    let candidate = fixture.candidate.clone();
    let Err(failed) = fixture.runtime.accept_combined_shell_content(
        candidate.clone(),
        SHELL,
        &mut native,
        &(),
        accept(fixture.output),
    ) else {
        panic!("a native refusal must refuse");
    };
    assert!(matches!(
        failed.error,
        CombinedAcceptError::Native(CombinedAcceptRefusal::PendingTransition)
    ));
    assert_eq!(failed.frame, candidate);
    assert_eq!(native.calls(), ["frame_service", "check", "preflight"]);
    assert!(
        (observed(&fixture), claims(&fixture)) == before,
        "a native refusal leaves the runtime unchanged"
    );
}

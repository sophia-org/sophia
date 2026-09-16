#![cfg(feature = "native-session")]

use std::cell::RefCell;
use std::collections::BTreeMap;

use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
};
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopNamedOutputCandidate, DesktopOutputCandidate,
    reconcile_desktop_output_candidate,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{OutputId, Size};
use sophia_session::desktop_output_heads::{
    NativeOutputComposedHead, NativeOutputHeadResolveError, NativeOutputHeadUnavailable,
    NativeOutputScanoutHardware, NativeOutputTopologyHardware, resolve_native_output_scanout_heads,
    resolve_native_output_topology_heads,
};
use sophia_session::desktop_output_topology::{
    NativeOutputActivationPlan, prepare_native_output_activation_plan,
    project_native_output_topology,
};

/// One head as this test sees it. The real composer yields DRM handles; the
/// resolver never inspects a head, so a pair of numbers proves the same behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestHead {
    output: u64,
    timing: LibdrmNativeOutputTiming,
}

#[derive(Debug, Default)]
struct FakeHardware {
    /// Outputs that fail, and how.
    failures: BTreeMap<u64, NativeOutputHeadUnavailable>,
    next_blob: RefCell<u64>,
    created: RefCell<Vec<u64>>,
    released: RefCell<Vec<u64>>,
    /// Connectors a head was composed for, in order. Under mirroring two heads
    /// share an `OutputId`, so the connector is the only thing that distinguishes
    /// them -- recording it is how a test can see that both were driven.
    composed: RefCell<Vec<String>>,
}

impl FakeHardware {
    fn new() -> Self {
        Self {
            next_blob: RefCell::new(700),
            ..Self::default()
        }
    }

    fn failing(output: u64, cause: NativeOutputHeadUnavailable) -> Self {
        let mut hardware = Self::new();
        hardware.failures.insert(output, cause);
        hardware
    }

    fn live_blobs(&self) -> Vec<u64> {
        let released = self.released.borrow();
        self.created
            .borrow()
            .iter()
            .copied()
            .filter(|blob| !released.contains(blob))
            .collect()
    }
}

impl NativeOutputTopologyHardware for FakeHardware {
    type Head = TestHead;

    fn compose_head(
        &self,
        output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        if let Some(cause) = self.failures.get(&output.raw()) {
            return Err(*cause);
        }
        self.composed.borrow_mut().push(connector.to_string());
        let mut next = self.next_blob.borrow_mut();
        let mode_blob = *next;
        *next += 1;
        self.created.borrow_mut().push(mode_blob);
        Ok(NativeOutputComposedHead {
            head: TestHead {
                output: output.raw(),
                timing,
            },
            mode_blob,
        })
    }

    fn release_mode_blob(&self, _output: OutputId, blob: u64) {
        self.released.borrow_mut().push(blob);
    }
}

/// A scanout head as this test sees it: which output, and whether it carries the
/// requested topology or the one being restored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestScanoutHead {
    output: u64,
    restores: bool,
}

#[derive(Debug, Default)]
struct FakeScanoutHardware {
    apply_failures: BTreeMap<u64, NativeOutputHeadUnavailable>,
    rollback_failures: BTreeMap<u64, NativeOutputHeadUnavailable>,
    next_blob: RefCell<u64>,
    created: RefCell<Vec<u64>>,
    released: RefCell<Vec<u64>>,
    /// See `FakeHardware::composed`.
    composed: RefCell<Vec<String>>,
}

impl FakeScanoutHardware {
    fn new() -> Self {
        Self {
            next_blob: RefCell::new(900),
            ..Self::default()
        }
    }

    fn live_blobs(&self) -> Vec<u64> {
        let released = self.released.borrow();
        self.created
            .borrow()
            .iter()
            .copied()
            .filter(|blob| !released.contains(blob))
            .collect()
    }

    fn take_blob(&self) -> u64 {
        let mut next = self.next_blob.borrow_mut();
        let blob = *next;
        *next += 1;
        self.created.borrow_mut().push(blob);
        blob
    }
}

impl NativeOutputScanoutHardware for FakeScanoutHardware {
    type Head = TestScanoutHead;

    fn compose_apply_head(
        &self,
        output: OutputId,
        connector: &str,
        _timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        if let Some(cause) = self.apply_failures.get(&output.raw()) {
            return Err(*cause);
        }
        self.composed.borrow_mut().push(connector.to_string());
        Ok(NativeOutputComposedHead {
            head: TestScanoutHead {
                output: output.raw(),
                restores: false,
            },
            mode_blob: self.take_blob(),
        })
    }

    fn compose_rollback_head(
        &self,
        output: OutputId,
        connector: &str,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        if let Some(cause) = self.rollback_failures.get(&output.raw()) {
            return Err(*cause);
        }
        self.composed.borrow_mut().push(connector.to_string());
        Ok(NativeOutputComposedHead {
            head: TestScanoutHead {
                output: output.raw(),
                restores: true,
            },
            mode_blob: self.take_blob(),
        })
    }

    fn release_mode_blob(&self, _output: OutputId, blob: u64) {
        self.released.borrow_mut().push(blob);
    }
}

fn timing() -> LibdrmNativeOutputTiming {
    LibdrmNativeOutputTiming::new(1920, 1080, 60_000)
}

fn capability(output: u64, connector: &str) -> LibdrmNativeOutputCapability {
    LibdrmNativeOutputCapability::new(
        OutputId::from_raw(output),
        u32::try_from(output).expect("test output ids are small"),
        connector,
        [timing()],
        Some(timing()),
        timing(),
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .expect("capability fixture is valid")
}

fn headless(output: u64) -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(output),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }
}

fn plan_for(
    outputs: &[u64],
) -> (
    NativeOutputActivationPlan,
    Vec<LibdrmNativeOutputCapability>,
) {
    plan_with_disabled(outputs, &[])
}

fn plan_with_disabled(
    outputs: &[u64],
    disabled: &[u64],
) -> (
    NativeOutputActivationPlan,
    Vec<LibdrmNativeOutputCapability>,
) {
    let capabilities = outputs
        .iter()
        .map(|output| capability(*output, &format!("DP-{output}")))
        .collect::<Vec<_>>();
    let headless = outputs.iter().copied().map(headless).collect::<Vec<_>>();
    let topology =
        project_native_output_topology(&capabilities, &headless).expect("topology projects");
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(7),
        digest: ConfigDigest::new([9; 32]),
        inherit_sophia: true,
        named: disabled
            .iter()
            .map(|output| DesktopNamedOutputCandidate {
                policy_key: None,
                mirror_fit: None,
                connector: format!("DP-{output}"),
                mode: None,
                scale: None,
                position: None,
                transform: None,
                enabled: Some(false),
                focus_at_startup: None,
                vrr: None,
                mirror: Vec::new(),
            })
            .collect(),
    };
    let reconciliation =
        reconcile_desktop_output_candidate(&candidate, &topology).expect("candidate reconciles");
    let plan = prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation)
        .expect("plan prepares");
    (plan, capabilities)
}

/// A capability for a connector that shares its logical output with another.
///
/// The `output` and `connector_id` differ deliberately: under mirroring one
/// `OutputId` covers several connectors, which is exactly the case where keying
/// head composition by output rather than by connector goes wrong.
fn mirror_capability(
    output: u64,
    connector_id: u32,
    connector: &str,
) -> LibdrmNativeOutputCapability {
    LibdrmNativeOutputCapability::new(
        OutputId::from_raw(output),
        connector_id,
        connector,
        [timing()],
        Some(timing()),
        timing(),
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .expect("capability fixture is valid")
}

#[test]
fn a_mirror_group_projects_two_connectors_behind_one_output() {
    // The shape head composition has to survive. Both connectors carry one
    // `OutputId`, which is why composition is keyed by connector -- keying by
    // output would compose the same head twice and leave the other dark.
    //
    // This stops at the projection rather than running through reconciliation,
    // because a mirror directive is refused again until a group's heads can come
    // up at their own modes with the scene composed into each. Restoring the
    // end-to-end form is part of that work landing.
    let capabilities = vec![
        mirror_capability(1, 1, "DP-1"),
        mirror_capability(1, 2, "DP-2"),
    ];

    let topology = project_native_output_topology(&capabilities, &[headless(1)])
        .expect("a mirror group projects");

    assert_eq!(topology.connectors.len(), 2);
    assert_eq!(topology.connectors[0].connector, "DP-1");
    assert_eq!(topology.connectors[1].connector, "DP-2");
    assert_eq!(
        topology.connectors[0].current.position, topology.connectors[1].current.position,
        "one logical output means one position"
    );
}

#[test]
fn every_enabled_output_contributes_one_head() {
    let (plan, capabilities) = plan_for(&[1, 2]);
    let hardware = FakeHardware::new();

    let resolved = resolve_native_output_topology_heads(&plan, &capabilities, &hardware)
        .expect("a complete plan resolves");

    assert_eq!(resolved.len(), 2);
    assert_eq!(
        resolved.heads(),
        [
            TestHead {
                output: 1,
                timing: timing()
            },
            TestHead {
                output: 2,
                timing: timing()
            },
        ]
    );
    // Composition is addressed by connector, which is what keeps a mirror group's
    // second head from being lost to the first.
    assert_eq!(*hardware.composed.borrow(), ["DP-1", "DP-2"]);
    assert_eq!(hardware.live_blobs().len(), 2);
}

#[test]
fn dropping_a_resolution_releases_every_blob_it_created() {
    let (plan, capabilities) = plan_for(&[1, 2]);
    let hardware = FakeHardware::new();

    {
        let resolved = resolve_native_output_topology_heads(&plan, &capabilities, &hardware)
            .expect("a complete plan resolves");
        assert_eq!(resolved.len(), 2);
        assert!(hardware.released.borrow().is_empty());
    }

    // The blobs belong to the resolution, so leaving its scope returns them.
    assert!(hardware.live_blobs().is_empty());
    assert_eq!(hardware.released.borrow().len(), 2);
}

#[test]
fn one_unavailable_output_fails_the_whole_topology_and_leaks_nothing() {
    // The second output fails after the first has already created a blob. A
    // partial topology is a different desktop, so the plan fails closed -- and the
    // blob the first output created must not survive the failure.
    let (plan, capabilities) = plan_for(&[1, 2]);
    let hardware = FakeHardware::failing(2, NativeOutputHeadUnavailable::MissingProperties);

    let error = resolve_native_output_topology_heads(&plan, &capabilities, &hardware)
        .expect_err("an unavailable head fails the plan");

    assert_eq!(
        error,
        NativeOutputHeadResolveError::Unavailable {
            output: 2,
            cause: NativeOutputHeadUnavailable::MissingProperties,
        }
    );
    assert_eq!(hardware.created.borrow().len(), 1);
    assert!(
        hardware.live_blobs().is_empty(),
        "a rejected resolution releases what it created"
    );
}

#[test]
fn each_unavailable_cause_survives_into_the_error() {
    // The causes are kept apart because they send an operator somewhere different.
    for cause in [
        NativeOutputHeadUnavailable::MissingSelection,
        NativeOutputHeadUnavailable::MissingProperties,
        NativeOutputHeadUnavailable::UnknownTiming,
        NativeOutputHeadUnavailable::ModeBlobRefused,
    ] {
        let (plan, capabilities) = plan_for(&[1]);
        let hardware = FakeHardware::failing(1, cause);

        assert_eq!(
            resolve_native_output_topology_heads(&plan, &capabilities, &hardware)
                .expect_err("the only head is unavailable"),
            NativeOutputHeadResolveError::Unavailable { output: 1, cause }
        );
    }
}

#[test]
fn a_disabled_output_contributes_no_head() {
    // Disablement is expressed by omission, not by an inactive head. That is the
    // same rule the output logical-space contract states for policy snapshots.
    let (plan, capabilities) = plan_with_disabled(&[1, 2], &[2]);
    let hardware = FakeHardware::new();

    let resolved = resolve_native_output_topology_heads(&plan, &capabilities, &hardware)
        .expect("one enabled output is a topology");

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved.heads()[0].output, 1);
    assert_eq!(hardware.created.borrow().len(), 1);
}

#[test]
fn a_candidate_that_enables_nothing_never_becomes_a_plan() {
    // The resolver carries a NoEnabledOutputs arm, but production never reaches it:
    // reconciliation refuses an all-disabled candidate first. Pinning that here
    // records where the invariant actually lives, so a later change that relaxes
    // reconciliation fails a test instead of quietly making the backstop load-bearing.
    let capabilities = [capability(1, "DP-1")];
    let topology =
        project_native_output_topology(&capabilities, &[headless(1)]).expect("topology projects");
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(7),
        digest: ConfigDigest::new([9; 32]),
        inherit_sophia: true,
        named: vec![DesktopNamedOutputCandidate {
            policy_key: None,
            mirror_fit: None,
            connector: "DP-1".to_owned(),
            mode: None,
            scale: None,
            position: None,
            transform: None,
            enabled: Some(false),
            focus_at_startup: None,
            vrr: None,
            mirror: Vec::new(),
        }],
    };

    assert!(
        reconcile_desktop_output_candidate(&candidate, &topology).is_err(),
        "a desktop with no enabled output is refused before a plan exists"
    );
}

#[test]
fn a_plan_whose_outputs_have_no_capability_resolves_nothing() {
    let (plan, _) = plan_for(&[1]);
    let hardware = FakeHardware::new();

    let error = resolve_native_output_topology_heads(&plan, &[], &hardware)
        .expect_err("an output with no capability cannot be composed");

    assert_eq!(error, NativeOutputHeadResolveError::MissingCapability(1));
    assert!(
        hardware.created.borrow().is_empty(),
        "a missing capability is caught before any kernel resource exists"
    );
}

#[test]
fn applying_resolves_a_rollback_beside_every_head() {
    // Rollback is resolved while the previous topology is still on screen. Sourcing
    // it after an apply has failed would source it from a desktop already wrong.
    let (plan, capabilities) = plan_for(&[1, 2]);
    let hardware = FakeScanoutHardware::new();

    let resolved = resolve_native_output_scanout_heads(&plan, &capabilities, &hardware)
        .expect("a complete plan resolves");

    assert_eq!(resolved.len(), 2);
    assert_eq!(
        resolved.apply(),
        [
            TestScanoutHead {
                output: 1,
                restores: false
            },
            TestScanoutHead {
                output: 2,
                restores: false
            },
        ]
    );
    assert_eq!(
        resolved.rollback(),
        [
            TestScanoutHead {
                output: 1,
                restores: true
            },
            TestScanoutHead {
                output: 2,
                restores: true
            },
        ]
    );
    // One blob per head, apply and rollback alike: restoring a mode needs that
    // mode's own blob.
    assert_eq!(hardware.live_blobs().len(), 4);
}

#[test]
fn an_output_with_no_usable_framebuffer_stops_the_apply() {
    // Reusing what is already on screen only works when the sizes agree. A mode
    // change needs a buffer allocated at the new size, and declining here is what
    // keeps a half-applied desktop from being attempted.
    let (plan, capabilities) = plan_for(&[1, 2]);
    let mut hardware = FakeScanoutHardware::new();
    hardware.apply_failures.insert(
        2,
        NativeOutputHeadUnavailable::NeedsFramebuffer { have: None },
    );

    let error = resolve_native_output_scanout_heads(&plan, &capabilities, &hardware)
        .expect_err("an output without a framebuffer cannot be applied");

    assert_eq!(
        error,
        NativeOutputHeadResolveError::Unavailable {
            output: 2,
            cause: NativeOutputHeadUnavailable::NeedsFramebuffer { have: None },
        }
    );
    assert!(
        hardware.live_blobs().is_empty(),
        "a rejected apply resolution releases every blob it created"
    );
}

#[test]
fn an_unrestorable_output_stops_the_apply_before_anything_is_submitted() {
    // No rollback means no way back, so the apply must not start. This is the one
    // failure that would otherwise be discovered only after the screen changed.
    let (plan, capabilities) = plan_for(&[1]);
    let mut hardware = FakeScanoutHardware::new();
    hardware.rollback_failures.insert(
        1,
        NativeOutputHeadUnavailable::NeedsFramebuffer { have: None },
    );

    let error = resolve_native_output_scanout_heads(&plan, &capabilities, &hardware)
        .expect_err("an output that cannot be restored cannot be applied");

    assert_eq!(
        error,
        NativeOutputHeadResolveError::Unavailable {
            output: 1,
            cause: NativeOutputHeadUnavailable::NeedsFramebuffer { have: None },
        }
    );
    assert_eq!(
        hardware.created.borrow().len(),
        1,
        "the apply head was composed before rollback failed"
    );
    assert!(hardware.live_blobs().is_empty());
}

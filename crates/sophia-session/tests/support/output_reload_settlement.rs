//! A reloaded profile's output topology is Session's own transaction. It is
//! admitted with the output owner's epoch, not the WM's. Local settlement with a
//! connected file client is covered by output_file_reload_settlement.rs.
use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_protocol::{
    OutputAuthoritySnapshot, OutputHeadTargetProposal, OutputLogicalGroupProposal,
    OutputTopologyCandidate, OutputTopologyIntent, OutputTransform, OutputVrrPolicy,
};

/// The daily profile at startup on the moved monitor: DP-1 preferred and
/// absent, DP-2 lit as the fallback with affinity 1.
const ADAPTIVE_OUTPUT: &str = "availability adaptive; fallback-policy-key 1; \
    inherit-sophia #false; named DP-1 { policy-key 1; enabled #true; }";

#[test]
fn gpu_admission_reload_is_declined_before_staging_or_replacing_a_wm() {
    use std::io::Write;
    for policy in [None, Some("grid")] {
        let (mut fixture, realized) = fallback_session();
        assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
        let before = fixture.source.config.desktop_profile.clone();
        save_output_profile(&fixture, policy, ADAPTIVE_OUTPUT);
        let path = fixture
            .source
            .config
            .desktop_profile_source
            .as_ref()
            .unwrap();
        writeln!(
            std::fs::OpenOptions::new().append(true).open(path).unwrap(),
            "session {{ exclude-gpu \"pci-0000:16:00.0\"; }}"
        )
        .unwrap();
        assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Declined);
        assert_eq!(fixture.source.config.desktop_profile, before);
        assert_eq!(
            fixture.wm.public.as_ref().unwrap().output_policy_keys,
            realized
        );
        assert!(!fixture.wm.desktop_reload_pending());
        assert!(!fixture.wm.force_transport_restart);
    }
}

fn save_output_profile(fixture: &ReloadFixture, policy: Option<&str>, output: &str) {
    use std::io::Write;
    fixture.save("/replacement/command", policy);
    let path = fixture
        .source
        .config
        .desktop_profile_source
        .as_ref()
        .unwrap();
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "output {{ {output} }}").unwrap();
}

/// A session that started on the DP-2 fallback. Only its output profile and
/// realized keys are set; the desktop generation is still the fixture's, so
/// the first reload sees an output change.
fn fallback_session() -> (ReloadFixture, BTreeMap<String, u64>) {
    let mut fixture = ReloadFixture::new();
    save_output_profile(&fixture, None, ADAPTIVE_OUTPUT);
    let prepared = sophia_config::load_prepared_desktop_profile(
        fixture.source.config.desktop_profile_source.as_deref(),
        sophia_config::ConfigGeneration::INITIAL,
    )
    .unwrap();
    fixture.source.config.output_profile =
        PreparedOutputProfile::new(prepared.candidates.output).unwrap();
    let realized = BTreeMap::from([("DP-2".to_owned(), 1)]);
    fixture.wm.public.as_mut().unwrap().output_policy_keys = realized.clone();
    (fixture, realized)
}

#[test]
fn reload_keeps_the_realized_fallback_binding_across_unrelated_output_changes() {
    let (mut fixture, realized) = fallback_session();

    // Compare the saved identities to the previous profile, not to the
    // realized DP-2 binding, or no reload could follow a fallback start.
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(public.output_policy_keys, realized);
    assert!(public.output_topology_reload_pending);

    save_output_profile(
        &fixture,
        None,
        &ADAPTIVE_OUTPUT.replace("enabled #true;", "enabled #true; scale 1.25;"),
    );
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
    let current = fixture.source.config.output_profile.current();
    assert_eq!(current.named.len(), 1);
    assert_eq!(
        current.named[0].scale,
        Some(sophia_config::DesktopOutputScale::FixedMilli(1250))
    );
    assert_eq!(current.fallback_policy_key, Some(1));
    assert_eq!(
        fixture.wm.public.as_ref().unwrap().output_policy_keys,
        realized
    );
}

#[test]
fn reload_declines_output_identity_changes_and_keeps_the_session() {
    let (mut fixture, realized) = fallback_session();
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
    let generation = fixture.source.config.desktop_profile.generation;
    let before = fixture.source.config.output_profile.current().clone();

    for (output, change) in [
        (
            ADAPTIVE_OUTPUT.replace("fallback-policy-key 1", "fallback-policy-key 2"),
            "fallback key",
        ),
        (
            ADAPTIVE_OUTPUT.replace(
                "availability adaptive; fallback-policy-key 1; ",
                "availability strict; ",
            ),
            "availability",
        ),
        (
            ADAPTIVE_OUTPUT.replace("{ policy-key 1;", "{ policy-key 3;"),
            "named key",
        ),
        (
            format!("{ADAPTIVE_OUTPUT}; named DP-2 {{ policy-key 2; enabled #true; }}"),
            "added key",
        ),
        (
            format!("{ADAPTIVE_OUTPUT}; named HDMI-A-2 {{ enabled #false; }}"),
            "exclusion",
        ),
        (
            ADAPTIVE_OUTPUT.replace("enabled #true;", "enabled #true; mirror DP-3;"),
            "mirror membership",
        ),
    ] {
        save_output_profile(&fixture, None, &output);
        // Declined is an ordinary outcome; an error here would end the
        // owner loop and with it the operator's session.
        assert_eq!(
            fixture.reload(),
            DesktopProfileReloadOutcome::Declined,
            "{change}"
        );
        assert_eq!(
            fixture.source.config.desktop_profile.generation, generation,
            "{change}"
        );
        assert_eq!(
            fixture.source.config.output_profile.current(),
            &before,
            "{change}"
        );
        assert_eq!(
            fixture.wm.public.as_ref().unwrap().output_policy_keys,
            realized,
            "{change}"
        );
        assert!(!fixture.wm.desktop_reload_pending(), "{change}");
    }
}

#[test]
fn a_policy_reload_changing_output_identity_is_declined_before_a_replacement_wm() {
    let (mut fixture, realized) = fallback_session();
    let generation = fixture.source.config.desktop_profile.generation;
    let policy_path = fixture.policy_path();
    let profile_key = fixture.wm.public.as_ref().unwrap().profile_key;
    let launch_spec = fixture.wm.supervisor.launch_spec().clone();

    save_output_profile(
        &fixture,
        Some("grid"),
        &ADAPTIVE_OUTPUT.replace("fallback-policy-key 1", "fallback-policy-key 2"),
    );
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Declined);
    assert!(!fixture.wm.desktop_reload_pending());
    assert!(!fixture.wm.force_transport_restart);
    assert_eq!(fixture.policy_path(), policy_path);
    assert_eq!(fixture.wm.public.as_ref().unwrap().profile_key, profile_key);
    assert_eq!(fixture.wm.supervisor.launch_spec(), &launch_spec);
    assert_eq!(fixture.source.config.desktop_profile.generation, generation);
    assert_eq!(
        fixture
            .source
            .config
            .output_profile
            .current()
            .fallback_policy_key,
        Some(1)
    );

    // The same policy change with the identities kept goes through the
    // replacement and publishes with the realized binding intact.
    save_output_profile(&fixture, Some("grid"), ADAPTIVE_OUTPUT);
    assert_eq!(
        fixture.reload(),
        DesktopProfileReloadOutcome::RestartRequired
    );
    fixture.replacement_started();
    assert_eq!(
        fixture.stage_configuration(&fixture.configuration()),
        sophia_protocol::PolicyProjectionOutcome::Committed
    );
    fixture.settle(true);
    assert!(!fixture.wm.desktop_reload_pending());
    let public = fixture.wm.public.as_ref().unwrap();
    assert!(public.configured);
    assert!(public.output_topology_reload_pending);
    assert_eq!(public.output_policy_keys, realized);
    assert!(fixture.source.config.desktop_profile.generation.raw() > generation.raw());
}

/// One connected head, its published snapshot and an apply candidate that
/// names it.
fn reload_inputs(
    public: &LivePublicPolicyState,
) -> (
    LibdrmNativeOutputCapability,
    OutputAuthoritySnapshot,
    OutputTopologyCandidate,
) {
    let output = public.outputs[0];
    let timing = LibdrmNativeOutputTiming::new(
        u32::try_from(output.size.width).unwrap(),
        u32::try_from(output.size.height).unwrap(),
        60_000,
    );
    let capability = LibdrmNativeOutputCapability::new(
        output.id,
        11,
        "DP-1",
        [timing],
        Some(timing),
        timing,
        LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
    )
    .unwrap()
    .bind_head(sophia_engine::RenderHeadId::from_raw(11))
    .unwrap();
    let snapshot =
        project_live_output_authority_snapshot(std::slice::from_ref(&capability), &[output], 7)
            .unwrap();
    let head = &snapshot.heads[0];
    let group = &snapshot.groups[0];
    let candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: vec![OutputHeadTargetProposal {
            head: head.head,
            head_generation: head.generation,
            mode: head.current_mode.unwrap(),
            transform: OutputTransform::Normal,
            vrr: OutputVrrPolicy::Disabled,
        }],
        groups: vec![OutputLogicalGroupProposal {
            output: group.output,
            logical: group.logical,
            members: group.members.clone(),
        }],
    };
    (capability, snapshot, candidate)
}

/// The output role's connection epoch and the WM policy's connection epoch
/// advance independently: output on peer departure and assignee replacement,
/// the WM on policy restarts. A reload is admitted against the former in both
/// orders of divergence.
#[test]
fn output_reload_is_admitted_with_the_output_owner_epoch_not_the_wm_epoch() {
    for (output_epoch, wm_epoch) in [(3, 1), (1, 4)] {
        let mut fixture = ReloadFixture::new();
        let public = fixture.wm.public.as_mut().unwrap();
        let (capability, snapshot, candidate) = reload_inputs(public);
        public.connection_epoch = wm_epoch;
        public.output_authority = Some(
            crate::live_output_authority::LiveOutputAuthorityOwner::new(output_epoch, snapshot)
                .unwrap(),
        );
        public.output_capabilities = vec![capability];
        assert!(
            public.admit_reloaded_output_topology(candidate).unwrap(),
            "reload declined with output epoch {output_epoch} and WM epoch {wm_epoch}"
        );
        assert!(
            public
                .output_authority
                .as_ref()
                .unwrap()
                .active_transaction()
                .is_some()
        );
    }
}

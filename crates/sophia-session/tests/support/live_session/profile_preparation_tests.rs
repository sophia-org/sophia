use super::*;

/// A run-unique directory, so concurrent tests cannot share a socket path.
fn test_config_root(prefix: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn public_profile_test_config(prefix: &str) -> PersistentXtermSessionConfig {
    public_profile_test_config_rooted(
        test_config_root(prefix),
        &["--wm-process=/usr/bin/true".to_owned()],
    )
}

/// The same configuration over a chosen window manager process.
///
/// Which process stands in decides which failure a launch can produce, so a
/// test that asserts one of them has to choose deliberately rather than take
/// the default.
fn public_profile_test_config_rooted(
    root: std::path::PathBuf,
    wm_args: &[String],
) -> PersistentXtermSessionConfig {
    let mut args = wm_args.to_vec();
    args.push("--wm-interface=sophia_wm_v1".to_owned());
    let mut config = super::session_config_tests::isolated_session_config(&args).unwrap();
    config.wm_socket_path = root.with_extension("sock");
    config
}

/// A window manager that starts, stays up, and never connects.
///
/// `/usr/bin/true` cannot stand in for this. It exits at once, which leaves the
/// supervisor racing its two readings of a failed launch: bubblewrap already
/// gone (`protection.rs`, `child.try_wait()`), against the child found in
/// /proc and the launch carrying on to time out at the accept. Both readings
/// are honest and the faster one wins, so the error a caller sees depends on
/// how the machine was loaded that second -- which is a spurious red in any
/// full-suite run, not a defect being caught.
///
/// A process that outlives the five second accept window closes the race from
/// the test's side: the child is there to be found, and the accept timeout is
/// the only failure still reachable.
fn nonconnecting_wm_test_config(prefix: &str) -> PersistentXtermSessionConfig {
    public_profile_test_config_rooted(
        test_config_root(prefix),
        &[
            "--wm-process=/usr/bin/sleep".to_owned(),
            "--wm-process-arg=10".to_owned(),
        ],
    )
}

#[test]
fn public_policy_launch_preparation_validates_fragments_and_cleans_up_before_launch() {
    use std::os::unix::fs::PermissionsExt as _;

    let config = public_profile_test_config("sophia-policy-prepare-test");
    let directory_path = config.wm_socket_path.with_extension("policy");
    let activation_key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);

    let prepared = PreparedPublicPolicyLaunch::new(&config).unwrap();

    assert_eq!(
        std::fs::metadata(&directory_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    sophia_config::validate_desktop_profile_fragments(&prepared.profile_fragments, activation_key)
        .unwrap();
    assert_eq!(
        prepared.shortcut_profile_slot.participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Prepared
    );
    for authority in sophia_config::DesktopAuthority::ALL {
        assert!(prepared.profile_fragments.path(authority).is_file());
    }

    drop(prepared);
    assert!(!directory_path.exists());
}

#[test]
fn public_profile_startup_reaches_the_complete_prepare_barrier_before_launch() {
    let mut config = public_profile_test_config("sophia-profile-barrier-test");
    let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);

    let prepared = LiveWmSession::prepare_public_launch(&mut config)
        .unwrap()
        .unwrap();

    assert_eq!(
        config.desktop_profile_activation.phase(),
        sophia_config::DesktopProfileActivationPhase::Prepared
    );
    assert_eq!(config.desktop_profile_activation.candidate(), Some(key));
    assert_eq!(config.desktop_profile_activation.active(), None);
    for participant in [
        prepared.policy_profile.slot.participant(),
        prepared.shell_profile.slot.participant(),
        prepared.shortcut_profile_slot.participant(),
        config.session_profile.slot().participant(),
        config.input_profile.slot().participant(),
        config.output_profile.slot().participant(),
        prepared.broker_profile.slot.participant(),
    ] {
        assert_eq!(
            participant.phase(),
            sophia_config::DesktopProfileParticipantPhase::Prepared
        );
        assert_eq!(participant.candidate(), Some(key));
        assert_eq!(participant.active(), None);
    }

    let directory_path = config.wm_socket_path.with_extension("policy");
    drop(prepared);
    assert!(!directory_path.exists());
}

#[test]
fn public_profile_activation_promotes_local_slots_and_pauses_at_policy() {
    let mut config = public_profile_test_config("sophia-profile-local-activation-test");
    let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);
    let mut prepared = LiveWmSession::prepare_public_launch(&mut config)
        .unwrap()
        .unwrap();
    let model = config.desktop_profile_activation.clone();
    let mut executor = PublicProfilePreparationExecutor {
        policy: prepared.policy_profile.slot_mut(),
        shell: prepared.shell_profile.slot_mut(),
        shortcut: &mut prepared.shortcut_profile_slot,
        session: config.session_profile.slot_mut(),
        input: config.input_profile.slot_mut(),
        output: config.output_profile.slot_mut(),
        broker: prepared.broker_profile.slot_mut(),
    };

    let report =
        crate::desktop_profile_activation::run_desktop_profile_prepared_activation_until_policy(
            &model,
            key,
            &mut executor,
        )
        .unwrap();
    // Ends the borrow; the executor holds nothing that needs dropping.
    let _ = executor;
    let policy_effect = report.effect.unwrap();

    assert_eq!(
        report.disposition,
        crate::desktop_profile_activation::DesktopProfileExternalActivationDisposition::AwaitingPolicy
    );
    assert_eq!(
        policy_effect.authority,
        sophia_config::DesktopAuthority::Policy
    );
    assert_eq!(
        prepared.policy_profile.slot.participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Prepared
    );
    for participant in [
        prepared.shell_profile.slot.participant(),
        prepared.shortcut_profile_slot.participant(),
        config.session_profile.slot().participant(),
        config.input_profile.slot().participant(),
        config.output_profile.slot().participant(),
        prepared.broker_profile.slot.participant(),
    ] {
        assert_eq!(
            participant.phase(),
            sophia_config::DesktopProfileParticipantPhase::Activated
        );
        assert_eq!(participant.active(), Some(key));
    }

    let rejected = crate::desktop_profile_activation::settle_desktop_profile_policy_activation(
        &report.model,
        policy_effect,
        false,
    )
    .unwrap();
    let mut rollback_executor = PublicProfilePreparationExecutor {
        policy: prepared.policy_profile.slot_mut(),
        shell: prepared.shell_profile.slot_mut(),
        shortcut: &mut prepared.shortcut_profile_slot,
        session: config.session_profile.slot_mut(),
        input: config.input_profile.slot_mut(),
        output: config.output_profile.slot_mut(),
        broker: prepared.broker_profile.slot_mut(),
    };
    let rolled_back = crate::desktop_profile_activation::run_desktop_profile_rollback(
        rejected.model,
        rejected.effects,
        &mut rollback_executor,
    )
    .unwrap();
    // Ends the borrow; the executor holds nothing that needs dropping.
    let _ = rollback_executor;
    assert_eq!(
        rolled_back.phase(),
        sophia_config::DesktopProfileActivationPhase::Idle
    );
    assert_eq!(rolled_back.active(), None);
    for participant in [
        prepared.policy_profile.slot.participant(),
        prepared.shell_profile.slot.participant(),
        prepared.shortcut_profile_slot.participant(),
        config.session_profile.slot().participant(),
        config.input_profile.slot().participant(),
        config.output_profile.slot().participant(),
        prepared.broker_profile.slot.participant(),
    ] {
        assert_eq!(
            participant.phase(),
            sophia_config::DesktopProfileParticipantPhase::Idle
        );
        assert_eq!(participant.active(), None);
        assert_eq!(participant.candidate(), None);
    }
}

#[test]
fn public_profile_prepare_failure_rolls_every_owner_back_without_activation() {
    for (failure_index, authority) in sophia_config::DesktopAuthority::ALL.into_iter().enumerate() {
        let mut config =
            public_profile_test_config(&format!("sophia-profile-rollback-test-{failure_index}"));
        let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);
        let mut prepared = PreparedPublicPolicyLaunch::new(&config).unwrap();
        match authority {
            sophia_config::DesktopAuthority::Policy => {
                *prepared.policy_profile.slot_mut() =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
            sophia_config::DesktopAuthority::Shell => {
                *prepared.shell_profile.slot_mut() =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
            sophia_config::DesktopAuthority::Shortcut => {
                prepared.shortcut_profile_slot =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
            sophia_config::DesktopAuthority::Session => {
                *config.session_profile.slot_mut() =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
            sophia_config::DesktopAuthority::Input => {
                *config.input_profile.slot_mut() =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
            sophia_config::DesktopAuthority::Output => {
                *config.output_profile.slot_mut() =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
            sophia_config::DesktopAuthority::Broker => {
                *prepared.broker_profile.slot_mut() =
                    sophia_config::DesktopProfileCandidateSlot::new(authority);
            }
        }

        let report = prepared
            .prepare_startup(
                &mut config.session_profile,
                &mut config.input_profile,
                &mut config.output_profile,
                &config.desktop_profile_activation,
                key,
            )
            .unwrap();

        assert_eq!(
            report.disposition,
            crate::desktop_profile_activation::DesktopProfileStartupPreparationDisposition::Rejected
        );
        assert_eq!(
            report.model.phase(),
            sophia_config::DesktopProfileActivationPhase::Idle
        );
        assert_eq!(report.model.active(), None);
        for participant in [
            prepared.policy_profile.slot.participant(),
            prepared.shell_profile.slot.participant(),
            prepared.shortcut_profile_slot.participant(),
            config.session_profile.slot().participant(),
            config.input_profile.slot().participant(),
            config.output_profile.slot().participant(),
            prepared.broker_profile.slot.participant(),
        ] {
            assert_eq!(
                participant.phase(),
                sophia_config::DesktopProfileParticipantPhase::Idle
            );
            assert_eq!(participant.active(), None);
            assert_eq!(participant.candidate(), None);
        }
    }
}

#[test]
fn pregraphics_policy_launch_failure_rolls_back_before_returning() {
    let mut config = nonconnecting_wm_test_config("sophia-profile-launch-failure-test");
    let directory_path = config.wm_socket_path.with_extension("policy");
    let prepared = LiveWmSession::prepare_public_launch(&mut config).unwrap();
    let started = Instant::now();

    let error = match LiveWmSession::activate_public_launch(&mut config, prepared) {
        Ok(_) => panic!("nonconnecting policy process unexpectedly activated"),
        Err(error) => error.to_string(),
    };

    assert!(
        error.contains("AcceptTimedOut"),
        "unexpected error: {error}"
    );
    assert!(started.elapsed() < Duration::from_secs(8));
    assert_eq!(
        config.desktop_profile_activation.phase(),
        sophia_config::DesktopProfileActivationPhase::Idle
    );
    assert_eq!(config.desktop_profile_activation.active(), None);
    for participant in [
        config.session_profile.slot().participant(),
        config.input_profile.slot().participant(),
        config.output_profile.slot().participant(),
    ] {
        assert_eq!(
            participant.phase(),
            sophia_config::DesktopProfileParticipantPhase::Idle
        );
        assert_eq!(participant.active(), None);
        assert_eq!(participant.candidate(), None);
    }
    assert!(!directory_path.exists());
}

#[test]
fn profile_restart_reattaches_the_exact_key_under_a_fresh_epoch() {
    let config = public_profile_test_config("sophia-profile-restart-identity-test");
    let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);

    let initial = policy_profile_identity(1, key).unwrap();
    let restarted = policy_profile_identity(2, key).unwrap();

    assert_eq!(initial.connection_epoch, 1);
    assert_eq!(restarted.connection_epoch, 2);
    assert_eq!(restarted.profile_generation, initial.profile_generation);
    assert_eq!(restarted.profile_digest, initial.profile_digest);
}

#[test]
fn component_prepare_reads_a_session_profile_the_window_manager_already_activated() {
    // WM startup activates every participant, the session profile included,
    // before components are prepared. Prepare must read the profile it is
    // running, not assert that activation has not happened yet: a debug
    // build with --wm-process panicked here on every start.
    let mut config = public_profile_test_config("sophia-profile-activated-prepare-test");
    let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);
    let prepared = LiveWmSession::prepare_public_launch(&mut config)
        .unwrap()
        .unwrap();
    let activated =
        sophia_config::activate_desktop_profile_candidate_slot(config.session_profile.slot(), key)
            .unwrap();
    *config.session_profile.slot_mut() = activated;
    assert_eq!(
        config.session_profile.slot().participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Activated
    );

    let (components, _) = super::super::component_lifecycle::prepare(&config, None).unwrap();
    assert!(components.is_none());
    drop(prepared);
}

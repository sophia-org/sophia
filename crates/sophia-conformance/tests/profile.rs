//! Guards on the profile shapes, kept out of the module the audit reads.

use sophia_conformance::profile::*;

fn options() -> Options {
    Options {
        display: ":77".to_owned(),
        startup_executable: "/usr/bin/true".to_owned(),
        startup_arguments: Vec::new(),
        desktop_profile: None,
        core_config: None,
        wm_process: None,
        wm_arguments: Vec::new(),
    }
}

/// The shape of the bug that killed the standalone and native profiles for
/// three days: a profile with no serving window manager acquiring one.
#[test]
fn a_profile_with_no_window_manager_refuses_to_be_given_one() {
    let mut options = options();
    options.wm_process = Some("/usr/bin/true".to_owned());
    let refused = session_args(find("standalone").unwrap(), &options);
    assert!(
        refused.is_err(),
        "a WM-less profile accepted a window manager"
    );
}

/// And the converse: a profile that is served by a policy client must be
/// told which one, rather than silently starting without it.
#[test]
fn a_profile_served_by_a_policy_client_requires_one() {
    let refused = session_args(find("managed").unwrap(), &options());
    assert!(refused.is_err(), "a served profile started with no client");
}

/// Arguments for a process that does not exist are a mistake, not a no-op.
/// Dropping them is how a `--wm-config` came to be passed to nothing.
#[test]
fn window_manager_arguments_without_a_process_are_refused() {
    let mut options = options();
    options
        .wm_arguments
        .push("--wm-config=/tmp/wm.kdl".to_owned());
    let refused = session_args(find("standalone").unwrap(), &options);
    assert!(
        refused.is_err(),
        "a stray window-manager argument was dropped"
    );
}

/// A profile with no window manager has no logout shortcut, because
/// shortcuts are resolved against a policy client's configuration. Its
/// only ordinary exit is its application exiting, so it must ask for that.
#[test]
fn a_profile_with_no_window_manager_ends_with_its_application() {
    for profile in PROFILES.iter().filter(|profile| !profile.window_manager) {
        let vector = session_args(*profile, &options()).unwrap();
        assert!(
            vector
                .iter()
                .any(|argument| argument == "--exit-when-startup-exits"),
            "profile {:?} has no window manager and no way to end",
            profile.name
        );
    }
}

/// With neither config named, the session takes the compiled defaults --
/// one flag, not two half-specified ones.
#[test]
fn a_profile_with_no_configuration_asks_for_none() {
    let vector = session_args(find("standalone").unwrap(), &options()).unwrap();
    assert!(vector.iter().any(|argument| argument == "--no-config"));
    assert!(
        !vector
            .iter()
            .any(|argument| argument.starts_with("--config="))
    );
}

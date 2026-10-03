//! The session lock's configuration: the factotum agent arguments and the
//! `session:lock` binding they make available (t034, t293).

use super::session_config_tests::isolated_core_config_argument;
use super::*;

/// A lock needs an authenticator to end it. Without the factotum agent, a
/// profile that binds one is refused rather than given a lock nobody could
/// open.
#[test]
fn an_explicit_profile_binding_a_lock_is_refused_without_an_authenticator() {
    let profile = std::env::temp_dir().join(format!(
        "sophia-explicit-lock-{}-{}.kdl",
        std::process::id(),
        line!()
    ));
    std::fs::write(
        &profile,
        concat!(
            "schema 1\n",
            "shell { enabled #false; }\n",
            "shortcut {\n",
            "  profile \"explicit-lock\"\n",
            "  bind \"Super+l\" \"session:lock\"\n",
            "}\n",
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let refused = PersistentXtermSessionConfig::from_args(&[
        isolated_core_config_argument(),
        "--session-mode=normal".to_owned(),
        format!("--desktop-profile={}", profile.display()),
        "--session-app=standalone=/usr/bin/true".to_owned(),
        "--session-start=standalone".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ]);
    std::fs::remove_file(&profile).unwrap();

    let error = refused.expect_err("a lock binding was accepted without an authenticator");
    assert!(
        error.to_string().contains("unavailable session capability"),
        "{error}"
    );
}

fn lock_profile_arguments(profile: &std::path::Path, extra: &[&str]) -> Vec<String> {
    let mut arguments = vec![
        isolated_core_config_argument(),
        "--session-mode=normal".to_owned(),
        format!("--desktop-profile={}", profile.display()),
        "--session-app=standalone=/usr/bin/true".to_owned(),
        "--session-start=standalone".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ];
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    arguments
}

fn lock_profile(name: &str) -> std::path::PathBuf {
    let profile = std::env::temp_dir().join(format!(
        "sophia-lock-agent-{}-{name}.kdl",
        std::process::id()
    ));
    std::fs::write(
        &profile,
        concat!(
            "schema 1\n",
            "shell { enabled #false; }\n",
            "shortcut {\n",
            "  profile \"lock-agent\"\n",
            "  bind \"Super+l\" \"session:lock\"\n",
            "}\n",
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    profile
}

/// With the factotum agent configured the lock is a session capability, so
/// a profile may bind it. The agent starts only with the owner loop.
#[test]
fn a_configured_factotum_agent_makes_the_lock_bindable() {
    if std::env::var("LOGNAME")
        .or_else(|_| std::env::var("USER"))
        .is_err()
    {
        // The lock verifies the login user; an environment without one
        // cannot configure it, which the refusal test below covers.
        return;
    }
    let profile = lock_profile("accepted");
    let accepted = PersistentXtermSessionConfig::from_args(&lock_profile_arguments(
        &profile,
        &[
            "--factotum-agent=/usr/libexec/sophia-factotum",
            "--factotum-pam-helper=/usr/libexec/sophia-factotum-pam",
        ],
    ));
    std::fs::remove_file(&profile).unwrap();
    assert!(
        accepted.is_ok(),
        "{:?}",
        accepted.err().map(|error| error.to_string())
    );
}

#[test]
fn factotum_arguments_are_refused_when_incomplete_or_malformed() {
    let profile = lock_profile("refused");
    for (extra, reason) in [
        (
            &["--factotum-agent=/usr/libexec/sophia-factotum"][..],
            "required together",
        ),
        (
            &[
                "--factotum-agent=sophia-factotum",
                "--factotum-pam-helper=/usr/libexec/helper",
            ][..],
            "absolute",
        ),
        (
            &[
                "--factotum-agent=/usr/libexec/sophia-factotum",
                "--factotum-pam-helper=/usr/libexec/helper",
                "--factotum-pam-service=../etc",
            ][..],
            "--factotum-pam-service",
        ),
    ] {
        let refused =
            PersistentXtermSessionConfig::from_args(&lock_profile_arguments(&profile, extra));
        let error = refused.expect_err("malformed factotum arguments were accepted");
        assert!(error.to_string().contains(reason), "{error}");
    }
    std::fs::remove_file(&profile).unwrap();
}

/// The proof-only lock (`--inject-session-lock`) needs the real agent,
/// physical text and scanned-out heads, and is never a normal session's.
/// Native scanout is not admitted outside a real-scanout run, so the
/// accepted setting is the QEMU session-lock scenario's to prove.
#[test]
fn the_proof_lock_needs_the_agent_physical_text_and_native_scanout() {
    if std::env::var("LOGNAME")
        .or_else(|_| std::env::var("USER"))
        .is_err()
    {
        return;
    }
    // Everything but native scanout, less whatever `omit` names.
    let proof = |omit: &str| {
        let arguments: Vec<String> = [
            "--client=zenity",
            "--expect-client-stdout=sophia\n",
            "--require-client-normal-exit",
            "--expect-physical-text=sophia",
            "--max-runtime-ms=60000",
            "--factotum-agent=/usr/libexec/sophia-factotum",
            "--factotum-pam-helper=/usr/libexec/sophia-factotum-pam",
            "--inject-session-lock",
        ]
        .into_iter()
        .filter(|argument| omit.is_empty() || !argument.starts_with(omit))
        .map(str::to_owned)
        .collect();
        super::session_config_tests::isolated_session_config(&arguments)
    };
    for omit in ["", "--factotum-", "--expect-physical-text"] {
        let error = proof(omit).expect_err("an incomplete proof lock was accepted");
        assert!(
            error.to_string().contains("--inject-session-lock requires"),
            "{omit}: {error}"
        );
    }

    let profile = lock_profile("proof");
    let refused = PersistentXtermSessionConfig::from_args(&lock_profile_arguments(
        &profile,
        &[
            "--factotum-agent=/usr/libexec/sophia-factotum",
            "--factotum-pam-helper=/usr/libexec/sophia-factotum-pam",
            "--inject-session-lock",
        ],
    ));
    std::fs::remove_file(&profile).unwrap();
    let error = refused.expect_err("a normal session took the proof lock");
    assert!(
        error.to_string().contains("cannot be combined with proof"),
        "{error}"
    );
}

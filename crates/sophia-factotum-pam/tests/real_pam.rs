//! The helper against real Linux-PAM, deterministically: a private
//! configuration directory (`pam_start_confdir`) whose services use only
//! `pam_permit`, `pam_deny`, or a stack that prompts twice. No check reads
//! `/etc/pam.d` or a real password.

use sophia_factotum::pam_wire::{FLAG_DISALLOW_NULL, HelperReply, HelperRequest};
use sophia_factotum::proto::PamVerdict;
use sophia_factotum::secret::SecretBytes;
use std::io::Write;
use std::process::{Command, Stdio};

struct Confdir(std::path::PathBuf);

impl Confdir {
    fn new(services: &[(&str, &str)]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-factotum-pam-{}-{}",
            std::process::id(),
            services
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join("-")
        ));
        std::fs::create_dir_all(&path).unwrap();
        for (name, stack) in services {
            std::fs::write(path.join(name), stack).unwrap();
        }
        Self(path)
    }
}

impl Drop for Confdir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(confdir: &Confdir, service: &str, secret: &[u8]) -> Option<HelperReply> {
    run_as(Some(confdir), service, "sophia-test-user", secret)
}

fn run_as(
    confdir: Option<&Confdir>,
    service: &str,
    user: &str,
    secret: &[u8],
) -> Option<HelperReply> {
    let request = HelperRequest {
        flags: FLAG_DISALLOW_NULL,
        service: service.into(),
        user: user.into(),
        secret: SecretBytes::from_slice(secret),
    }
    .encode()
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_sophia-factotum-pam"));
    if let Some(confdir) = confdir {
        command.arg("--confdir").arg(&confdir.0);
    }
    let mut child = command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.as_slice())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    output
        .status
        .success()
        .then(|| HelperReply::decode(&output.stdout).unwrap())
}

#[test]
fn pam_permit_accepts_and_pam_deny_rejects() {
    let confdir = Confdir::new(&[
        ("permit", "auth required pam_permit.so\n"),
        ("deny", "auth required pam_deny.so\n"),
    ]);
    assert_eq!(
        run(&confdir, "permit", b"anything").map(|reply| reply.verdict),
        Some(PamVerdict::Accepted)
    );
    assert_eq!(
        run(&confdir, "deny", b"anything").map(|reply| reply.verdict),
        Some(PamVerdict::Rejected)
    );
}

#[test]
fn a_service_with_no_stack_is_unavailable_not_accepted() {
    let confdir = Confdir::new(&[("permit", "auth required pam_permit.so\n")]);
    let verdict = run(&confdir, "absent", b"anything").map(|reply| reply.verdict);
    assert_ne!(verdict, Some(PamVerdict::Accepted));
}

#[test]
fn a_malformed_request_gives_no_verdict() {
    let confdir = Confdir::new(&[("permit", "auth required pam_permit.so\n")]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_sophia-factotum-pam"))
        .arg("--confdir")
        .arg(&confdir.0)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\x0c\x00\x00\x00XXXX\x01\x00\x00\x00")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "no verdict for a malformed request"
    );
}

/// The system's services verify only the caller's own account. A request
/// naming anyone else is refused before PAM starts, so this check never
/// reads `/etc/pam.d`: neither name below is the account running it.
#[test]
fn the_system_services_refuse_a_user_other_than_the_caller() {
    for user in ["sophia-test-user-absent", "root"] {
        let caller = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/proc/self").unwrap());
        if user == "root" && caller == 0 {
            continue;
        }
        let reply = run_as(None, "sophia-lock", user, b"anything").unwrap();
        assert_eq!(reply.verdict, PamVerdict::Rejected, "{user}");
        assert_eq!(reply.pam_code, 10, "{user}: PAM_USER_UNKNOWN");
    }
}

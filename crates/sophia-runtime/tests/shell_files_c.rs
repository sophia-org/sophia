//! Independent C client against the production file export, without a desktop.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::*;
use sophia_runtime::*;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-c-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn compile(directory: &Path, name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source/src");
    let binary = directory.join(name);
    let mut cc = Command::new("nice");
    cc.args(["-n", "19"])
        .arg(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-UNDEBUG",
        ]);
    for domain in ["nine_p", "shell_files", "shell_session"] {
        let mut sources: Vec<_> = std::fs::read_dir(root.join(domain))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
            .collect();
        sources.sort();
        cc.args(sources);
    }
    cc.arg(root.join("desktop_connection.c"))
        .arg(root.join("tests").join(format!("{name}.c")))
        .arg("-o")
        .arg(&binary);
    assert!(cc.status().unwrap().success(), "C99 strict build failed");
    binary
}
#[test]
fn independent_vectors_and_pipeline_regressions() {
    let scratch = Scratch::new();
    for test in [
        "sophia_9p_client_test",
        "sophia_shell_files_test",
        "sophia_shell_files_roles_test",
    ] {
        assert!(
            Command::new(compile(&scratch.0, test))
                .status()
                .unwrap()
                .success()
        );
    }
}
#[test]
fn c_session_negotiates_uploads_and_cancels_over_native_files() {
    session_peer("sophia_shell_files_peer");
}
#[test]
fn public_c_session_tracks_custody_and_uploads_against_production_owners() {
    session_peer("desktop_session_peer");
}
fn session_peer(name: &str) {
    let scratch = Scratch::new();
    let binary = compile(&scratch.0, name);
    let directory = scratch.0.join("socket");
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let grant = ContentGrant {
        connection_epoch: 17,
        content_grant_epoch: 1,
    };
    transport
        .reserve_content(&mut registry, ContentLimits::prototype(grant))
        .unwrap();
    let mut peer = Command::new(binary)
        .arg(transport.socket_path())
        .arg(
            (SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE)
                .to_string(),
        )
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: peer.id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
        .begin_file_negotiation(
            &registry,
            grant.connection_epoch,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    peer.stdin.take().unwrap().write_all(b"G").unwrap();
    let start = Instant::now();
    let mut negotiated = false;
    let status = loop {
        if let Some(status) = peer.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = peer.kill();
            let _ = peer.wait();
            panic!("C peer exceeded bounded deadline");
        }
        let result = if !negotiated {
            transport
                .poll_negotiation(&mut registry, 64 * 1024)
                .map(|welcome| {
                    if welcome.is_some() {
                        negotiated = true;
                    }
                })
        } else {
            transport
                .service_content_allocation_requests(
                    &mut registry,
                    &[],
                    start.elapsed().as_millis() as u64,
                )
                .and_then(|_| {
                    transport.service_content_resources(
                        &mut registry,
                        start.elapsed().as_millis() as u64,
                    )
                })
                .map(|_| ())
        };
        if let Err(error) = result {
            let status = loop {
                if let Some(status) = peer.try_wait().unwrap() {
                    break status;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    let _ = peer.kill();
                    let _ = peer.wait();
                    panic!("C peer failed to exit after {error}");
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            assert!(status.success(), "C peer failed after {error}");
            break status;
        }
        std::thread::sleep(Duration::from_micros(100));
    };
    assert!(status.success());
    assert!(negotiated);
    let lease = transport
        .lease_content_resource(
            &registry,
            grant,
            ContentResourceId {
                id: 1,
                generation: 1,
            },
        )
        .unwrap();
    assert_eq!(lease.bytes(), &[0u8; 65536]);
    drop(lease);
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
}

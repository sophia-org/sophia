//! Independent SDK startup against the real export with supplied admission.
//! No protected launch, owner effects or physical presentation are claimed.
use sophia_protocol::*;
use sophia_runtime::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "requires SOPHIA_SHELL_STARTUP_PEER naming an absolute independently built SDK peer"]
fn independent_sdk_startup_reads_negotiation_and_optional_limits() {
    let peer = PathBuf::from(std::env::var_os("SOPHIA_SHELL_STARTUP_PEER").expect("SDK peer"));
    assert!(peer.is_absolute() && peer.is_file());
    for content in [false, true] {
        let directory = Directory(std::env::temp_dir().join(format!(
            "shell-sdk-startup-{}-{content}",
            std::process::id()
        )));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory.0,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        if content {
            transport
                .reserve_content(
                    &mut epochs,
                    ContentLimits::prototype(ContentGrant {
                        connection_epoch: 41,
                        content_grant_epoch: 7,
                    }),
                )
                .unwrap();
        }
        let mut child = ChildGuard(
            Command::new(&peer)
                .arg(transport.socket_path())
                .arg(if content { "content" } else { "metadata" })
                .stdin(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: child.0.id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        transport
            .begin_descriptor_file_negotiation(
                &epochs,
                41,
                Duration::from_secs(4),
                if content {
                    ShellContentAdmissionPolicy::Granted {
                        discrete_input: false,
                    }
                } else {
                    ShellContentAdmissionPolicy::Unavailable
                },
            )
            .unwrap();
        child.0.stdin.take().unwrap().write_all(&[1]).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut negotiated = false;
        loop {
            if negotiated {
                match transport.poll_io(&mut epochs) {
                    Ok(()) | Err(ShellTransportError::NotConnected) => (),
                    Err(e) => panic!("{e:?}"),
                }
            } else if let Some(welcome) = transport.poll_negotiation(&mut epochs, 65536).unwrap() {
                assert_eq!(welcome.connection_epoch, 41);
                assert_eq!(welcome.selected_revision, 8);
                negotiated = true;
                transport
                    .begin_candidate_request(
                        &mut epochs,
                        TransactionId::from_raw(17),
                        &ShellV1DescriptorSnapshot {
                            connection_epoch: 41,
                            snapshot_generation: 17,
                            output: OutputId::from_raw(8),
                            output_generation: 9,
                            broker_epoch: 4,
                            broker_revocation_epoch: 5,
                            descriptors: Vec::new(),
                        },
                    )
                    .unwrap();
            }
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "SDK peer failed: {status}");
                break;
            }
            assert!(Instant::now() < until, "SDK startup timeout");
            std::thread::yield_now();
        }
        assert!(negotiated);
        transport.disconnect(&mut epochs).unwrap();
        assert!(
            transport
                .collect_content_accounting(&mut epochs)
                .quiescent()
        );
    }
}

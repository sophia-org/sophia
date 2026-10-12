//! Supplied admission over a real SDK pipeline, not a Factotum verdict.
#[allow(dead_code)]
#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use file_wire_peer::{EPOCH, Peer, Profile, WAIT};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::{os::unix::net::UnixListener, time::Instant};

#[test]
fn shell_profiles_continue_on_the_exact_attached_pipeline() {
    for (role, revision, capabilities) in [
        ("bar", 6, 0x83),
        ("launcher", 7, 0x9a0),
        ("dock", 8, 0x11a2),
        ("descriptor", 8, 3),
    ] {
        let path =
            std::env::temp_dir().join(format!("shell-attached-{role}-{}", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        let profile = Profile {
            role,
            revision,
            capabilities,
            limits_published: role != "descriptor",
        };
        let peer = std::thread::spawn(move || Peer::handshake(listener, profile));
        let mut pipeline = Pipeline::connect(&path, PipelineLimits::default(), WAIT).unwrap();
        let (tag, root) = pipeline.attach(b"supplied", role.as_bytes()).unwrap();
        assert!(matches!(
            pipeline.wait(tag, Instant::now() + WAIT).unwrap(),
            Reply::Attach(_)
        ));
        let connection = ShellConnection::from_attached(
            pipeline,
            root,
            ShellClientOptions {
                minimum_revision: revision,
                maximum_revision: revision,
                required_capabilities: capabilities,
                handshake_timeout: WAIT,
            },
        )
        .unwrap();
        let peer = peer.join().unwrap();
        assert_eq!(
            peer.attaches, 1,
            "shell handoff preserves the existing attach"
        );
        assert_eq!(connection.connection_epoch(), EPOCH);
        assert_eq!(connection.welcome().selected_revision, revision);
        assert_eq!(
            peer.acks.iter().map(|ack| ack.sequence).collect::<Vec<_>>(),
            [1, 2]
        );
        std::fs::remove_file(path).unwrap();
    }
}

//! A deferred publication batch leaves through the same authority boundary as
//! any other flush. Revoking the supplied authority while a publication is
//! queued, or unwinding out of the batch, delivers nothing to the peer. The
//! authority is a supplied test verdict, not launch authentication evidence.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;
#[path = "support/revocable_shell_authority.rs"]
mod authority;
use authority::{Authority, Factory};

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq)]
enum Batch {
    /// Control: the same harness delivers when authority stays live.
    Live,
    RevokedOk,
    RevokedError,
    Unwound,
}

#[test]
fn live_supplied_authority_delivers_the_batched_publication() {
    run(Batch::Live);
}

#[test]
fn revocation_during_batch_delivers_nothing_and_reports_not_connected() {
    run(Batch::RevokedOk);
}

#[test]
fn revocation_during_batch_keeps_the_callback_error_class() {
    run(Batch::RevokedError);
}

#[test]
fn unwinding_out_of_a_batch_disconnects_without_delivery() {
    run(Batch::Unwound);
}

fn transport(authority: &Arc<Authority>) -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-batch-authority-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(Factory(authority.clone())),
        )
        .unwrap();
    (transport, directory)
}

/// Version, afid, attach through that afid, then the role files.
fn authenticated_setup(peer: &mut Peer) {
    let version = [
        65536u32.to_le_bytes().as_slice(),
        &8u16.to_le_bytes(),
        b"9P2000.L",
    ]
    .concat();
    assert_eq!(peer.rpc(100, &version).unwrap().0, 101);
    let auth = [
        30u32.to_le_bytes().as_slice(),
        &[0; 4],
        &u32::MAX.to_le_bytes(),
    ]
    .concat();
    assert_eq!(peer.rpc(102, &auth).unwrap().0, 103);
    let attach = [
        1u32.to_le_bytes().as_slice(),
        &30u32.to_le_bytes(),
        &[0; 4],
        &u32::MAX.to_le_bytes(),
    ]
    .concat();
    assert_eq!(peer.rpc(104, &attach).unwrap().0, 105);
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
}

fn facts() -> Vec<ContentOutputFactsEntry> {
    vec![ContentOutputFactsEntry {
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        local_width: 64,
        local_height: 64,
        scale_numerator: 1,
        scale_denominator: 1,
        scale_generation: 1,
    }]
}

fn run(batch: Batch) {
    let authority = Arc::new(Authority::default());
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport(&authority);
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    transport
        .reserve_content(&mut registry, ContentLimits::prototype(grant))
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let (settled_tx, settled) = mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        authenticated_setup(&mut peer);
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        settled_tx.send(()).unwrap();
        // This read waits in the server while the batch queues its record.
        // Raw reply: a refusal arrives as Rlerror, not as the helper's data.
        let read = [
            2u32.to_le_bytes().as_slice(),
            &peer.event_offset().to_le_bytes(),
            &65500u32.to_le_bytes(),
        ]
        .concat();
        peer.rpc(116, &read)
    });
    transport
        .begin_file_negotiation(
            &registry,
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let start = Instant::now();
    while transport
        .poll_negotiation(&mut registry, 64 * 1024)
        .unwrap()
        .is_none()
    {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    while settled.try_recv().is_err() {
        transport.poll_io(&mut registry).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "peer did not acknowledge Negotiated"
        );
        std::thread::yield_now();
    }
    // Give the peer's events read time to reach the server and wait there, so
    // a final flush would answer it. The owner turn then serves input.
    for _ in 0..20 {
        transport.poll_io(&mut registry).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    transport.service_owner_turn(&mut registry).unwrap();
    let polls = authority.polls.load(Ordering::SeqCst);
    assert!(polls >= 2, "the owner turn polls the supplied authority");

    let revoking = authority.clone();
    let publish = move |connection: &mut ShellTransportConnection<'_>| {
        connection
            .publish_content_output_facts(TransactionId::from_raw(101), 1, facts())
            .unwrap();
        // Deferred: nothing leaves inside the batch.
        connection.poll_io().unwrap();
        if batch != Batch::Live {
            revoking.revoke();
        }
    };
    match batch {
        Batch::Live => {
            let result = transport
                .connection(&mut registry)
                .with_publication_batch(|connection| {
                    publish(connection);
                    Ok::<_, ShellTransportError>(7)
                });
            assert_eq!(result, Ok(7));
        }
        Batch::RevokedOk => {
            let result = transport
                .connection(&mut registry)
                .with_publication_batch(|connection| {
                    publish(connection);
                    Ok::<_, ShellTransportError>(7)
                });
            assert_eq!(
                result,
                Err(ShellTransportError::NotConnected),
                "the boundary flush meets revocation before any reply"
            );
        }
        Batch::RevokedError => {
            let result = transport
                .connection(&mut registry)
                .with_publication_batch(|connection| {
                    publish(connection);
                    Err::<(), _>(ShellTransportError::WrongTransaction)
                });
            assert_eq!(
                result,
                Err(ShellTransportError::WrongTransaction),
                "a revoked flush must not replace the callback's error"
            );
        }
        Batch::Unwound => {
            let unwound = catch_unwind(AssertUnwindSafe(|| {
                let _ = transport
                    .connection(&mut registry)
                    .with_publication_batch(|connection| {
                        publish(connection);
                        panic!("role visit unwinds with a queued publication");
                        #[allow(unreachable_code)]
                        Ok::<(), ShellTransportError>(())
                    });
            }));
            assert!(unwound.is_err());
            assert_eq!(
                transport.poll_io(&mut registry),
                Err(ShellTransportError::NotConnected),
                "an unwound batch leaves no connected wire"
            );
            assert_eq!(
                transport
                    .connection(&mut registry)
                    .with_publication_batch(|_| Ok::<_, ShellTransportError>(())),
                Err(ShellTransportError::InvalidServiceBatch),
                "a later batch needs a fresh owner turn on a fresh wire"
            );
        }
    }

    if batch == Batch::Live {
        // No further service: the boundary flush itself answered the waiting
        // read, so the revoked cases exercise a flush that would deliver.
        while !peer.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "the batch boundary flush did not answer the waiting read"
            );
            std::thread::yield_now();
        }
        let (kind, body) = peer.join().unwrap().expect("live batch delivers");
        assert_eq!(kind, 117);
        let published = decode_shell_file_object_published(&body[4..]).unwrap();
        assert_eq!(published.object, ShellFileKind::Outputs);
        assert_eq!(published.generation, 1);
    } else {
        if batch != Batch::Unwound {
            // Revoked: the transport stays fail-closed on later service.
            for _ in 0..4 {
                let _ = transport.poll_io(&mut registry);
            }
        }
        let _ = transport.disconnect(&mut registry);
        // Teardown may answer the waiting read with an error or close the
        // socket; it may never answer it with data.
        if let Ok((kind, body)) = peer.join().unwrap() {
            assert_eq!(
                (kind, body.len()),
                (7, 4),
                "no publication may reach the peer after revocation or unwind"
            );
        }
    }
    let _ = transport.disconnect(&mut registry);
    drop(transport);
    assert_eq!(authority.retired.load(Ordering::SeqCst), 1, "one cleanup");
    let _ = std::fs::remove_dir_all(directory);
}

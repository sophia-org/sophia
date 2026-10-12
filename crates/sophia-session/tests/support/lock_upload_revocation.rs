//! Actual afid/relay authentication and lock upload, with a supplied receive
//! schedule that revokes between fragments before idle transport cleanup.
use super::*;
use sophia_9p::{Connection, connection::InPlaceTarget};
use sophia_runtime::authenticated_files::{AuthenticatedFiles, FileAuthenticationFactory};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixListener;

#[test]
fn revoked_launch_refuses_the_next_in_place_upload_fragment() {
    let peer = Peer::build(false);
    let root = Directory(std::env::temp_dir().join(format!("t317-upload-{}", std::process::id())));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root.0)
        .unwrap();
    let reports = root.0.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("progress");
    let sockets = root.0.join("sockets");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&sockets)
        .unwrap();
    let socket = sockets.join("endpoint");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut f = Fixture::new();
    let id = f
        .book
        .reserve(
            LaunchPolicy {
                principal: 8,
                namespace: 9,
                audience: 10,
                role: ProtectionDomainRole::LockProvider,
            },
            Instant::now(),
            deadline(),
        )
        .unwrap();
    let end = deadline();
    while f.book.phase(id) != Some(Phase::Ready) {
        f.tick(end);
    }
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
        .unwrap()
        .path(ProtectionPath::read_only(&sockets))
        .unwrap()
        .path(ProtectionPath::read_write(&reports))
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::LockProvider,
        ProcessLaunchSpec::new(&peer.0)
            .arg(&socket)
            .arg(&report)
            .arg("upload")
            .protection_domain(domain),
    );
    supervisor
        .start_with_launch_channel(f.book.take_channel(id, Instant::now()).unwrap())
        .unwrap();
    f.activate(id, &mut supervisor);
    let worker = f
        .book
        .worker_authority(id, &mut supervisor, Instant::now())
        .unwrap();
    let mut factory = TransportBudget::default()
        .endpoint()
        .bind(worker, b"lock".to_vec(), b"lock-provider".to_vec())
        .unwrap();
    let mut export = AuthenticatedFiles::pending(super::super::lock_authentication::view());
    let connection_id = ConnectionId(12);
    export
        .install(
            factory
                .create(connection_id, Instant::now(), f.wake.clone())
                .unwrap(),
        )
        .unwrap();
    let limits = Limits::new(65_536, 512, 16, 32, 131_072, 1).unwrap();
    let mut connection = Connection::new(connection_id, None, limits);
    let end = deadline();
    let mut stream = loop {
        f.tick(end);
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::yield_now(),
            Err(e) => panic!("accept protected upload peer: {e}"),
        }
    };
    stream.set_nonblocking(true).unwrap();
    let mut revoked = false;
    let mut prefix = 0;
    let mut discarded = 0;
    let end = deadline();
    loop {
        f.tick(end);
        // No idle authentication poll after revocation: the next fragment's
        // check must reject custody itself, before obtaining a destination.
        if !revoked {
            export.poll(Instant::now()).unwrap();
        }
        if let Some(target) = connection.in_place_target(&mut export) {
            if !revoked {
                let InPlaceTarget::Destination(destination) = target else {
                    panic!("an authenticated resource upload must receive its prefix");
                };
                assert_eq!(destination.len(), 8 - prefix);
                match stream.read(&mut destination[..4 - prefix]) {
                    Ok(0) => panic!("upload peer closed before the prefix arrived"),
                    Ok(n) => {
                        assert!(destination[..n].iter().all(|byte| *byte == 0x5a));
                        prefix += n;
                        connection.received_in_place(&mut export, n).unwrap();
                        assert!(
                            connection.output().is_empty(),
                            "a prefix cannot commit a write"
                        );
                        assert!(export.files().custody().upload_binding(0).is_some());
                        if prefix == 4 {
                            f.book.revoke(id, Instant::now()).unwrap();
                            revoked = true;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => panic!("upload prefix read: {e}"),
                }
            } else {
                assert!(
                    matches!(target, InPlaceTarget::Discard(_)),
                    "revoked launch must refuse the next upload destination before idle cleanup"
                );
                let InPlaceTarget::Discard(remaining) = target else {
                    unreachable!()
                };
                assert_eq!(remaining, 4 - discarded);
                let mut scratch = [0; 4];
                match stream.read(&mut scratch[..remaining]) {
                    Ok(0) => panic!("upload peer closed before the refused write drained"),
                    Ok(n) => {
                        discarded += n;
                        connection.received_in_place(&mut export, n).unwrap();
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => panic!("discard upload: {e}"),
                }
            }
        } else if !revoked {
            // Read headers separately so none of the payload is accidentally
            // buffered before the test's chosen fragment boundary.
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(1) => assert_eq!(connection.receive(&mut export, &byte).unwrap(), 1),
                Ok(_) => panic!("peer closed before upload"),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => panic!("receive authentication/upload header: {e}"),
            }
        }
        if !connection.output().is_empty() {
            match stream.write(connection.output()) {
                Ok(n) => connection.sent(n),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => panic!("reply to upload peer: {e}"),
            }
        }
        if !revoked {
            connection.resume(&mut export).unwrap();
            connection.retry_waiting(&mut export).unwrap();
        }
        if supervisor.poll().unwrap().is_some() {
            break;
        }
        std::thread::yield_now();
    }
    assert!(supervisor.take_exit_status().unwrap().success());
    assert!(revoked);
    assert_eq!(discarded, 4);
    assert_eq!(std::fs::read_to_string(report).unwrap(), "upload-revoked\n");
    connection.close(&mut export);
}

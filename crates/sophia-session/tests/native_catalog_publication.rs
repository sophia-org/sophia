#![cfg(feature = "native-session")]
//! Real private transport, supplied protection and real bounded FIFO ownership.
//! No supervised child, display, native renderer or application execution.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::application_catalog::*;
use std::io::Read;
#[allow(dead_code)]
#[path = "../../sophia-runtime/tests/support/native_launcher_socket.rs"]
mod socket;
use socket::*;
#[path = "../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod shell_file_peer;

fn connected(epochs: &mut ContentEpochRegistry, limits: ContentLimits) -> Peer {
    let mut peer = Peer::with_limits(epochs, ContentStoreProfile::NativeLauncher, limits);
    peer.negotiate(epochs, hello(), granted()).unwrap();
    peer.read(); // actual Welcome
    peer.read(); // actual Limits
    peer
}
fn publication(epoch: u64, count: usize) -> PublishedApplicationCatalog {
    let registered = (0..count)
        .map(|id| RegisteredCatalogApplication {
            name: format!("app{id:04}"),
            command: ApplicationLaunchCommand {
                executable: std::env::current_exe().unwrap(),
                arguments: vec![],
                working_directory: None,
            },
        })
        .collect::<Vec<_>>();
    let source = build_application_catalog(
        &sophia_config::ApplicationCatalogConfig {
            name: "fixture".into(),
            sources: vec![],
            applications: registered.iter().map(|entry| entry.name.clone()).collect(),
            terminal: None,
            terminal_arguments: vec![],
        },
        &registered,
        &ApplicationCatalogEnvironment {
            search_path: vec![],
            locale: "C".into(),
            current_desktop: vec![],
        },
    )
    .unwrap();
    assert_eq!(source.entries.len(), count);
    PublishedApplicationCatalog::new(epoch, 8, source).unwrap()
}

/// One length-prefixed IPC frame from `client`, or `None` if none has
/// arrived within a short read timeout. `read_exact`'s first underlying
/// read either returns the whole header at once or times out having
/// consumed nothing (the sender, `poll_io`, writes each turn's whole flush
/// synchronously before returning, so a header is never split across
/// turns): only that first read is short-timed, so a `None` never loses
/// bytes already on the wire.
fn try_read_frame(client: &mut std::os::unix::net::UnixStream) -> Option<Vec<u8>> {
    client
        .set_read_timeout(Some(std::time::Duration::from_millis(200)))
        .unwrap();
    let mut bytes = vec![0u8; SOPHIA_IPC_HEADER_LEN];
    let header = client.read_exact(&mut bytes);
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    match header {
        Ok(()) => {
            let n = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
            assert!(n <= 65536);
            bytes.resize(SOPHIA_IPC_HEADER_LEN + n, 0);
            client
                .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
                .unwrap();
            Some(bytes)
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// Drives `poll_io` until exactly `count` frames have arrived, without
/// assuming how many the transport's bulk budget lets through on any one
/// turn: a publication larger than the budget now drains across however
/// many turns that takes, decided by the transport, not by this test.
fn drain_frames(
    peer: &mut Peer,
    epochs: &mut ContentEpochRegistry,
    count: usize,
    deadline: std::time::Instant,
) -> Vec<Vec<u8>> {
    let mut received = Vec::with_capacity(count);
    while received.len() < count {
        peer.transport.poll_io(epochs).unwrap();
        while received.len() < count
            && let Some(frame) = try_read_frame(&mut peer.client)
        {
            received.push(frame);
        }
        if received.len() < count {
            assert!(std::time::Instant::now() < deadline, "drain timed out");
            std::thread::yield_now();
        }
    }
    received
}

/// t252 B5 (corrected transport semantics, `shell_transport/publication.rs`
/// `queue_publication`/`flush_publication`): `publish_catalog` takes custody
/// of the whole publication whenever no earlier one is still draining, even
/// if the output queue has no room for any of it yet, and pushes what fits
/// now, draining the rest across later `poll_io` turns. A publication still
/// draining refuses a competing one and changes nothing.
#[test]
fn catalog_publication_is_accepted_under_saturation_drains_whole_and_in_order_and_precedes_opening()
{
    let mut epochs = empty();
    let mut peer = connected(&mut epochs, limits());
    // Larger than one visit's bulk budget: several `poll_io` turns are
    // needed to drain it, decided by the transport now, not by Session.
    let source = publication(GRANT.connection_epoch, 70);
    let expected = source.frames(tx(50)).unwrap();
    let mut transfer =
        NativeCatalogPublication::new(&peer.transport.connection(&mut epochs), tx(50), source)
            .unwrap();
    assert!(transfer.published().is_none());

    let filler = publication(GRANT.connection_epoch, 0)
        .frames(tx(99))
        .unwrap()
        .remove(0);
    let mut queued = 0;
    loop {
        match peer.transport.enqueue_async(&epochs, filler.clone()) {
            Ok(()) => queued += 1,
            Err(ShellTransportError::ActivationQueueSaturated) => break,
            Err(error) => panic!("{error}"),
        }
        assert!(queued <= 1024);
    }
    assert!(queued > 0);

    // The output queue is fully saturated by filler, yet the publication is
    // still accepted: custody moves to the transport immediately, before a
    // single byte of it can be written.
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    assert!(transfer.published().is_some());

    // A competing publish while this one is still draining is refused and
    // takes nothing: what eventually arrives is exactly the first catalog.
    let competing = publication(GRANT.connection_epoch, 0).value(false).unwrap();
    assert!(matches!(
        peer.transport.publish_catalog(&epochs, tx(60), &competing),
        Err(ShellTransportError::ActivationQueueSaturated)
    ));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

    // Only the filler comes off the wire until it is fully drained: the
    // catalog has had no room to start writing yet.
    for frame in drain_frames(&mut peer, &mut epochs, queued, deadline) {
        assert_eq!(frame, filler);
    }

    // Once capacity frees, the whole catalog drains -- across however many
    // `poll_io` turns the transport's bulk budget takes -- whole, in order,
    // and byte-identical to the direct encoder.
    let received = drain_frames(&mut peer, &mut epochs, expected.len(), deadline);
    assert_eq!(received, expected, "byte-identical to the direct encoder");

    // A second visit after publication is a harmless no-op.
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );

    // The catalog precedes the Opening it gates on the same FIFO: it is
    // read out here only after every catalog byte already has been.
    peer.transport
        .publish_native_launcher_opening(&epochs, tx(51), opening())
        .unwrap();
    let opening_frame = drain_frames(&mut peer, &mut epochs, 1, deadline)
        .pop()
        .unwrap();
    assert_eq!(
        decode_shell_native_launcher_frame(&opening_frame)
            .unwrap()
            .1,
        ShellNativeLauncherRecord::Opening(opening())
    );
}

#[test]
fn catalog_transfer_requires_exact_current_grant_not_just_connection_epoch() {
    let mut old_epochs = empty();
    let mut old = connected(&mut old_epochs, limits());
    let mut transfer = NativeCatalogPublication::new(
        &old.transport.connection(&mut old_epochs),
        tx(50),
        publication(GRANT.connection_epoch, 0),
    )
    .unwrap();
    let mut successor_limits = limits();
    successor_limits.grant.content_grant_epoch += 1;
    let mut new_epochs = empty();
    let mut new = connected(&mut new_epochs, successor_limits);
    assert!(matches!(
        transfer.service(&mut new.transport.connection(&mut new_epochs)),
        Err(ShellTransportError::WrongContentGrant)
    ));
    assert!(transfer.published().is_none());
    assert!(
        transfer
            .service(&mut old.transport.connection(&mut old_epochs))
            .unwrap()
    );
    old.transport.poll_io(&mut old_epochs).unwrap();
    let actual = vec![old.read(), old.read()];
    assert_eq!(decode_shell_application_catalog(&actual).unwrap().0, tx(50));
    assert!(matches!(
        NativeCatalogPublication::new(
            &new.transport.connection(&mut new_epochs),
            tx(51),
            publication(GRANT.connection_epoch + 1, 0)
        ),
        Err(ShellTransportError::WrongContentGrant)
    ));
    old.transport.disconnect(&mut old_epochs).unwrap();
    assert!(matches!(
        transfer.service(&mut old.transport.connection(&mut old_epochs)),
        Err(ShellTransportError::WrongContentGrant)
    ));
}

#[test]
fn opening_waits_for_publication_and_retains_exact_transfer_under_saturation() {
    use sophia_session::shell_native_launcher::NativeLauncherContentService;
    let mut epochs = empty();
    let mut peer = connected(&mut epochs, limits());
    let mut publication = NativeCatalogPublication::new(
        &peer.transport.connection(&mut epochs),
        tx(50),
        publication(GRANT.connection_epoch, 0),
    )
    .unwrap();
    let mut content =
        NativeLauncherContentService::new(&peer.transport.connection(&mut epochs)).unwrap();
    let outputs = [sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 800,
            height: 600,
        },
        scale: 1,
    }];
    let mut serial = 100;
    let mut next = || {
        serial += 1;
        Ok(tx(serial))
    };
    assert!(content.request_open(outputs[0].id, 7));
    assert!(!content.request_open(outputs[0].id, 8));
    assert!(
        !content
            .service_open_request(
                &mut peer.transport.connection(&mut epochs),
                &publication,
                &outputs,
                &mut next
            )
            .unwrap()
    );
    assert!(peer.transport.native_launcher_state().is_none());
    assert!(
        publication
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    content
        .publish_outputs(
            &mut peer.transport.connection(&mut epochs),
            &outputs,
            &mut next,
        )
        .unwrap();
    peer.transport.poll_io(&mut epochs).unwrap();
    let frames = vec![peer.read(), peer.read()];
    assert_eq!(
        decode_shell_application_catalog(&frames)
            .unwrap()
            .1
            .entries
            .len(),
        0
    );
    assert!(matches!(
        decode_shell_content_frame(&peer.read()).unwrap().1,
        ShellContentRecord::OutputFacts(_)
    ));
    let filler = publication
        .published()
        .unwrap()
        .frames(tx(99))
        .unwrap()
        .remove(0);
    let mut queued = 0;
    loop {
        match peer.transport.enqueue_async(&epochs, filler.clone()) {
            Ok(()) => queued += 1,
            Err(ShellTransportError::ActivationQueueSaturated) => break,
            Err(e) => panic!("{e}"),
        }
        assert!(queued <= 1024);
    }
    assert!(queued > 0);
    assert!(
        !content
            .service_open_request(
                &mut peer.transport.connection(&mut epochs),
                &publication,
                &outputs,
                &mut next
            )
            .unwrap()
    );
    assert!(
        !content
            .service_open_request(
                &mut peer.transport.connection(&mut epochs),
                &publication,
                &outputs,
                &mut next
            )
            .unwrap()
    );
    assert!(peer.transport.native_launcher_state().is_none());
    for group in (0..queued).collect::<Vec<_>>().chunks(32) {
        peer.transport.poll_io(&mut epochs).unwrap();
        for _ in group {
            assert_eq!(peer.read(), filler);
        }
    }
    assert!(
        content
            .service_open_request(
                &mut peer.transport.connection(&mut epochs),
                &publication,
                &outputs,
                &mut next
            )
            .unwrap()
    );
    assert!(
        !content
            .service_open_request(
                &mut peer.transport.connection(&mut epochs),
                &publication,
                &outputs,
                &mut next
            )
            .unwrap()
    );
    assert!(
        !content
            .service_focus(&mut peer.transport.connection(&mut epochs), tx(200))
            .unwrap()
    );
    assert!(peer.transport.native_launcher_focus().is_none());
    peer.transport.poll_io(&mut epochs).unwrap();
    let (transaction, record) = decode_shell_native_launcher_frame(&peer.read()).unwrap();
    assert_eq!(
        transaction,
        tx(102),
        "facts once, opening transfer once despite retries"
    );
    assert_eq!(serial, 102);
    assert!(
        matches!(record, ShellNativeLauncherRecord::Opening(v) if v.opening == 7
        && v.output.id == outputs[0].id.raw() && v.catalog_generation == 8)
    );
    assert!(
        !content.request_open(outputs[0].id, 9),
        "active opening is not replaced by another request"
    );
}

/// t252 B5: the dock's r8 catalog transfer now hands the whole typed value
/// to `publish_catalog` instead of building socket frames, so it reaches a
/// dock connected over the file wire as the `Catalog` object with identities
/// -- the same object the runtime-level file-wire test exercises directly
/// (`shell_persistent_catalog_files.rs`), now reached only through Session's
/// own `NativeCatalogPublication`.
#[test]
fn a_file_wire_dock_receives_the_catalog_object_with_identities_when_session_publishes() {
    const DOCK_CAPS: u64 = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
        | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
        | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;

    let mut epochs = empty();
    let directory = std::env::temp_dir().join(format!(
        "session-native-catalog-files-{}-{}",
        std::process::id(),
        std::time::Instant::now().elapsed().as_nanos()
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
        .reserve_content_with_profile(
            &mut epochs,
            limits(),
            ContentStoreProfile::PersistentCatalog,
        )
        .unwrap();
    let socket = transport.socket_path().to_owned();

    let (negotiated_tx, negotiated_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = shell_file_peer::Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: GRANT.connection_epoch,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 8,
                maximum_revision: 8,
                required_capabilities: DOCK_CAPS,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(value.welcome.selected_revision, 8);
        assert_eq!(value.welcome.capabilities, DOCK_CAPS);
        peer.ack(&negotiated);
        negotiated_tx.send(()).unwrap();

        // The whole catalog, with r8 identities, published as one object by
        // Session's typed `NativeCatalogPublication::service`.
        let published = peer.next_event();
        let announced = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(announced.object, ShellFileKind::Catalog);
        peer.ack(&published);
        peer.open(6, b"catalog", 0);
        decode_shell_file_catalog(&peer.read(6, 0)).unwrap()
    });

    let start = std::time::Instant::now();
    transport
        .begin_file_negotiation(
            &epochs,
            GRANT.connection_epoch,
            std::time::Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
        .unwrap();
    let welcome = loop {
        if let Some(welcome) = transport.poll_negotiation(&mut epochs, 64 * 1024).unwrap() {
            break welcome;
        }
        assert!(!peer.is_finished(), "peer ended before negotiation");
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        std::thread::yield_now();
    };
    assert_eq!(welcome.capabilities, DOCK_CAPS);

    while negotiated_rx.try_recv().is_err() {
        transport.poll_io(&mut epochs).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        std::thread::yield_now();
    }

    let source = publication(GRANT.connection_epoch, 2);
    let mut publish =
        NativeCatalogPublication::new(&transport.connection(&mut epochs), tx(1), source).unwrap();
    assert!(
        publish
            .service(&mut transport.connection(&mut epochs))
            .unwrap()
    );

    while !peer.is_finished() {
        transport.poll_io(&mut epochs).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        std::thread::yield_now();
    }
    let received = peer.join().unwrap();
    assert_eq!(received.catalog.identities.len(), 2);
    assert_eq!(
        received.catalog.identities[&1],
        format!("registered:app{:04}", 0)
    );
    assert_eq!(
        received.catalog.identities[&2],
        format!("registered:app{:04}", 1)
    );
    assert_eq!(
        received.catalog.catalog,
        *publish.published().unwrap().wire()
    );

    transport.disconnect(&mut epochs).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

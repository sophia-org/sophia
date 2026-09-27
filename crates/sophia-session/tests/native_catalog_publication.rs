#![cfg(feature = "native-session")]
//! Real private transport, supplied protection and real bounded FIFO ownership.
//! No supervised child, display, native renderer or application execution.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::application_catalog::*;
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

#[test]
fn catalog_declines_whole_publication_under_saturation_and_precedes_opening_in_fifo() {
    // The transport now owns the whole publication as one typed value
    // (`publish_catalog`): under saturation it takes nothing, so there is no
    // partial front left to retain here any more. A retried `service` call
    // either publishes the exact same bytes as the direct encoder or, while
    // the queue has no room at all, publishes nothing.
    let mut epochs = empty();
    let mut peer = connected(&mut epochs, limits());
    // Small enough (with Begin/End) to fit under this connection's
    // `max_control_records` (64) in the one visit the retried call takes
    // once capacity frees; pacing a catalog too large for that is the
    // transport's job now, not Session's.
    let source = publication(GRANT.connection_epoch, 40);
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
    // Fully saturated: the transport cannot take even the first record of the
    // publication, so `service` declines and leaves it unpublished.
    assert!(
        !transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    assert!(transfer.published().is_none());
    // These were queue-only admissions. Draining is a separate production call.
    for group in (0..queued).collect::<Vec<_>>().chunks(32) {
        peer.transport.poll_io(&mut epochs).unwrap();
        for _ in group {
            assert_eq!(peer.read(), filler);
        }
    }
    // Capacity is free again: the retried call publishes the whole catalog
    // in one visit and reports it published.
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    assert!(transfer.published().is_some());
    // A second visit after publication is a harmless no-op.
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    peer.transport
        .publish_native_launcher_opening(&epochs, tx(51), opening())
        .unwrap();
    peer.transport.poll_io(&mut epochs).unwrap();
    let received: Vec<Vec<u8>> = (0..expected.len()).map(|_| peer.read()).collect();
    assert_eq!(received, expected, "byte-identical to the direct encoder");
    assert_eq!(
        decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
        ShellNativeLauncherRecord::Opening(opening()),
        "the catalog precedes the Opening it gates on the same FIFO"
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

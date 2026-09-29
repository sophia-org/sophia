#![cfg(feature = "native-session")]
//! Real file transport, supplied protection and bounded publication ownership.
//! No supervised child, display, native renderer or application execution.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::application_catalog::*;
#[allow(dead_code)]
#[path = "../../sophia-runtime/tests/support/native_files_peer.rs"]
mod files;
use files::*;
#[path = "../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod shell_file_peer;

fn connected(epochs: &mut ContentEpochRegistry, limits: ContentLimits) -> Peer {
    let mut peer = Peer::with_limits(epochs, ContentStoreProfile::NativeLauncher, limits);
    peer.negotiate(epochs);
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

fn filler() -> ShellContentRecord {
    ShellContentRecord::ResourceStatus(ContentResourceStatus {
        grant: GRANT,
        resource: ContentResourceId {
            id: 99,
            generation: 1,
        },
        status: 3,
        reason: ContentReason::Stale as u16,
        next_ordinal: 0,
        admitted_bytes: 0,
    })
}

fn saturate(peer: &mut Peer, epochs: &mut ContentEpochRegistry) -> usize {
    for count in 0..4096 {
        match peer
            .transport
            .send_content_record(epochs, tx(99), &filler())
        {
            Ok(()) => peer.transport.poll_io(epochs).unwrap(),
            Err(ShellTransportError::ContentQueueSaturated) => {
                assert!(count > 0);
                return count;
            }
            Err(error) => panic!("unexpected pressure failure: {error}"),
        }
    }
    panic!("journal and outbox did not saturate");
}

fn drain_filler(peer: &mut Peer, epochs: &mut ContentEpochRegistry, count: usize) {
    for _ in 0..count {
        assert_eq!(peer.read_content(epochs), (tx(99), filler()));
    }
}

/// File publication takes no custody under pressure. Session retains the
/// exact whole catalog for retry; its announcement cannot overtake old output.
#[test]
fn catalog_publication_retries_whole_under_saturation_and_precedes_opening() {
    let mut epochs = empty();
    let mut peer = connected(&mut epochs, limits());
    let source = publication(GRANT.connection_epoch, 1000);
    let expected = ShellFileCatalog {
        transaction: tx(50),
        catalog: source.value(false).unwrap(),
    };
    assert!(
        encode_shell_file_catalog_body(&expected).unwrap().len() > 65536,
        "catalog must require multiple reads"
    );
    let mut transfer =
        NativeCatalogPublication::new(&peer.transport.connection(&mut epochs), tx(50), source)
            .unwrap();
    let queued = saturate(&mut peer, &mut epochs);
    for _ in 0..2 {
        assert!(
            !transfer
                .service(&mut peer.transport.connection(&mut epochs))
                .unwrap()
        );
        assert!(transfer.published().is_none());
    }
    let competing = publication(GRANT.connection_epoch, 0).value(false).unwrap();
    assert!(matches!(
        peer.transport.publish_catalog(&epochs, tx(60), &competing),
        Err(ShellTransportError::ActivationQueueSaturated)
    ));

    // Checking every old record also refuses a catalog announcement that
    // bypassed the saturated outbox. No caller has successfully published yet.
    drain_filler(&mut peer, &mut epochs, queued);
    peer.no_event(&mut epochs);
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    assert!(transfer.published().is_some());
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    peer.transport
        .publish_native_launcher_opening(&epochs, tx(51), opening())
        .unwrap();
    assert_eq!(peer.read_catalog_object(&mut epochs), expected);
    assert_eq!(
        peer.read_native(&mut epochs),
        (tx(51), ShellNativeLauncherRecord::Opening(opening()))
    );
    peer.no_event(&mut epochs);
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
    assert_eq!(
        old.read_catalog_object(&mut old_epochs),
        ShellFileCatalog {
            transaction: tx(50),
            catalog: publication(GRANT.connection_epoch, 0).value(false).unwrap(),
        }
    );
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
    assert_eq!(
        peer.read_catalog_object(&mut epochs),
        ShellFileCatalog {
            transaction: tx(50),
            catalog: publication.published().unwrap().value(false).unwrap(),
        }
    );
    assert!(
        matches!(peer.read_content(&mut epochs), (transaction, ShellContentRecord::OutputFacts(_)) if transaction == tx(101))
    );
    let queued = saturate(&mut peer, &mut epochs);
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
    drain_filler(&mut peer, &mut epochs, queued);
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
    let (transaction, record) = peer.read_native(&mut epochs);
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

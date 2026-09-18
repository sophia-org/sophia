#![cfg(feature = "native-session")]
//! Real private transport, supplied protection and real bounded FIFO ownership.
//! No supervised child, display, native renderer or application execution.
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::application_catalog::*;
#[allow(dead_code)]
#[path = "../../sophia-runtime/tests/support/native_launcher_socket.rs"]
mod socket;
use socket::*;

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
fn catalog_retains_exact_front_on_saturation_and_precedes_opening_in_fifo() {
    let mut epochs = empty();
    let mut peer = connected(&mut epochs, limits());
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
    let mut received = vec![];
    for count in [32, 32, 8] {
        let complete = transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap();
        assert_eq!(complete, count == 8);
        assert_eq!(transfer.published().is_some(), complete);
        if complete {
            peer.transport
                .publish_native_launcher_opening(&epochs, tx(51), opening())
                .unwrap();
        }
        use std::io::Read;
        peer.client.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.client.read(&mut [0u8; 1]).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        peer.client.set_nonblocking(false).unwrap();
        peer.transport.poll_io(&mut epochs).unwrap();
        for _ in 0..count {
            received.push(peer.read());
        }
        assert_eq!(&received, &expected[..received.len()]);
    }
    assert_eq!(received, expected);
    assert_eq!(
        decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
        ShellNativeLauncherRecord::Opening(opening())
    );
    assert!(
        transfer
            .service(&mut peer.transport.connection(&mut epochs))
            .unwrap()
    );
    peer.transport.poll_io(&mut epochs).unwrap();
    use std::io::Read;
    peer.client.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.client.read(&mut [0u8; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
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

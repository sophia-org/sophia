//! File publication controls from shell_component_connections.rs at
//! 90806a8d5. The mixed-wire case now selects files for both roles; it keeps
//! its independent role/grant assertions. IPC interoperability retires with IPC.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn file_bar_and_launcher_negotiate_in_one_registry() {
    use sophia_protocol::shell_files::*;
    let directory = std::env::temp_dir().join(format!(
        "session-component-publication-files-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let mut owner = ShellComponentConnections::new().unwrap();
    let uid = rustix::process::geteuid().as_raw();
    owner
        .add_with_transport(
            "panel",
            ShellComponentRole::Bar,
            &directory.join("panel"),
            uid,
            sophia_config::ShellTransportSelection::NineP2000L,
        )
        .unwrap();
    owner
        .add_with_transport(
            "menu",
            ShellComponentRole::ApplicationLauncher,
            &directory.join("menu"),
            uid,
            sophia_config::ShellTransportSelection::NineP2000L,
        )
        .unwrap();
    let mut h = Harness { owner, directory };

    let bar = h.owner.reserve_attempt(0).unwrap();
    h.owner
        .begin_negotiation(
            bar,
            &evidence(),
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let socket = h.owner.socket_path(0).unwrap().to_owned();
    let epoch = bar.grant.connection_epoch;
    let peer = std::thread::spawn(move || {
        let mut peer = shell_file_peer::Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: epoch,
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
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);
        (peer, value)
    });
    let start = std::time::Instant::now();
    let welcome = loop {
        if let Some((key, result)) = h
            .owner
            .poll_negotiations(65536)
            .into_iter()
            .flatten()
            .next()
        {
            assert_eq!(key, bar);
            break result.unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    };
    // The owner loop keeps serving the export: the peer still acknowledges.
    while !peer.is_finished() {
        h.owner
            .with_connection(bar, |t| t.poll_io())
            .unwrap()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let (_peer, negotiated) = peer.join().unwrap();
    assert_eq!(negotiated.welcome, welcome);
    assert_eq!(welcome.connection_epoch, epoch);
    assert_eq!(welcome.selected_revision, 6);
    assert!(negotiated.limits_published);
    assert!(
        h.owner
            .with_connection(bar, |t| t.supports_content())
            .unwrap()
    );

    // The launcher keeps its own grant and profile beside the file bar.
    let menu = h.owner.reserve_attempt(1).unwrap();
    let _client = h.connect(menu);
    h.owner.close(bar).unwrap();
    h.owner.close(menu).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[cfg(feature = "native-session")]
#[test]
fn a_file_wire_bar_receives_the_indicators_object_when_session_publishes() {
    use sophia_protocol::shell_files::*;
    use sophia_session::shell_panel_service::PanelComponentService;
    let directory = std::env::temp_dir().join(format!(
        "session-component-indicators-files-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let mut owner = ShellComponentConnections::new().unwrap();
    let uid = rustix::process::geteuid().as_raw();
    owner
        .add_with_transport(
            "panel",
            ShellComponentRole::Bar,
            &directory.join("panel"),
            uid,
            sophia_config::ShellTransportSelection::NineP2000L,
        )
        .unwrap();
    let mut h = Harness { owner, directory };

    let bar = h.owner.reserve_attempt(0).unwrap();
    h.owner
        .begin_negotiation(
            bar,
            &evidence(),
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let socket = h.owner.socket_path(0).unwrap().to_owned();
    let epoch = bar.grant.connection_epoch;

    let (negotiated_tx, negotiated_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = shell_file_peer::Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: epoch,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
                    | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(
            value.welcome.capabilities & SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS,
            SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS,
            "bit 9 must be granted for the bar to see `indicators` at all"
        );
        peer.ack(&negotiated);
        negotiated_tx.send(()).unwrap();

        // The whole snapshot, published as one object by Session's typed call.
        let published = peer.next_event();
        let announced = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(announced.object, ShellFileKind::Indicators);
        peer.ack(&published);
        peer.open(6, b"indicators", 0);
        decode_shell_file_indicators(&peer.read(6, 0)).unwrap()
    });

    let start = std::time::Instant::now();
    let welcome = loop {
        if let Some((key, result)) = h
            .owner
            .poll_negotiations(65536)
            .into_iter()
            .flatten()
            .next()
        {
            assert_eq!(key, bar);
            break result.unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    };
    assert_eq!(welcome.connection_epoch, epoch);

    // Keep the file wire serviced while the peer finishes negotiating.
    while negotiated_rx.try_recv().is_err() {
        h.owner
            .with_connection(bar, |t| t.poll_io())
            .unwrap()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }

    let publication = sophia_engine::PolicyIndicatorPublication {
        tab_groups: vec![],
        generation: 9,
        connection_epoch: Some(epoch),
        indicators: vec![],
        output_statuses: vec![],
    };
    let mut service = h
        .owner
        .with_connection(bar, |t| PanelComponentService::new(t, 64, false))
        .unwrap()
        .unwrap();
    h.owner
        .with_connection(bar, |t| {
            service
                .service_indicators(t, Some(&publication), None)
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();

    while !peer.is_finished() {
        h.owner
            .with_connection(bar, |t| t.poll_io())
            .unwrap()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let received = peer.join().unwrap();
    let expected =
        sophia_session::shell_indicator_publication::indicator_snapshot(&publication, None, epoch);
    assert_eq!(received.snapshot, expected);

    h.owner.close(bar).unwrap();
    assert!(h.owner.collect().quiescent());
}

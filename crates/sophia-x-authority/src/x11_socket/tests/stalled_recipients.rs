// A peer that cannot take what it is owed is ended, and nobody else pays
// for it (t090).
//
// WHAT THESE PROVE. Two delivery paths that used to turn a recipient's full
// queue into the sender's failure: a Present completion fanned out to its
// subscribers, and a lifecycle notice (DestroyNotify) fanned out to the
// window's watchers. With a one-slot queue and a recipient that never reads,
// the sender's call succeeds, the healthy recipient is told, the stalled one
// reads EOF, and a newcomer still registers. Each fixture is its own
// negative control: without containment the routed call returns the queue
// error, and the assertion on it fails.

mod stalled_recipients {
    use super::*;
    use std::io::{Read, Write};
    use std::num::NonZeroUsize;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;

    fn filler() -> XClientEvent {
        XClientEvent::MappingNotify {
            sequence: 0,
            request: 2,
            first_keycode: 0,
            count: 0,
        }
    }

    struct Peer {
        _registration: XServerFrontendClientRouteRegistration,
        channels: XServerFrontendClientRouteChannels,
        socket: UnixStream,
    }

    /// A registered client whose connection input recovery can end, with the
    /// far end of its socket held here to read the ending.
    fn peer(broker: &XServerFrontendRouteBroker, client: XServerFrontendClientId) -> Peer {
        let (registration, channels) = broker.registry.register_client(client).unwrap();
        let (socket, far) = UnixStream::pair().unwrap();
        broker.registry.input_recovery.attach(client, socket).unwrap();
        far.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        Peer {
            _registration: registration,
            channels,
            socket: far,
        }
    }

    fn assert_ended(peer: &mut Peer, what: &str) {
        assert_eq!(
            peer.socket.read(&mut [0; 8]).unwrap(),
            0,
            "{what}: the stalled recipient reads EOF"
        );
        assert_eq!(peer.channels.protocol.try_recv().unwrap(), filler());
        assert!(
            peer.channels.protocol.try_recv().is_err(),
            "{what}: nothing was queued behind the full slot"
        );
    }

    fn assert_continues(broker: &XServerFrontendRouteBroker, healthy: &mut Peer, newcomer: u64) {
        healthy
            .socket
            .write_all(b"still here")
            .expect("the healthy recipient's connection is open");
        broker
            .registry
            .register_client(XServerFrontendClientId(newcomer))
            .expect("a newcomer is admitted after a recipient was ended");
    }

    #[test]
    fn a_present_subscriber_that_cannot_take_its_completion_is_ended_and_the_rest_are_told() {
        let namespace = NamespaceId::from_raw(90);
        let presenter = XServerFrontendClientId(1);
        let stalled_id = XServerFrontendClientId(2);
        let healthy_id = XServerFrontendClientId(3);
        let window = XResourceId::new(0x300001, 1);
        let pixmap = XResourceId::new(0x300002, 1);
        let stalled_event = XResourceId::new(0x400001, 1);
        let healthy_event = XResourceId::new(0x500001, 1);
        let transaction = TransactionId::from_raw(901);
        // One slot: the first event a recipient does not take fills it.
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(1).unwrap());
        let (_presenter_registration, _presenter_channels) =
            broker.registry.register_client(presenter).unwrap();
        let mut stalled = peer(&broker, stalled_id);
        let mut healthy = peer(&broker, healthy_id);
        broker
            .registry
            .register_surface(presenter, namespace, SurfaceId::new(31, 1), window)
            .unwrap();
        // The stalled subscriber sorts first, so without containment the
        // healthy one behind it would be skipped as well.
        broker
            .registry
            .select_present_input(stalled_id, stalled_event, window, 7)
            .unwrap();
        broker
            .registry
            .select_present_input(healthy_id, healthy_event, window, 7)
            .unwrap();
        broker
            .registry
            .queue_present(transaction, presenter, window, pixmap, 1, None, false)
            .unwrap();
        broker.registry.route_protocol(stalled_id, filler()).unwrap();

        assert_eq!(
            broker.route_present_complete(transaction, 10, 20, XPresentCompletionMode::Flip),
            Ok(true),
            "a stalled subscriber is not the presenter's failure"
        );
        let event = healthy
            .channels
            .protocol
            .recv_timeout(Duration::from_secs(1))
            .expect("the healthy subscriber is told");
        assert!(
            matches!(event, XClientEvent::PresentCompleteNotify { event_id, .. } if event_id == healthy_event),
            "{event:?}"
        );
        assert_ended(&mut stalled, "present");
        assert_continues(&broker, &mut healthy, 4);
    }

    #[test]
    fn a_lifecycle_watcher_that_cannot_take_a_destroy_notice_is_ended_and_the_rest_are_told() {
        let sender = XServerFrontendClientId(1);
        let stalled_id = XServerFrontendClientId(2);
        let healthy_id = XServerFrontendClientId(3);
        let window = XResourceId::new(0x300001, 1);
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(1).unwrap());
        let (_sender_registration, _sender_channels) =
            broker.registry.register_client(sender).unwrap();
        let mut stalled = peer(&broker, stalled_id);
        let mut healthy = peer(&broker, healthy_id);
        broker
            .registry
            .select_core_events(stalled_id, window, STRUCTURE_NOTIFY_MASK)
            .unwrap();
        broker
            .registry
            .select_core_events(healthy_id, window, STRUCTURE_NOTIFY_MASK)
            .unwrap();
        broker.registry.route_protocol(stalled_id, filler()).unwrap();

        let destroy = XClientEvent::DestroyNotify {
            sequence: 0,
            event: window,
            window,
        };
        let mut output = XDispatchResult {
            response: None,
            outputs: vec![crate::XClientOutput::Event(destroy)],
            metadata_candidates: Vec::new(),
        };
        route_core_lifecycle_events(&broker.registry, sender, &mut output)
            .expect("a stalled watcher is not the sender's failure");
        assert!(
            output.outputs.is_empty(),
            "the sender did not select on the window: {:?}",
            output.outputs
        );
        assert_eq!(
            healthy
                .channels
                .protocol
                .recv_timeout(Duration::from_secs(1))
                .expect("the healthy watcher is told"),
            destroy
        );
        assert_ended(&mut stalled, "lifecycle");
        assert_continues(&broker, &mut healthy, 4);
    }
}

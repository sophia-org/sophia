use super::*;

const BOUND: Duration = Duration::from_secs(2);

fn finished(thread: &std::thread::JoinHandle<Result<(), X11SetupSocketError>>) -> bool {
    let deadline = Instant::now() + BOUND;
    while !thread.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    thread.is_finished()
}

#[test]
fn empty_output_drain_stops_without_socket_activity() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let output = X11ClientOutput::shared(socket, 1);
    let mut drain = spawn_x11_output_drain(output, 1).unwrap();
    // Let the empty drain enter its untimed wait. The bound below also covers
    // a stop before that wait; neither interleaving may require socket data.
    std::thread::sleep(Duration::from_millis(20));
    drain.stop();
    let stopped = finished(drain.thread.as_ref().unwrap());
    drain.wake.notify(); // rescue a missing-stop-notify mutant before join
    drain.thread.take().unwrap().join().unwrap().unwrap();
    assert!(
        stopped,
        "an empty drain must observe stop without a timeout"
    );
}

#[test]
fn blocked_output_drain_stops_before_its_six_second_silence_deadline() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let output = X11ClientOutput::shared(socket, 2);
    output
        .lock()
        .unwrap()
        .admit(vec![0; 2 << 20], Vec::new())
        .unwrap();
    assert!(output.lock().unwrap().outstanding > 0);
    let mut drain = spawn_x11_output_drain(output, 2).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    drain.stop();
    let stopped = finished(drain.thread.as_ref().unwrap());
    drain.wake.notify();
    drain.thread.take().unwrap().join().unwrap().unwrap();
    assert!(stopped, "stop must interrupt socket-writability polling");
}

#[test]
fn protocol_writer_stops_while_the_route_producer_is_still_alive() {
    let client = XServerFrontendClientId::from_raw(41);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    let (socket, _peer) = UnixStream::pair().unwrap();
    let writer = spawn_x11_protocol_event_writer(
        X11ClientOutput::shared(socket, client.raw()),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(0)),
        client,
        channels.protocol,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    writer.stop.store(true, Ordering::Release);
    writer.wake.as_ref().unwrap().notify();
    assert!(finished(&writer.thread));
    writer.thread.join().unwrap().unwrap();
}

#[test]
fn idle_frontend_accepts_a_connection_and_observes_notified_shutdown() {
    let path = private_service_socket("notified-public");
    let slot = sophia_wake::WakeSlot::default();
    let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(145))
        .unwrap()
        .with_service_wake(slot.clone());
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (transactions, _receiver) = sync_channel(8);
    let (commands, inbox) = sync_channel(8);
    let commands = sophia_wake::SignalSender::new(commands, slot.clone());
    let worker = std::thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(config, transactions, broker, inbox)
    });
    assert!(waited_for(|| slot.attached()));
    let mut client = connect_private_client(&path);
    handshake(&mut client);
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    assert!(
        finished(&worker),
        "command wake must end an idle frontend wait"
    );
    worker.join().unwrap().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn idle_frontend_observes_last_command_producer_disconnecting() {
    let path = private_service_socket("notified-disconnect");
    let slot = sophia_wake::WakeSlot::default();
    let config = XServerFrontendConfig::new(&path, NamespaceId::from_raw(146))
        .unwrap()
        .with_service_wake(slot.clone());
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (transactions, _receiver) = sync_channel(8);
    let (commands, inbox) = sync_channel(8);
    let commands = sophia_wake::SignalSender::new(commands, slot.clone());
    let worker = std::thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(config, transactions, broker, inbox)
    });
    assert!(waited_for(|| slot.attached()));
    drop(commands);
    assert!(finished(&worker));
    worker.join().unwrap().unwrap();
    let _ = std::fs::remove_file(path);
}

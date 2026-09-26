#![cfg(test)]

use super::*;

#[test]
fn shutdown_disconnects_a_full_event_queue_before_joining() {
    let (commands, _receiver) = sync_channel(1);
    let (events, receiver) = sync_channel(1);
    events
        .send(PolicyTransportEvent::ReadyForCycle { capabilities: 0 })
        .unwrap();
    let producer = std::thread::spawn(move || {
        // This send blocks until Drop disconnects the event receiver.
        assert!(
            events
                .send(PolicyTransportEvent::ReadyForCycle { capabilities: 0 })
                .is_err()
        );
    });
    let worker = PolicyTransportWorker {
        commands: Some(commands),
        events: receiver,
        thread: Some(producer),
        stop: None,
    };
    let (done, completion) = sync_channel(1);
    std::thread::spawn(move || {
        drop(worker);
        done.send(()).unwrap();
    });
    completion
        .recv_timeout(Duration::from_secs(2))
        .expect("worker shutdown must not wait for an owner to drain events");
}

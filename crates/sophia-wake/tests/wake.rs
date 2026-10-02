use sophia_wake::{SignalSender, Wake, WakeSlot, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(2);

#[test]
fn rendezvous_channels_are_refused_before_a_consumer_can_sleep() {
    let error = channel::bounded::<u8>(0)
        .err()
        .expect("zero capacity refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn publication_between_inspection_and_wait_stays_readable() {
    let wake = Wake::new().unwrap();
    let (raw, receiver) = mpsc::sync_channel(2);
    let slot = WakeSlot::default();
    slot.set(wake.notifier());
    let sender = SignalSender::new(raw, slot);
    wake.clear().unwrap();
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
    sender.send(7).unwrap();
    sender.send(8).unwrap();
    assert!(wake.wait(Some(Instant::now() + BOUND)).unwrap());
    wake.clear().unwrap();
    assert_eq!(receiver.try_iter().collect::<Vec<_>>(), [7, 8]);
    assert!(!wake.wait(Some(Instant::now())).unwrap());
}

#[test]
fn attachment_covers_work_published_before_the_owner_exists() {
    let (raw, receiver) = mpsc::sync_channel(1);
    let slot = WakeSlot::default();
    let sender = SignalSender::new(raw, slot.clone());
    sender.send(9).unwrap();
    let wake = Wake::new().unwrap();
    slot.set(wake.notifier());
    assert!(wake.wait(Some(Instant::now())).unwrap());
    assert_eq!(receiver.try_recv().unwrap(), 9);
}

#[test]
fn sender_disconnect_wakes_a_consumer_with_no_deadline() {
    let (sender, receiver) = channel::bounded::<u8>(1).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let rescue = receiver.notifier().unwrap();
    let (done, finished) = mpsc::channel();
    let stop = cancel.clone();
    let worker = std::thread::spawn(move || {
        done.send(receiver.recv_until_stopped(&stop).unwrap())
            .unwrap();
    });
    drop(sender);
    let result = finished.recv_timeout(BOUND);
    cancel.store(true, Ordering::Release);
    rescue.notify();
    worker.join().unwrap();
    assert_eq!(result.unwrap(), None);
}

#[test]
fn cancellation_interrupts_a_wait_without_dropping_the_producer() {
    let (_sender, receiver) = channel::bounded::<u8>(1).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let notifier = receiver.notifier().unwrap();
    let worker_stop = stop.clone();
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        done.send(receiver.recv_until_stopped(&worker_stop).unwrap())
            .unwrap();
    });
    stop.store(true, Ordering::Release);
    notifier.notify();
    assert_eq!(finished.recv_timeout(BOUND).unwrap(), None);
    worker.join().unwrap();
}

#[test]
fn full_queue_wakes_its_producer_on_capacity_and_disconnect() {
    let wake = Wake::new().unwrap();
    let (sender, receiver) = channel::bounded(1).unwrap();
    receiver.set_capacity_wake(wake.notifier());
    sender.send(1).unwrap();
    assert_eq!(sender.try_send(2), Err(mpsc::TrySendError::Full(2)));
    wake.clear().unwrap();
    assert_eq!(receiver.try_recv().unwrap(), 1);
    assert!(wake.wait(Some(Instant::now())).unwrap());
    sender.try_send(2).unwrap();
    wake.clear().unwrap();
    drop(receiver);
    assert!(wake.wait(Some(Instant::now())).unwrap());
    assert_eq!(sender.try_send(3), Err(mpsc::TrySendError::Disconnected(3)));
}

#[test]
fn signal_sender_drop_publishes_disconnect_before_notifying() {
    let wake = Wake::new().unwrap();
    let slot = WakeSlot::default();
    slot.set(wake.notifier());
    let (raw, receiver) = mpsc::sync_channel::<u8>(1);
    let sender = SignalSender::new(raw, slot);
    wake.clear().unwrap();
    drop(sender);
    assert!(wake.wait(Some(Instant::now())).unwrap());
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn cleared_notification_keeps_the_same_absolute_deadline() {
    let wake = Wake::new().unwrap();
    let deadline = Instant::now() + Duration::from_millis(20);
    wake.notifier().notify();
    assert!(wake.wait(Some(deadline)).unwrap());
    wake.clear().unwrap();
    assert!(!wake.wait(Some(deadline)).unwrap());
    assert!(Instant::now() >= deadline);
    assert!(!wake.wait(Some(deadline)).unwrap());
}

#[test]
fn abandoned_notifications_do_not_keep_the_descriptor_alive() {
    let wake = Wake::new().unwrap();
    let notifier = wake.notifier();
    drop(wake);
    notifier.notify();
}

//! The acquisition worker sleeps without a timeout. These drive its production
//! loop and wait through a scripted source: a wake stands in for libinput's
//! descriptor, and scripted batches stand in for libinput's own queue, which
//! holds events without keeping that descriptor readable.

use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sophia_protocol::InputEventKind;
use sophia_wake::{Notifier, Wake, WakeSlot};

use super::{
    AcquisitionSource, NativeInputAcquisitionSaturation, QueuedInputEvent,
    ThreadedNativeLibinputEventPoller, acquisition_capacity, run_input_worker,
};
use crate::{
    DeviceId, InputEventPacket, NativeLibinputPolicyReport, NonBlockingInputPoller, SeatId,
};

/// Bounds every wait on the worker, so a lost wake fails instead of hanging.
const BOUND: Duration = Duration::from_secs(5);

fn key(serial: u64) -> InputEventPacket {
    InputEventPacket {
        serial,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: serial,
        kind: InputEventKind::Key {
            keycode: 30,
            pressed: true,
        },
        global_position: None,
        target_surface: None,
        local_position: None,
    }
}

fn keys(serials: std::ops::RangeInclusive<u64>) -> Vec<InputEventPacket> {
    serials.map(key).collect()
}

fn rung(wake: &Wake) -> bool {
    wake.wait(Some(Instant::now())).unwrap()
}

type Batches = Arc<Mutex<VecDeque<Vec<InputEventPacket>>>>;

struct ScriptedSource {
    descriptor: Wake,
    batches: Batches,
    limit: usize,
    reads: Arc<AtomicUsize>,
    /// Runs when the worker asks for its descriptor: after its last stop
    /// check and immediately before it blocks.
    before_wait: Box<dyn Fn() + Send>,
}

impl AcquisitionSource for ScriptedSource {
    fn readiness(&self) -> BorrowedFd<'_> {
        (self.before_wait)();
        self.descriptor.as_fd()
    }

    fn read_batch(&mut self) -> io::Result<Vec<InputEventPacket>> {
        self.reads.fetch_add(1, Ordering::AcqRel);
        // Dispatch drains libinput's descriptor whatever it leaves queued.
        self.descriptor.clear()?;
        let batch = self.batches.lock().unwrap().pop_front().unwrap_or_default();
        assert!(batch.len() <= self.limit, "a read never exceeds its limit");
        Ok(batch)
    }

    fn batch_limit(&self) -> usize {
        self.limit
    }
}

/// The worker's shared state, made before its source so a source's hook can
/// reach the stop signal.
struct Signals {
    stop: Arc<AtomicBool>,
    stop_wake: Wake,
    owner: Wake,
    owner_slot: WakeSlot,
}

impl Signals {
    fn new() -> Self {
        let owner = Wake::new().unwrap();
        let owner_slot = WakeSlot::default();
        owner_slot.set(owner.notifier());
        // Attaching rings once; start each test from a quiet owner.
        owner.clear().unwrap();
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            stop_wake: Wake::new().unwrap(),
            owner,
            owner_slot,
        }
    }

    fn stop_notifier(&self) -> Notifier {
        self.stop_wake.notifier()
    }

    fn spawn(self, source: ScriptedSource, queue_capacity: usize) -> Running {
        let (sender, events) = sync_channel(queue_capacity);
        let (done_tx, done) = mpsc::channel();
        let max_gap = Arc::new(AtomicUsize::new(0));
        let stop_notifier = self.stop_notifier();
        let Self {
            stop,
            stop_wake,
            owner,
            owner_slot,
        } = self;
        let worker_stop = Arc::clone(&stop);
        let worker_max_gap = Arc::clone(&max_gap);
        let worker = thread::spawn(move || {
            let mut source = source;
            let result = run_input_worker(
                &mut source,
                sender,
                &worker_stop,
                &stop_wake,
                &owner_slot,
                &AtomicUsize::new(0),
                &AtomicUsize::new(0),
                &worker_max_gap,
                &Mutex::new(NativeInputAcquisitionSaturation::default()),
                acquisition_capacity(queue_capacity),
            );
            let _ = done_tx.send(result);
        });
        Running {
            stop,
            stop_notifier,
            owner,
            events,
            max_gap,
            done,
            worker,
        }
    }
}

struct Running {
    stop: Arc<AtomicBool>,
    stop_notifier: Notifier,
    owner: Wake,
    events: Receiver<QueuedInputEvent>,
    max_gap: Arc<AtomicUsize>,
    done: Receiver<Result<(), String>>,
    worker: JoinHandle<()>,
}

impl Running {
    fn next_serial(&self) -> u64 {
        self.events
            .recv_timeout(BOUND)
            .expect("the worker delivered without a timed wakeup")
            .packet
            .serial
    }

    /// Ends the worker the way the poller's drop does, and joins it.
    fn stop(self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        self.stop_notifier.notify();
        self.finish()
    }

    fn finish(self) -> Result<(), String> {
        let result = self
            .done
            .recv_timeout(BOUND)
            .expect("the worker left its untimed wait");
        self.worker.join().unwrap();
        result
    }
}

fn source(
    batches: Vec<Vec<InputEventPacket>>,
    limit: usize,
    before_wait: impl Fn() + Send + 'static,
) -> (ScriptedSource, Notifier, Batches, Arc<AtomicUsize>) {
    let descriptor = Wake::new().unwrap();
    let notifier = descriptor.notifier();
    let batches: Batches = Arc::new(Mutex::new(batches.into()));
    let reads = Arc::new(AtomicUsize::new(0));
    (
        ScriptedSource {
            descriptor,
            batches: Arc::clone(&batches),
            limit,
            reads: Arc::clone(&reads),
            before_wait: Box::new(before_wait),
        },
        notifier,
        batches,
        reads,
    )
}

#[test]
fn full_batches_are_read_again_before_the_worker_sleeps() {
    let signals = Signals::new();
    // libinput's descriptor is never readable: everything is already queued
    // inside libinput, where only reading again can find it.
    let (source, _descriptor, _batches, reads) =
        source(vec![keys(1..=4), keys(5..=8), keys(9..=10)], 4, || {});
    let running = signals.spawn(source, 16);

    for serial in 1..=10 {
        assert_eq!(running.next_serial(), serial);
    }
    assert!(running.stop().is_ok());
    // The short batch ran the queue dry, so the worker slept until stopped.
    assert_eq!(reads.load(Ordering::Acquire), 3);
}

#[test]
fn a_stop_rung_between_the_flag_check_and_the_wait_ends_the_worker() {
    let signals = Signals::new();
    let stop = Arc::clone(&signals.stop);
    let stop_notifier = signals.stop_notifier();
    // Teardown lands after the loop has seen the flag clear and before it
    // blocks: exactly the window a timeout used to paper over.
    let (source, _descriptor, _batches, reads) = source(Vec::new(), 4, move || {
        stop.store(true, Ordering::Release);
        stop_notifier.notify();
    });
    let running = signals.spawn(source, 16);

    assert!(running.finish().is_ok());
    assert_eq!(reads.load(Ordering::Acquire), 1);
}

#[test]
fn descriptor_readiness_wakes_an_idle_worker_and_rings_the_owner() {
    let signals = Signals::new();
    let (waiting_tx, waiting) = mpsc::channel();
    let (source, descriptor, batches, _reads) = source(Vec::new(), 4, move || {
        let _ = waiting_tx.send(());
    });
    let running = signals.spawn(source, 16);

    waiting.recv_timeout(BOUND).expect("the worker went idle");
    // An empty read publishes nothing and rings nobody.
    assert!(!rung(&running.owner));
    let idle = Duration::from_millis(20);
    thread::sleep(idle);
    batches.lock().unwrap().push_back(vec![key(1)]);
    descriptor.notify();

    assert_eq!(running.next_serial(), 1);
    assert!(
        running.owner.wait(Some(Instant::now() + BOUND)).unwrap(),
        "a published batch rings the owner"
    );
    // The gap spans the idle sleep: it is not a dispatch latency, and the
    // worker does not pretend it is.
    let gap = running.max_gap.load(Ordering::Acquire);
    assert!(
        gap >= idle.as_millis() as usize,
        "gap {gap} hides idle time"
    );
    assert!(running.stop().is_ok());
}

#[test]
fn a_failed_read_ends_the_worker_with_its_error() {
    struct FailingSource(Wake);
    impl AcquisitionSource for FailingSource {
        fn readiness(&self) -> BorrowedFd<'_> {
            self.0.as_fd()
        }
        fn read_batch(&mut self) -> io::Result<Vec<InputEventPacket>> {
            Err(io::Error::other("reduced native libinput read failed"))
        }
        fn batch_limit(&self) -> usize {
            4
        }
    }
    let signals = Signals::new();
    let (sender, _events) = sync_channel(4);
    let result = run_input_worker(
        &mut FailingSource(Wake::new().unwrap()),
        sender,
        &signals.stop,
        &signals.stop_wake,
        &signals.owner_slot,
        &AtomicUsize::new(0),
        &AtomicUsize::new(0),
        &AtomicUsize::new(0),
        &Mutex::new(NativeInputAcquisitionSaturation::default()),
        acquisition_capacity(4),
    );
    assert_eq!(
        result,
        Err("reduced native libinput read failed".to_owned())
    );
}

/// A poller with no worker behind it: the consumer half alone.
fn consumer(
    queued: Vec<InputEventPacket>,
    max_read_per_poll: usize,
) -> (
    ThreadedNativeLibinputEventPoller,
    Wake,
    mpsc::SyncSender<Result<(), String>>,
) {
    let (sender, receiver) = sync_channel(queued.len().max(1));
    let depth = queued.len();
    for packet in queued {
        sender
            .try_send(QueuedInputEvent {
                packet,
                queued_at: Instant::now(),
            })
            .unwrap();
    }
    // The worker's half of the health channel stays open: a live worker.
    let (health_tx, health) = sync_channel(1);
    let stop_wake = Wake::new().unwrap();
    let poller = ThreadedNativeLibinputEventPoller {
        receiver,
        health,
        policy: Arc::new(Mutex::new(NativeLibinputPolicyReport::default())),
        stop: Arc::new(AtomicBool::new(false)),
        stop_wake: stop_wake.notifier(),
        owner_wake: WakeSlot::default(),
        queue_depth: Arc::new(AtomicUsize::new(depth)),
        max_queue_depth: Arc::new(AtomicUsize::new(depth)),
        max_dispatch_gap_msec: Arc::new(AtomicUsize::new(0)),
        max_queue_dwell_msec: 0,
        event_timings: VecDeque::new(),
        saturation: Arc::new(Mutex::new(NativeInputAcquisitionSaturation::default())),
        inventory: Arc::new(Mutex::new(Vec::new())),
        max_read_per_poll,
        worker: None,
    };
    drop(sender);
    (poller, stop_wake, health_tx)
}

#[test]
fn a_consumer_batch_that_leaves_events_queued_rings_the_owner_again() {
    let (mut poller, _stop_wake, _health) = consumer(keys(1..=3), 2);
    let owner = Wake::new().unwrap();

    poller.set_owner_wake(owner.notifier());
    assert!(rung(&owner), "attaching rings for what was already queued");
    owner.clear().unwrap();

    let first = poller.poll_ready().unwrap();
    assert_eq!(first.len(), 2);
    assert!(rung(&owner), "one event is still queued behind the limit");
    owner.clear().unwrap();

    let second = poller.poll_ready().unwrap();
    assert_eq!(second.len(), 1);
    assert!(!rung(&owner), "a drained queue needs no further wake");
}

#[test]
fn dropping_the_poller_rings_its_stop_wake() {
    let (poller, stop_wake, _health) = consumer(Vec::new(), 2);
    let stop = Arc::clone(&poller.stop);
    assert!(!rung(&stop_wake));

    drop(poller);

    assert!(stop.load(Ordering::Acquire));
    assert!(
        rung(&stop_wake),
        "a worker in an untimed wait sees the ring"
    );
}
#[test]
fn closed_input_descriptor_fails_instead_of_spinning() {
    let (source, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let stop = Wake::new().unwrap();
    drop(peer);
    assert_eq!(
        super::wait_for_input(source.as_fd(), stop.as_fd()).unwrap_err(),
        "native input descriptor closed while waiting"
    );
    stop.notifier().notify();
    assert!(super::wait_for_input(source.as_fd(), stop.as_fd()).unwrap());
}

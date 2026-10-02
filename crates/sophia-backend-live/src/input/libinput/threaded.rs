use std::collections::VecDeque;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags};
use sophia_protocol::{
    BoundedCapacity, CapacityAcquisitionLedger, CapacityBatchAttempt, CapacityBatchOutcome,
    CapacityEscalation, CapacityResourceId, CapacitySaturationDisposition,
    CapacitySaturationReport, CapacityWait, drive_capacity_batch,
};
use sophia_wake::{Notifier, Wake, WakeSlot};

use crate::prelude::*;

use super::{
    NativeLibinputDeviceMap, NativeLibinputEventPoller, NativeLibinputEventReader,
    NativeLibinputOpenError, NativeLibinputPointerPolicy, NativeLibinputPolicyReport,
    open_native_libinput_path_poller_with_pointer_policy,
    open_native_libinput_udev_poller_with_pointer_policy,
};

const NATIVE_INPUT_ACQUISITION: CapacityResourceId =
    CapacityResourceId("backend_live.input.acquisition");

/// How long the acquisition worker declines to read before abandoning a batch.
///
/// Not reading is the correct backpressure here: evdev buffers upstream while
/// the worker waits, so a consumer that is merely late costs latency rather
/// than events. The ceiling is what keeps this a bounded deferral instead of
/// the unbounded retry it replaces.
const NATIVE_INPUT_ACQUISITION_DEFERRAL_MSEC: u32 = 50;
const NATIVE_INPUT_ACQUISITION_RETRY_MSEC: u32 = 1;

/// What acquisition saturation cost, shared with the session frontend.
///
/// The report is a replaceable slot rather than a queue: every report carries
/// the cumulative discard total, so the newest supersedes its predecessors and
/// reporting saturation cannot itself become an unbounded resource.
///
/// A non-zero discard total means raw device packets were dropped, which can
/// include key releases. The frontend treats that as a reason to flush the keys
/// it believes are held; acquisition cannot do that itself because at this
/// layer a packet has no routed meaning yet.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeInputAcquisitionSaturation {
    pub ledger: CapacityAcquisitionLedger,
    pub latest: Option<CapacitySaturationReport>,
}

const fn acquisition_capacity(capacity: usize) -> BoundedCapacity {
    BoundedCapacity::new(
        NATIVE_INPUT_ACQUISITION,
        capacity,
        CapacitySaturationDisposition::BoundedDeferral {
            deadline_msec: NATIVE_INPUT_ACQUISITION_DEFERRAL_MSEC,
            retry_interval_msec: NATIVE_INPUT_ACQUISITION_RETRY_MSEC,
            escalation: CapacityEscalation::EndpointEpochClosed,
        },
    )
}

/// The worker's own waiting strategy. Pausing this thread is the backpressure:
/// it stops draining libinput, which is what lets the kernel hold the events.
struct AcquisitionWait<'a> {
    started: Instant,
    stop: &'a AtomicBool,
}

impl CapacityWait for AcquisitionWait<'_> {
    fn elapsed_msec(&self) -> u32 {
        u32::try_from(self.started.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    fn pause(&mut self, interval_msec: u32) {
        std::thread::sleep(Duration::from_millis(u64::from(interval_msec.max(1))));
    }

    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

struct QueuedInputEvent {
    packet: InputEventPacket,
    queued_at: Instant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThreadedNativeInputStats {
    /// The longest interval between two worker reads. The worker sleeps
    /// without a timeout while the seat is idle, so this includes intentional
    /// idle time and is not a dispatch latency: a quiet seat reports a large
    /// gap by design.
    pub max_dispatch_gap_msec: usize,
    pub max_queue_depth: usize,
    pub max_queue_dwell_msec: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadedNativeInputEventTiming {
    pub serial: u64,
    pub event_time_msec: u64,
    pub queue_dwell_msec: usize,
}

pub struct ThreadedNativeLibinputEventPoller {
    receiver: Receiver<QueuedInputEvent>,
    health: Receiver<Result<(), String>>,
    policy: Arc<std::sync::Mutex<NativeLibinputPolicyReport>>,
    stop: Arc<AtomicBool>,
    /// Rings the worker out of its untimed wait; the flag alone is never seen.
    stop_wake: Notifier,
    /// The consumer's wake, rung by the worker whenever it publishes.
    owner_wake: WakeSlot,
    queue_depth: Arc<AtomicUsize>,
    max_queue_depth: Arc<AtomicUsize>,
    max_dispatch_gap_msec: Arc<AtomicUsize>,
    max_queue_dwell_msec: usize,
    event_timings: VecDeque<ThreadedNativeInputEventTiming>,
    saturation: Arc<Mutex<NativeInputAcquisitionSaturation>>,
    inventory: Arc<Mutex<Vec<NativeLibinputDeviceRecord>>>,
    max_read_per_poll: usize,
    worker: Option<JoinHandle<()>>,
}

impl ThreadedNativeLibinputEventPoller {
    /// Installs the consumer's wake. The worker rings it after each batch it
    /// publishes (events, the inventory changes they carry, saturation), when
    /// a deferral starts waiting on a full queue, and when it stops or fails.
    /// Installation itself rings once, so anything published before the
    /// consumer attached is not left for the next device event.
    pub fn set_owner_wake(&self, notifier: Notifier) {
        self.owner_wake.set(notifier);
    }

    pub fn stats(&self) -> ThreadedNativeInputStats {
        ThreadedNativeInputStats {
            max_dispatch_gap_msec: self.max_dispatch_gap_msec.load(Ordering::Acquire),
            max_queue_depth: self.max_queue_depth.load(Ordering::Acquire),
            max_queue_dwell_msec: self.max_queue_dwell_msec,
        }
    }

    pub fn policy_report(&self) -> NativeLibinputPolicyReport {
        self.policy
            .lock()
            .map_or_else(|_| NativeLibinputPolicyReport::default(), |policy| *policy)
    }

    /// The devices the worker's seat holds right now, as opaque records.
    /// Published by the reader on every change; a poisoned lock reads empty.
    pub fn device_inventory(&self) -> Vec<NativeLibinputDeviceRecord> {
        self.inventory
            .lock()
            .map_or_else(|_| Vec::new(), |inventory| inventory.clone())
    }

    pub fn drain_event_timings(&mut self) -> Vec<ThreadedNativeInputEventTiming> {
        self.event_timings.drain(..).collect()
    }

    /// Takes the pending acquisition-saturation report, leaving the cumulative
    /// ledger in place. Returns `None` when nothing was discarded since the
    /// last call, which is the ordinary case.
    pub fn take_acquisition_saturation(&mut self) -> Option<CapacitySaturationReport> {
        self.saturation
            .lock()
            .ok()
            .and_then(|mut state| state.latest.take())
    }

    /// Cumulative acquisition accounting. `discarded_total` is zero unless a
    /// deferral ran out of time.
    pub fn acquisition_ledger(&self) -> CapacityAcquisitionLedger {
        self.saturation.lock().map_or_else(
            |_| CapacityAcquisitionLedger::default(),
            |state| state.ledger,
        )
    }

    fn worker_error(&self) -> io::Result<()> {
        match self.health.try_recv() {
            Ok(Ok(())) | Err(TryRecvError::Empty) => Ok(()),
            Ok(Err(message)) => Err(io::Error::other(message)),
            Err(TryRecvError::Disconnected) if self.stop.load(Ordering::Acquire) => Ok(()),
            Err(TryRecvError::Disconnected) => Err(io::Error::other(
                "native input acquisition worker disconnected",
            )),
        }
    }
}

impl NonBlockingInputPoller for ThreadedNativeLibinputEventPoller {
    fn poll_ready(&mut self) -> io::Result<Vec<InputEventPacket>> {
        self.worker_error()?;
        let mut packets = Vec::new();
        while packets.len() < self.max_read_per_poll {
            let queued = match self.receiver.try_recv() {
                Ok(queued) => queued,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.worker_error()?;
                    break;
                }
            };
            self.queue_depth.fetch_sub(1, Ordering::AcqRel);
            let queue_dwell_msec =
                usize::try_from(queued.queued_at.elapsed().as_millis()).unwrap_or(usize::MAX);
            self.max_queue_dwell_msec = self.max_queue_dwell_msec.max(queue_dwell_msec);
            self.event_timings
                .push_back(ThreadedNativeInputEventTiming {
                    serial: queued.packet.serial,
                    event_time_msec: queued.packet.time_msec,
                    queue_dwell_msec,
                });
            packets.push(queued.packet);
        }
        // The worker rings once per batch, and a ring is coalesced. What this
        // call leaves queued would otherwise wait for the next device event.
        if packets.len() >= self.max_read_per_poll && self.queue_depth.load(Ordering::Acquire) > 0 {
            self.owner_wake.notify();
        }
        self.worker_error()?;
        Ok(packets)
    }
}

impl Drop for ThreadedNativeLibinputEventPoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.stop_wake.notify();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn open_threaded_native_libinput_path_poller(
    paths: &[std::path::PathBuf],
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    open_threaded_native_libinput_path_poller_with_pointer_policy(
        paths,
        devices,
        max_read_per_poll,
        queue_capacity,
        NativeLibinputPointerPolicy::default(),
    )
}

pub fn open_threaded_native_libinput_path_poller_with_pointer_policy(
    paths: &[std::path::PathBuf],
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    if paths.is_empty() {
        return Err(NativeLibinputOpenError::NoDevices);
    }
    if paths.len() > 16 {
        return Err(NativeLibinputOpenError::TooManyDevices);
    }
    open_threaded_native_libinput_poller(
        NativeLibinputSource::Paths(paths.to_vec()),
        devices,
        max_read_per_poll,
        queue_capacity,
        pointer_policy,
    )
}

pub fn open_threaded_native_libinput_udev_poller(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    open_threaded_native_libinput_udev_poller_with_pointer_policy(
        seat_name,
        devices,
        max_read_per_poll,
        queue_capacity,
        NativeLibinputPointerPolicy::default(),
    )
}

pub fn open_threaded_native_libinput_udev_poller_with_pointer_policy(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    open_threaded_native_libinput_poller(
        NativeLibinputSource::Udev(seat_name.to_owned()),
        devices,
        max_read_per_poll,
        queue_capacity,
        pointer_policy,
    )
}

#[cfg(feature = "seat-control")]
pub fn open_threaded_native_libinput_udev_poller_with_seat(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
    opener: crate::LiveSeatDeviceOpener,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    open_threaded_native_libinput_udev_poller_with_seat_and_pointer_policy(
        seat_name,
        devices,
        max_read_per_poll,
        queue_capacity,
        opener,
        NativeLibinputPointerPolicy::default(),
    )
}

#[cfg(feature = "seat-control")]
pub fn open_threaded_native_libinput_udev_poller_with_seat_and_pointer_policy(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
    opener: crate::LiveSeatDeviceOpener,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    open_threaded_native_libinput_poller(
        NativeLibinputSource::SeatUdev(seat_name.to_owned(), opener),
        devices,
        max_read_per_poll,
        queue_capacity,
        pointer_policy,
    )
}

enum NativeLibinputSource {
    Paths(Vec<std::path::PathBuf>),
    Udev(String),
    #[cfg(feature = "seat-control")]
    SeatUdev(String, crate::LiveSeatDeviceOpener),
}

fn open_threaded_native_libinput_poller(
    source: NativeLibinputSource,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    queue_capacity: usize,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<ThreadedNativeLibinputEventPoller, NativeLibinputOpenError> {
    let max_read_per_poll = max_read_per_poll.clamp(1, 256);
    let queue_capacity = queue_capacity.clamp(1, 4_096);
    // Without a stop wake the worker could never be joined from its untimed
    // wait, so failing to make one is failing to open.
    let stop_wake = Wake::new().map_err(|_| NativeLibinputOpenError::DeviceUnavailable)?;
    let stop_notifier = stop_wake.notifier();
    let owner_wake = WakeSlot::default();
    let worker_owner_wake = owner_wake.clone();
    let (sender, receiver) = sync_channel(queue_capacity);
    let (startup_sender, startup_receiver) = sync_channel(1);
    let (health_sender, health) = sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let queue_depth = Arc::new(AtomicUsize::new(0));
    let max_queue_depth = Arc::new(AtomicUsize::new(0));
    let max_dispatch_gap_msec = Arc::new(AtomicUsize::new(0));
    let worker_stop = Arc::clone(&stop);
    let worker_depth = Arc::clone(&queue_depth);
    let worker_max_depth = Arc::clone(&max_queue_depth);
    let worker_max_gap = Arc::clone(&max_dispatch_gap_msec);
    let saturation = Arc::new(Mutex::new(NativeInputAcquisitionSaturation::default()));
    let worker_saturation = Arc::clone(&saturation);
    let capacity = acquisition_capacity(queue_capacity);
    let worker = std::thread::spawn(move || {
        let opened = match source {
            NativeLibinputSource::Paths(paths) => {
                open_native_libinput_path_poller_with_pointer_policy(
                    &paths,
                    devices,
                    max_read_per_poll,
                    pointer_policy,
                )
            }
            NativeLibinputSource::Udev(seat) => {
                open_native_libinput_udev_poller_with_pointer_policy(
                    &seat,
                    devices,
                    max_read_per_poll,
                    pointer_policy,
                )
            }
            #[cfg(feature = "seat-control")]
            NativeLibinputSource::SeatUdev(seat, opener) => {
                super::open_native_libinput_udev_poller_with_seat_and_pointer_policy(
                    &seat,
                    devices,
                    max_read_per_poll,
                    opener,
                    pointer_policy,
                )
            }
        };
        let mut poller = match opened {
            Ok(poller) => {
                let policy = poller.reader().policy_report();
                let handle = poller.reader().policy_handle();
                let inventory = poller.reader().inventory_handle();
                let _ = startup_sender.send(Ok((policy, handle, inventory)));
                poller
            }
            Err(error) => {
                let _ = startup_sender.send(Err(error));
                return;
            }
        };
        let result = run_input_worker(
            &mut poller,
            sender,
            &worker_stop,
            &stop_wake,
            &worker_owner_wake,
            &worker_depth,
            &worker_max_depth,
            &worker_max_gap,
            &worker_saturation,
            capacity,
        );
        let _ = health_sender.try_send(result);
        // Publish the outcome first: a consumer woken here must find it.
        worker_owner_wake.notify();
    });
    match startup_receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok((_policy, policy, inventory))) => Ok(ThreadedNativeLibinputEventPoller {
            receiver,
            health,
            policy,
            stop,
            stop_wake: stop_notifier,
            owner_wake,
            queue_depth,
            max_queue_depth,
            max_dispatch_gap_msec,
            max_queue_dwell_msec: 0,
            event_timings: VecDeque::new(),
            saturation,
            inventory,
            max_read_per_poll,
            worker: Some(worker),
        }),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            stop.store(true, Ordering::Release);
            stop_notifier.notify();
            let _ = worker.join();
            Err(NativeLibinputOpenError::DeviceUnavailable)
        }
    }
}

/// The worker's view of libinput: a descriptor that turns readable when the
/// kernel has something for it, and a bounded read of what libinput holds.
/// Tests drive the production loop through this with descriptors and batches
/// they control.
trait AcquisitionSource {
    fn readiness(&self) -> BorrowedFd<'_>;
    /// Reads at most [`Self::batch_limit`] events.
    fn read_batch(&mut self) -> io::Result<Vec<InputEventPacket>>;
    fn batch_limit(&self) -> usize;
}

impl AcquisitionSource for NativeLibinputEventPoller<NativeLibinputEventReader> {
    fn readiness(&self) -> BorrowedFd<'_> {
        self.reader().libinput_mut_ref().as_fd()
    }

    fn read_batch(&mut self) -> io::Result<Vec<InputEventPacket>> {
        self.poll_ready()
    }

    fn batch_limit(&self) -> usize {
        self.max_read_per_poll()
    }
}

/// Blocks until libinput or the stop wake is readable and reports whether
/// the stop wake was. There is no timeout: an idle seat costs no wakeups, so
/// every reason to run again has to arrive on one of these descriptors.
fn wait_for_input(source: BorrowedFd<'_>, stop: BorrowedFd<'_>) -> Result<bool, String> {
    let mut fds = [
        PollFd::new(&source, PollFlags::IN),
        PollFd::new(&stop, PollFlags::IN),
    ];
    sophia_wake::wait(&mut fds, None).map_err(|error| error.to_string())?;
    let stopped = fds[1]
        .revents()
        .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL);
    if !stopped
        && fds[0]
            .revents()
            .intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
    {
        return Err("native input descriptor closed while waiting".to_owned());
    }
    Ok(stopped)
}

fn run_input_worker(
    source: &mut impl AcquisitionSource,
    sender: SyncSender<QueuedInputEvent>,
    stop: &AtomicBool,
    stop_wake: &Wake,
    owner_wake: &WakeSlot,
    queue_depth: &AtomicUsize,
    max_queue_depth: &AtomicUsize,
    max_dispatch_gap_msec: &AtomicUsize,
    saturation: &Mutex<NativeInputAcquisitionSaturation>,
    capacity: BoundedCapacity,
) -> Result<(), String> {
    let mut last_dispatch = Instant::now();
    // The first read never waits: announcements retained from the open, and
    // anything libinput queued meanwhile, have no readiness of their own.
    let mut drained = false;
    while !stop.load(Ordering::Acquire) {
        if drained && wait_for_input(source.readiness(), stop_wake.as_fd())? {
            // Clear before the flag is checked again, so a later ring stays
            // readable. Only teardown rings this wake.
            stop_wake.clear().map_err(|error| error.to_string())?;
            continue;
        }
        let gap = usize::try_from(last_dispatch.elapsed().as_millis()).unwrap_or(usize::MAX);
        observe_max(max_dispatch_gap_msec, gap);
        last_dispatch = Instant::now();
        let events = source.read_batch().map_err(|error| error.to_string())?;
        // A full batch can leave events in libinput's own queue, which does
        // not keep its descriptor readable. Read again before sleeping; a
        // short batch means the read ran libinput's queue dry.
        drained = events.len() < source.batch_limit();
        let arrived = events.len();
        record_arrivals(saturation, arrived);
        let outcome = drive_capacity_batch(
            &capacity,
            events,
            || AcquisitionWait {
                started: Instant::now(),
                stop,
            },
            |packet| {
                // Claim the slot before the event becomes visible to the
                // receiver, and release it if the offer failed. A claim made
                // afterwards could be subtracted before it was ever added.
                let depth = queue_depth.fetch_add(1, Ordering::AcqRel).saturating_add(1);
                observe_max(max_queue_depth, depth);
                match sender.try_send(QueuedInputEvent {
                    packet,
                    queued_at: Instant::now(),
                }) {
                    Ok(()) => CapacityBatchAttempt::Accepted,
                    Err(TrySendError::Full(event)) => {
                        queue_depth.fetch_sub(1, Ordering::AcqRel);
                        // The batch has not been rung for yet, and the
                        // deferral only ends early if the consumer drains.
                        owner_wake.notify();
                        CapacityBatchAttempt::Full {
                            record: event.packet,
                            depth: capacity.capacity,
                        }
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        queue_depth.fetch_sub(1, Ordering::AcqRel);
                        CapacityBatchAttempt::RecipientGone
                    }
                }
            },
        );
        record_admitted(saturation, outcome.admitted());
        match outcome {
            CapacityBatchOutcome::Drained { .. } => {}
            // Neither teardown nor a departed poller is a degradation of a
            // running session. What they abandoned stays visible as the
            // ledger's held count without being reported as loss.
            CapacityBatchOutcome::Cancelled { .. } | CapacityBatchOutcome::RecipientGone { .. } => {
                return Ok(());
            }
            // Deferral is spent. This costs the batch rather than the session,
            // and says exactly what it cost: a truncation reporting zero would
            // be the silent drop this disposition exists to forbid.
            CapacityBatchOutcome::Saturated {
                discarded, report, ..
            } => record_discarded(saturation, discarded, report),
        }
        // One ring covers the batch: its events, the inventory changes they
        // carry, and any saturation report.
        if arrived > 0 {
            owner_wake.notify();
        }
    }
    Ok(())
}

fn record_arrivals(saturation: &Mutex<NativeInputAcquisitionSaturation>, count: usize) {
    if let Ok(mut state) = saturation.lock() {
        state.ledger.arrived(count as u64);
    }
}

fn record_admitted(saturation: &Mutex<NativeInputAcquisitionSaturation>, count: usize) {
    if let Ok(mut state) = saturation.lock() {
        state.ledger.admitted(count as u64);
    }
}

fn record_discarded(
    saturation: &Mutex<NativeInputAcquisitionSaturation>,
    count: usize,
    mut report: CapacitySaturationReport,
) {
    debug_assert!(
        count > 0,
        "a discard report must account for at least one event"
    );
    if let Ok(mut state) = saturation.lock() {
        state.ledger.discarded(count as u64);
        // The published total is cumulative, which is what lets one replaceable
        // slot supersede every earlier report without losing volume.
        report.discarded = usize::try_from(state.ledger.discarded_total()).unwrap_or(usize::MAX);
        state.latest = Some(report);
    }
}

fn observe_max(value: &AtomicUsize, candidate: usize) {
    let mut current = value.load(Ordering::Acquire);
    while candidate > current {
        match value.compare_exchange_weak(current, candidate, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/support/libinput_threaded_wake.rs"]
mod wake_tests;

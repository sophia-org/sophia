use super::render_inventory::{
    LiveRenderDeviceIdentitySnapshot, snapshot_admitted_seat_render_inventory,
};
use super::seat_inventory::{discover_admitted_seat_cards, policy::valid_seat};
use std::ffi::OsStr;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};

mod events;
mod scope;
use events::TopologyEventSource;
use scope::SeatTopologyScope;

const DRM_TOPOLOGY_MONITOR_POLL_MSEC: i64 = 50;
const DRM_TOPOLOGY_MONITOR_BATCH_MAX_EVENTS: usize = 256;

fn saturating_increment(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
        Some(value.saturating_add(1))
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveDrmTopologyRescanNotice {
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveDrmTopologyMonitorStats {
    pub observed: u64,
    pub coalesced: u64,
    pub delivered: u64,
}

/// Kernel revocation and processed udev changes share one bounded notice stream.
/// Device admission and reassignment require a running udev service; opening
/// its monitor socket alone does not establish that the service delivers events.
pub struct LiveDrmTopologyMonitor {
    seat: String,
    gpu_admission: crate::LiveGpuAdmission,
    ready: Receiver<()>,
    inventory_ready: Receiver<()>,
    inventory_baseline: Option<(String, Vec<LiveRenderDeviceIdentitySnapshot>)>,
    inventory_dirty: bool,
    inventory_retry_at: Option<Instant>,
    health: Receiver<Result<(), String>>,
    stop: Arc<AtomicBool>,
    latest_sequence: Arc<AtomicU64>,
    observed: Arc<AtomicU64>,
    coalesced: Arc<AtomicU64>,
    delivered: u64,
    worker: Option<JoinHandle<()>>,
}

fn inventory_changed(
    previous: &[LiveRenderDeviceIdentitySnapshot],
    current: &[LiveRenderDeviceIdentitySnapshot],
) -> bool {
    previous != current
}

impl LiveDrmTopologyMonitor {
    pub fn open(seat: &str) -> io::Result<Self> {
        Self::open_with_gpu_admission(seat, crate::LiveGpuAdmission::default())
    }

    pub fn open_with_gpu_admission(
        seat: &str,
        gpu_admission: crate::LiveGpuAdmission,
    ) -> io::Result<Self> {
        if !valid_seat(seat) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid DRM monitor seat",
            ));
        }
        let (notice_sender, ready) = sync_channel(1);
        let (inventory_sender, inventory_ready) = sync_channel(1);
        let (startup_sender, startup_receiver) = sync_channel(1);
        let (health_sender, health) = sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let latest_sequence = Arc::new(AtomicU64::new(0));
        let observed = Arc::new(AtomicU64::new(0));
        let coalesced = Arc::new(AtomicU64::new(0));
        let worker_stop = Arc::clone(&stop);
        let worker_sequence = Arc::clone(&latest_sequence);
        let worker_observed = Arc::clone(&observed);
        let worker_coalesced = Arc::clone(&coalesced);
        let worker_seat = seat.to_owned();
        let worker_admission = gpu_admission.clone();
        let worker = std::thread::spawn(move || {
            let monitors = (|| -> io::Result<_> {
                let kernel = udev::MonitorBuilder::new_kernel()
                    .and_then(|builder| builder.match_subsystem("drm"))
                    .and_then(udev::MonitorBuilder::listen)
                    .map_err(|error| io::Error::other(format!("kernel DRM monitor: {error}")))?;
                let processed = udev::MonitorBuilder::new()
                    .and_then(|builder| builder.match_subsystem("drm"))
                    .and_then(udev::MonitorBuilder::listen)
                    .map_err(|error| {
                        io::Error::other(format!("processed udev DRM monitor: {error}"))
                    })?;
                // Both subscriptions precede discovery. Events racing with
                // this baseline remain queued on the monitor sockets.
                let scope = SeatTopologyScope::new(discover_admitted_seat_cards(
                    &worker_seat,
                    &worker_admission,
                )?);
                Ok((kernel, processed, scope))
            })();
            let (kernel, processed, scope) = match monitors {
                Ok(monitors) => {
                    let _ = startup_sender.send(Ok(()));
                    monitors
                }
                Err(error) => {
                    let _ = startup_sender.send(Err(error.to_string()));
                    return;
                }
            };
            let result = run_drm_topology_monitor(
                kernel,
                processed,
                (&worker_seat, &worker_admission),
                scope,
                notice_sender,
                inventory_sender,
                &worker_stop,
                &worker_sequence,
                &worker_observed,
                &worker_coalesced,
            );
            let _ = health_sender.try_send(result);
        });
        match startup_receiver.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                seat: seat.to_owned(),
                gpu_admission,
                ready,
                inventory_ready,
                inventory_baseline: None,
                inventory_dirty: false,
                inventory_retry_at: None,
                health,
                stop,
                latest_sequence,
                observed,
                coalesced,
                delivered: 0,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(io::Error::other(error))
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "DRM topology monitor startup timed out",
                ))
            }
        }
    }

    /// Establishes the membership baseline after subscriptions are active.
    pub fn initialize_render_inventory(&mut self) -> io::Result<()> {
        let snapshot = snapshot_admitted_seat_render_inventory(&self.seat, &self.gpu_admission)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.inventory_baseline = Some((self.seat.clone(), snapshot));
        Ok(())
    }

    /// Returns a notice only when the settled render-device identities differ
    /// from the baseline. Event bursts are coalesced before comparison.
    pub fn poll_render_inventory_notice(&mut self) -> io::Result<bool> {
        let admission = self.gpu_admission.clone();
        self.poll_render_inventory_with(Instant::now(), |seat| {
            snapshot_admitted_seat_render_inventory(seat, &admission)
                .map_err(|error| io::Error::other(error.to_string()))
        })
    }

    fn poll_render_inventory_with(
        &mut self,
        now: Instant,
        snapshot: impl FnOnce(&str) -> io::Result<Vec<LiveRenderDeviceIdentitySnapshot>>,
    ) -> io::Result<bool> {
        self.worker_error()?;
        if self.inventory_ready.try_recv().is_ok() {
            self.inventory_dirty = true;
        }
        if !self.inventory_dirty || self.inventory_retry_at.is_some_and(|retry| now < retry) {
            return Ok(false);
        }
        let Some((seat, previous)) = self.inventory_baseline.as_ref() else {
            return Ok(false);
        };
        let current = match snapshot(seat) {
            Ok(current) => current,
            Err(error) => {
                self.inventory_retry_at = Some(now + Duration::from_millis(250));
                return Err(error);
            }
        };
        self.inventory_retry_at = None;
        self.inventory_dirty = false;
        if !inventory_changed(previous, &current) {
            return Ok(false);
        }
        self.inventory_baseline = Some((seat.clone(), current));
        Ok(true)
    }

    /// Returns the last successfully compared inventory without reopening or
    /// rescanning any device.
    pub fn render_inventory_snapshot(&self) -> Option<&[LiveRenderDeviceIdentitySnapshot]> {
        self.inventory_baseline
            .as_ref()
            .map(|(_, snapshot)| snapshot.as_slice())
    }

    pub fn poll_notice(&mut self) -> io::Result<Option<LiveDrmTopologyRescanNotice>> {
        self.worker_error()?;
        match self.ready.try_recv() {
            Ok(()) => {
                self.delivered = self.delivered.saturating_add(1);
                Ok(Some(LiveDrmTopologyRescanNotice {
                    sequence: self.latest_sequence.load(Ordering::Acquire),
                }))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.worker_error()?;
                Ok(None)
            }
        }
    }

    pub fn stats(&self) -> LiveDrmTopologyMonitorStats {
        LiveDrmTopologyMonitorStats {
            observed: self.observed.load(Ordering::Acquire),
            coalesced: self.coalesced.load(Ordering::Acquire),
            delivered: self.delivered,
        }
    }

    fn worker_error(&self) -> io::Result<()> {
        match self.health.try_recv() {
            Ok(Ok(())) | Err(TryRecvError::Empty) => Ok(()),
            Ok(Err(error)) => Err(io::Error::other(error)),
            Err(TryRecvError::Disconnected) if self.stop.load(Ordering::Acquire) => Ok(()),
            Err(TryRecvError::Disconnected) => {
                Err(io::Error::other("DRM topology monitor disconnected"))
            }
        }
    }
}

impl Drop for LiveDrmTopologyMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_drm_topology_monitor(
    kernel: udev::MonitorSocket,
    processed: udev::MonitorSocket,
    admission: (&str, &crate::LiveGpuAdmission),
    mut scope: SeatTopologyScope,
    sender: SyncSender<()>,
    inventory_sender: SyncSender<()>,
    stop: &AtomicBool,
    latest_sequence: &AtomicU64,
    observed: &AtomicU64,
    coalesced: &AtomicU64,
) -> Result<(), String> {
    let (seat, gpu_admission) = admission;
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: DRM_TOPOLOGY_MONITOR_POLL_MSEC * 1_000_000,
    };
    while !stop.load(Ordering::Acquire) {
        let mut fds = [
            PollFd::new(&kernel, PollFlags::IN),
            PollFd::new(&processed, PollFlags::IN),
        ];
        poll(&mut fds, Some(&timeout)).map_err(|error| error.to_string())?;
        if fds.iter().any(|fd| {
            fd.revents()
                .intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
        }) {
            return Err("DRM topology monitor socket failed".to_owned());
        }
        for (source, monitor) in [
            (TopologyEventSource::Kernel, &kernel),
            (TopologyEventSource::Processed, &processed),
        ] {
            for event in monitor.iter().take(DRM_TOPOLOGY_MONITOR_BATCH_MAX_EVENTS) {
                let inventory_change = match source {
                    TopologyEventSource::Kernel => event.sysname().to_str().is_some_and(|name| {
                        (name.starts_with("card") || name.starts_with("renderD"))
                            && matches!(
                                event.event_type(),
                                udev::EventType::Remove | udev::EventType::Unbind
                            )
                    }),
                    TopologyEventSource::Processed => {
                        event.sysname().to_str().is_some_and(|name| {
                            (name.starts_with("card") || name.starts_with("renderD"))
                                && matches!(
                                    event.event_type(),
                                    udev::EventType::Add
                                        | udev::EventType::Bind
                                        | udev::EventType::Change
                                )
                        })
                    }
                };
                if inventory_change {
                    let _ = inventory_sender.try_send(());
                }
                if !scope.observe(
                    source,
                    event.event_type(),
                    event.sysname(),
                    event.syspath(),
                    event.property_value("HOTPLUG") == Some(OsStr::new("1")),
                ) {
                    continue;
                }
                // A processed event can carry new seat facts even when its
                // kernel event was already delivered. Sequence numbers are local.
                if !publish_topology_notice(&sender, latest_sequence, observed, coalesced)? {
                    return Ok(());
                }
            }
        }
        // Retry failed membership comparisons even without another event.
        match scope.refresh(Instant::now(), || {
            discover_admitted_seat_cards(seat, gpu_admission)
        }) {
            Ok(true) => {
                let _ = inventory_sender.try_send(());
                if !publish_topology_notice(&sender, latest_sequence, observed, coalesced)? {
                    return Ok(());
                }
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "seat topology inventory comparison deferred"),
        }
    }
    Ok(())
}

fn publish_topology_notice(
    sender: &SyncSender<()>,
    latest_sequence: &AtomicU64,
    observed: &AtomicU64,
    coalesced: &AtomicU64,
) -> Result<bool, String> {
    let mut current = latest_sequence.load(Ordering::Acquire);
    loop {
        let next = current
            .checked_add(1)
            .ok_or("DRM topology notice sequence exhausted")?;
        match latest_sequence.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(observed_current) => current = observed_current,
        }
    }
    saturating_increment(observed);
    match sender.try_send(()) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(())) => {
            saturating_increment(coalesced);
            Ok(true)
        }
        Err(TrySendError::Disconnected(())) => Ok(false),
    }
}

mod tests;

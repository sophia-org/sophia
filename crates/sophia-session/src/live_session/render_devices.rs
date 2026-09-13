use sophia_backend_live::{LiveRenderDevice, LiveRenderDeviceIdentitySnapshot as Identity};
use sophia_x_authority::{
    XServerFrontendDeviceBundle as Bundle, XServerFrontendDeviceBundleError as BundleError,
    XServerFrontendServiceCommand as Command,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    time::{Duration, Instant},
};

mod worker;
pub(super) use worker::initial;

const RETRY: Duration = Duration::from_millis(250);
const ACK_TIMEOUT: Duration = Duration::from_secs(4);
type Acknowledgement = (Receiver<Result<(), BundleError>>, Instant);

struct PreparationRequest {
    ticket: u64,
    bundle_generation: u64,
    seat: String,
    identities: Vec<Identity>,
    preferred_physical: PathBuf,
    pixmap_textures: bool,
    replace_default: bool,
}

struct PreparedBundle {
    bundle: Arc<Bundle>,
    identity: Identity,
}
struct PreparedInventory {
    devices: Vec<LiveRenderDevice>,
    replacement: Result<Option<PreparedBundle>, String>,
}
struct PreparationFlight {
    ticket: u64,
    deadline: Instant,
    deadline_reported: bool,
}

struct PendingInstall {
    ticket: u64,
    prepared: PreparedBundle,
    acknowledgement: Option<Acknowledgement>,
}

/// Device preparation and frontend acknowledgement have separate bounded stages.
/// A changed inventory never redirects an existing connection's device contract.
pub(super) struct LiveRenderDeviceCoordinator {
    seat: String,
    desired: Vec<Identity>,
    ticket: u64,
    next_bundle_generation: u64,
    active_generation: u64,
    active_identity: Identity,
    active_available: bool,
    pixmap_textures: bool,
    dirty: bool,
    retry_at: Instant,
    preparation: Option<PreparationFlight>,
    prepare: SyncSender<PreparationRequest>,
    prepared: Receiver<(u64, Result<PreparedInventory, String>)>,
    install: Option<PendingInstall>,
    losses: BTreeMap<u64, Option<Acknowledgement>>,
    admitted: Vec<LiveRenderDevice>,
    admitted_sequence: u64,
}

impl LiveRenderDeviceCoordinator {
    pub(super) fn shell_gpu_device(&self) -> Result<Identity, String> {
        if !self.active_available {
            return Err("the active render device is unavailable".into());
        }
        let mut matching = self
            .admitted
            .iter()
            .filter(|device| device.identity == self.active_identity);
        let identity = matching
            .next()
            .ok_or("the active render device is absent from the admitted inventory")?;
        if matching.next().is_some() {
            return Err("the admitted inventory contains an ambiguous active render device".into());
        }
        Ok(identity.identity.clone())
    }

    fn with_preparer(
        seat: String,
        initial: Arc<Bundle>,
        identity: Identity,
        inventory: Vec<LiveRenderDevice>,
        mut prepare_inventory: impl FnMut(PreparationRequest) -> Result<PreparedInventory, String>
        + Send
        + 'static,
    ) -> Result<Self, String> {
        let (prepare, incoming) = mpsc::sync_channel::<PreparationRequest>(1);
        let (completed, prepared) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("sophia-render-devices".into())
            .spawn(move || {
                while let Ok(request) = incoming.recv() {
                    let ticket = request.ticket;
                    let result = prepare_inventory(request);
                    if completed.send((ticket, result)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            seat,
            desired: inventory
                .iter()
                .map(|device| device.identity.clone())
                .collect(),
            ticket: 1,
            next_bundle_generation: initial
                .generation()
                .checked_add(1)
                .ok_or("device generation exhausted")?,
            active_generation: initial.generation(),
            active_identity: identity,
            active_available: true,
            pixmap_textures: initial.supports_pixmap_textures(),
            dirty: false,
            retry_at: Instant::now(),
            preparation: None,
            prepare,
            prepared,
            install: None,
            losses: BTreeMap::new(),
            admitted: inventory,
            admitted_sequence: 1,
        })
    }

    pub(super) fn observe_inventory(
        &mut self,
        identities: &[Identity],
        now: Instant,
    ) -> Result<(), String> {
        if self.desired == identities {
            return Ok(());
        }
        self.ticket = self
            .ticket
            .checked_add(1)
            .ok_or("render inventory sequence exhausted")?;
        self.desired = identities.to_vec();
        self.dirty = true;
        self.retry_at = now;
        if self.active_available && !self.desired.contains(&self.active_identity) {
            self.active_available = false;
            self.losses.entry(self.active_generation).or_insert(None);
        }
        Ok(())
    }

    /// Opened, identity-checked files for the separate renderer inventory handoff.
    pub(super) fn admitted_inventory(&self) -> Option<(u64, &[LiveRenderDevice])> {
        Some((self.admitted_sequence, &self.admitted))
    }

    /// Membership evidence is visible immediately, even while preparation is busy.
    pub(super) fn observed_inventory(&self) -> (u64, &[Identity]) {
        (self.ticket, &self.desired)
    }

    pub(super) fn poll(
        &mut self,
        now: Instant,
        frontend: &SyncSender<Command>,
    ) -> Result<(), String> {
        self.poll_losses(now, frontend)?;
        match self.prepared.try_recv() {
            Ok((ticket, result)) => {
                if self.preparation.as_ref().map(|flight| flight.ticket) != Some(ticket) {
                    return Err("unexpected render preparation completion".into());
                }
                self.preparation = None;
                if ticket == self.ticket {
                    match result {
                        Ok(prepared) => {
                            self.admitted = prepared.devices;
                            self.admitted_sequence = ticket;
                            match prepared.replacement {
                                Ok(Some(prepared)) => {
                                    self.install = Some(PendingInstall {
                                        ticket,
                                        prepared,
                                        acknowledgement: None,
                                    })
                                }
                                Ok(None) => self.dirty = false,
                                Err(error) => self.retry(now, &error),
                            }
                        }
                        Err(error) => self.retry(now, &error),
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Err("render preparation worker stopped".into());
            }
        }
        if let Some(flight) = self.preparation.as_mut()
            && now >= flight.deadline
            && !flight.deadline_reported
        {
            flight.deadline_reported = true;
            tracing::warn!(
                ticket = flight.ticket,
                "render-device preparation deadline exceeded; worker remains quarantined"
            );
        }
        self.poll_install(now, frontend)?;
        if self.dirty
            && self.preparation.is_none()
            && self.install.is_none()
            && now >= self.retry_at
        {
            let request = PreparationRequest {
                ticket: self.ticket,
                bundle_generation: self.next_bundle_generation,
                seat: self.seat.clone(),
                identities: self.desired.clone(),
                preferred_physical: self.active_identity.physical_device.clone(),
                pixmap_textures: self.pixmap_textures,
                replace_default: !self.active_available,
            };
            self.next_bundle_generation = self
                .next_bundle_generation
                .checked_add(1)
                .ok_or("device generation exhausted")?;
            match self.prepare.try_send(request) {
                Ok(()) => {
                    self.preparation = Some(PreparationFlight {
                        ticket: self.ticket,
                        deadline: now + ACK_TIMEOUT,
                        deadline_reported: false,
                    });
                }
                Err(TrySendError::Full(_)) => self.retry_at = now + RETRY,
                Err(TrySendError::Disconnected(_)) => {
                    return Err("render preparation worker stopped".into());
                }
            }
        }
        Ok(())
    }

    fn retry(&mut self, now: Instant, reason: &str) {
        self.dirty = true;
        self.retry_at = now + RETRY;
        tracing::warn!(ticket=self.ticket, %reason, "render-device preparation deferred");
    }

    fn poll_losses(&mut self, now: Instant, frontend: &SyncSender<Command>) -> Result<(), String> {
        let mut completed = Vec::new();
        for (&generation, acknowledgement) in &mut self.losses {
            if let Some((receiver, deadline)) = acknowledgement {
                match receiver.try_recv() {
                    Ok(Ok(()) | Err(BundleError::UnknownGeneration)) => {
                        completed.push(generation);
                        continue;
                    }
                    Ok(Err(error)) => return Err(format!("device loss acknowledgement: {error}")),
                    Err(TryRecvError::Disconnected) => {
                        return Err("device loss acknowledgement disconnected".into());
                    }
                    Err(TryRecvError::Empty) if now >= *deadline => {
                        return Err("device loss acknowledgement timed out".into());
                    }
                    Err(TryRecvError::Empty) => continue,
                }
            }
            let (reply, receiver) = mpsc::sync_channel(1);
            match frontend.try_send(Command::MarkDeviceGenerationUnavailable {
                generation,
                acknowledgement: reply,
            }) {
                Ok(()) => *acknowledgement = Some((receiver, now + ACK_TIMEOUT)),
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {
                    return Err("frontend device service disconnected".into());
                }
            }
        }
        for generation in completed {
            self.losses.remove(&generation);
        }
        Ok(())
    }

    fn poll_install(&mut self, now: Instant, frontend: &SyncSender<Command>) -> Result<(), String> {
        let Some(mut install) = self.install.take() else {
            return Ok(());
        };
        if let Some((receiver, deadline)) = install.acknowledgement.as_ref() {
            match receiver.try_recv() {
                Ok(Ok(())) => {
                    self.active_generation = install.prepared.bundle.generation();
                    self.active_identity = install.prepared.identity;
                    self.active_available = self.desired.contains(&self.active_identity);
                    self.dirty = install.ticket != self.ticket || !self.active_available;
                    if !self.active_available {
                        self.losses.entry(self.active_generation).or_insert(None);
                    }
                    tracing::info!(
                        generation = self.active_generation,
                        available = self.active_available,
                        "frontend render-device generation installed"
                    );
                    return Ok(());
                }
                Ok(Err(BundleError::Capacity)) => {
                    install.acknowledgement = None;
                    self.retry_at = now + RETRY;
                }
                Ok(Err(error)) => {
                    return Err(format!("frontend device installation refused: {error}"));
                }
                Err(TryRecvError::Disconnected) => {
                    return Err("device installation acknowledgement disconnected".into());
                }
                Err(TryRecvError::Empty) if now >= *deadline => {
                    return Err("device installation acknowledgement timed out".into());
                }
                Err(TryRecvError::Empty) => {
                    self.install = Some(install);
                    return Ok(());
                }
            }
        }
        if install.ticket != self.ticket {
            return Ok(());
        }
        if now >= self.retry_at {
            let (reply, receiver) = mpsc::sync_channel(1);
            match frontend.try_send(Command::InstallDeviceBundle {
                bundle: install.prepared.bundle.clone(),
                acknowledgement: reply,
            }) {
                Ok(()) => install.acknowledgement = Some((receiver, now + ACK_TIMEOUT)),
                Err(TrySendError::Full(_)) => self.retry_at = now + RETRY,
                Err(TrySendError::Disconnected(_)) => {
                    return Err("frontend device service disconnected".into());
                }
            }
        }
        self.install = Some(install);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/support/render_device_coordinator.rs"]
mod tests;

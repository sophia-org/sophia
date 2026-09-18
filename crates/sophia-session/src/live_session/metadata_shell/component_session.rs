//! Session join for selected process attempts and their exact borrowed services.
use super::{NativeLauncherActionService, NativeLauncherContentService, PanelComponentService};
use super::{
    RevokedContentGrantLedger,
    component_launch::{ComponentGpuLaunchEvidence, ShellComponentLaunch},
};
use crate::shell_component_connections::{ComponentConnectionKey, ComponentConnectionPhase};
use crate::shell_component_processes::{
    ComponentProcessEvent, ComponentProcessVisit, ShellComponentProcesses,
};
use sophia_backend_live::{LiveProductionVisualRuntime, LiveRenderDeviceIdentitySnapshot};
use sophia_config::{MAX_SHELL_COMPONENTS, ShellComponentConfig, ShellComponentRole, ShellGpuMode};
use sophia_runtime::{
    ContentEpochAccounting, ShellContentAdmissionPolicy, ShellTransportConnection,
};
use std::path::Path;

mod scheduling;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub enum ShellComponentService {
    Bar(Box<PanelComponentService>),
    Launcher {
        content: Box<NativeLauncherContentService>,
        actions: NativeLauncherActionService,
    },
}
struct Ready {
    key: ComponentConnectionKey,
    service: ShellComponentService,
}

/// Retain this owner alongside unresolved backend consumers on every error and
/// shutdown path. `poll` does not start replacements, and join success alone
/// cannot dispose byte owners. The live scheduler supplies presentation state.
pub struct ShellComponentSession {
    processes: ShellComponentProcesses,
    plans: Vec<ShellComponentLaunch>,
    launch_evidence: [Option<(ComponentConnectionKey, Option<ComponentGpuLaunchEvidence>)>;
        MAX_SHELL_COMPONENTS],
    ready: [Option<Ready>; MAX_SHELL_COMPONENTS],
    revoked: RevokedContentGrantLedger,
    panel_limit: u16,
    policy: ShellContentAdmissionPolicy,
    available: bool,
    stopping: bool,
    retained_panel_bands: [Option<(
        ComponentConnectionKey,
        Vec<sophia_protocol::OutputReservation>,
    )>; MAX_SHELL_COMPONENTS],
    retry_at: [Option<std::time::Instant>; MAX_SHELL_COMPONENTS],
    start_cursor: usize,
    last_schedule: Option<std::time::Instant>,
}
impl ShellComponentSession {
    /// `directory` is the already-created Session-private endpoint parent.
    /// Each role endpoint performs its own ownership/mode validation.
    pub fn prepare(
        selections: &[ShellComponentConfig],
        panel_limit: u16,
        device: Option<LiveRenderDeviceIdentitySnapshot>,
        directory: &Path,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<Self> {
        if selections.is_empty() || selections.len() > MAX_SHELL_COMPONENTS {
            return Err("component session requires one through three selected roles".into());
        }
        sophia_config::validate_shell_component_reservations(selections)?;
        if selections
            .iter()
            .any(|selection| selection.role == ShellComponentRole::Dock)
        {
            return Err("persistent catalog component service is not implemented".into());
        }
        // Validate all policies before creating any endpoint. Denied roles
        // never inherit the bar's optional render device.
        let plans = selections
            .iter()
            .map(|selection| {
                ShellComponentLaunch::new(
                    selection.clone(),
                    Some(panel_limit),
                    (selection.gpu == ShellGpuMode::Direct)
                        .then(|| device.clone())
                        .flatten(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut processes = ShellComponentProcesses::new()?;
        for plan in &plans {
            let selected = plan.selection();
            processes.add(
                &selected.id,
                selected.role,
                &directory.join(&selected.id),
                rustix::process::geteuid().as_raw(),
            )?;
        }
        Ok(Self {
            processes,
            plans,
            launch_evidence: std::array::from_fn(|_| None),
            ready: std::array::from_fn(|_| None),
            revoked: Default::default(),
            panel_limit,
            policy,
            available: false,
            stopping: false,
            retained_panel_bands: std::array::from_fn(|_| None),
            retry_at: std::array::from_fn(|_| None),
            start_cursor: 0,
            last_schedule: None,
        })
    }

    /// Evidence belongs to this successfully negotiated attempt, never a later
    /// occupant of the same role slot. Preparation alone is not admission.
    pub fn launch_evidence(
        &self,
        key: ComponentConnectionKey,
    ) -> Result<(ShellComponentRole, Option<&ComponentGpuLaunchEvidence>)> {
        if self.processes.phase(key)? != ComponentConnectionPhase::Connected {
            return Err("component launch evidence requires a connected attempt".into());
        }
        let (observed, gpu) = self
            .launch_evidence
            .get(key.slot)
            .and_then(Option::as_ref)
            .ok_or("component launch evidence missing")?;
        if *observed != key {
            return Err("stale component launch evidence".into());
        }
        Ok((self.plans[key.slot].selection().role, gpu.as_ref()))
    }

    pub fn attempt(&self, slot: usize) -> Option<ComponentConnectionKey> {
        self.processes.attempt(slot)
    }
    pub fn retained_processes(&self) -> usize {
        (0..self.plans.len())
            .filter(|slot| {
                self.processes
                    .attempt(*slot)
                    .is_some_and(|key| self.processes.process_retained(key))
            })
            .count()
    }
    pub fn process_retained(&self, key: ComponentConnectionKey) -> bool {
        self.processes.process_retained(key)
    }
    pub fn accounting(&self) -> ContentEpochAccounting {
        self.processes.accounting()
    }
    pub fn pending_revocations(&self) -> usize {
        self.revoked.len()
    }

    /// Pausing disarms service before signaling. Continue polling this owner to
    /// reap retained processes; failure never changes stopping into available.
    pub fn set_presentation_available(&mut self, available: bool) -> Result<()> {
        self.available = available && !self.stopping;
        if !self.available {
            self.stop_all()?;
        }
        Ok(())
    }
    pub fn request_shutdown(&mut self) -> Result<()> {
        self.stopping = true;
        self.available = false;
        self.stop_all()
    }
    fn stop_all(&mut self) -> Result<()> {
        let mut first = None;
        for slot in 0..self.plans.len() {
            if let Some(key) = self.processes.attempt(slot)
                && let Err(error) = self.stop(key)
                && first.is_none()
            {
                first = Some(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
    pub fn stop(&mut self, key: ComponentConnectionKey) -> Result<()> {
        // Do not let a stale caller fill the cleanup inventory for a successor.
        self.processes.phase(key)?;
        self.revoked.record(key.grant)?;
        self.processes.request_stop(key).map_err(Into::into)
    }
    pub fn start(&mut self, slot: usize) -> Result<ComponentConnectionKey> {
        if !self.available || self.stopping || self.revoked.len() != 0 {
            return Err("component presentation unavailable or cleanup retained".into());
        }
        if let Some(key) = self.processes.attempt(slot)
            && (self.processes.phase(key)? != ComponentConnectionPhase::Revoked
                || self.processes.process_retained(key))
        {
            return Err("component attempt still owned".into());
        }
        let plan = self.plans.get(slot).ok_or("unknown component selection")?;
        let result = self.processes.start(
            slot,
            |key, socket| {
                let (spec, evidence) = plan.prepare(key, socket).map_err(|e| e.to_string())?;
                self.launch_evidence[slot] = Some((key, evidence));
                Ok(spec)
            },
            self.policy,
        );
        match result {
            Ok(key) => Ok(key),
            Err(error) => {
                if let Some(key) = self.processes.attempt(slot) {
                    self.revoked.record(key.grant)?;
                }
                Err(error.into())
            }
        }
    }

    /// One bounded process/negotiation visit. Services are constructed only for
    /// the actual successful role negotiation, not from an advertised profile.
    pub fn poll(&mut self, bytes: usize) -> Result<ComponentProcessVisit> {
        let visit = self.processes.visit(bytes);
        for event in visit.processes.iter().flatten() {
            let key = match event {
                ComponentProcessEvent::ProcessRetired(key, _)
                | ComponentProcessEvent::Failed(key, _) => *key,
            };
            self.revoked.record(key.grant)?;
        }
        for (key, result) in visit.negotiations.iter().flatten() {
            if result.is_err() || !self.available || self.stopping {
                self.stop(*key)?;
            }
        }
        if !self.available || self.stopping {
            return Ok(visit);
        }
        // Reconcile actual connected owners, not only this visit's events. An
        // earlier role failure must not lose a neighbor's successful negotiation.
        for slot in 0..self.plans.len() {
            let Some(key) = self.processes.attempt(slot) else {
                continue;
            };
            if self.processes.phase(key)? != ComponentConnectionPhase::Connected
                || self.ready[slot]
                    .as_ref()
                    .is_some_and(|ready| ready.key == key)
            {
                continue;
            }
            let role = self.plans[key.slot].selection().role;
            let reservation = self.plans[key.slot].selection().reservation;
            let limit = reservation.map_or(self.panel_limit, |p| p.max_thickness);
            let input = matches!(
                self.policy,
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true
                }
            );
            let service = self
                .processes
                .with_connection(key, |transport| match role {
                    ShellComponentRole::Dock => {
                        Err(sophia_runtime::ShellTransportError::MissingCapability)
                    }
                    ShellComponentRole::Bar => PanelComponentService::new(transport, limit, input)
                        .map(|service| {
                            ShellComponentService::Bar(Box::new(
                                service.with_reservation(reservation),
                            ))
                        }),
                    ShellComponentRole::ApplicationLauncher => {
                        NativeLauncherContentService::new(transport).map(|content| {
                            ShellComponentService::Launcher {
                                content: Box::new(content),
                                actions: NativeLauncherActionService::default(),
                            }
                        })
                    }
                })?;
            match service {
                Ok(service) => {
                    if let Some(Ready {
                        key: old_key,
                        service: ShellComponentService::Bar(old),
                    }) = &self.ready[key.slot]
                        && let Some(bands) = old.presented_work_area_bands()
                    {
                        self.retained_panel_bands[key.slot] = Some((*old_key, bands));
                    }
                    self.ready[key.slot] = Some(Ready { key, service });
                }
                Err(error) => {
                    self.stop(key)?;
                    return Err(error.into());
                }
            }
        }
        Ok(visit)
    }

    /// Exact connected attempt plus current presentation permission is required
    /// on every borrow. Retained old service metadata grants no IPC authority.
    pub fn with_service<T>(
        &mut self,
        key: ComponentConnectionKey,
        service: impl FnOnce(&mut ShellComponentService, &mut ShellTransportConnection<'_>) -> T,
    ) -> Result<T> {
        if !self.available || self.stopping {
            return Err("component service paused".into());
        }
        let ready = self
            .ready
            .get_mut(key.slot)
            .and_then(Option::as_mut)
            .filter(|ready| ready.key == key)
            .ok_or("component service has no negotiated owner")?;
        Ok(self
            .processes
            .with_connection(key, |transport| service(&mut ready.service, transport))?)
    }
    pub fn settle_revocations(
        &mut self,
        runtime: Option<&mut LiveProductionVisualRuntime>,
    ) -> Result<usize> {
        let Some(runtime) = runtime else {
            return Ok(0);
        };
        let settled = self.revoked.settle_with(Some(&mut |grant| {
            Ok::<_, Box<dyn std::error::Error>>(
                runtime.revoke_shell_content_retirement_claims(grant),
            )
        }))?;
        Ok(settled.grants)
    }
    /// Keep last displayed panel reservations through peer replacement. A new
    /// negotiated connection cannot change work areas before its first Present.
    pub fn work_area_bands(&self) -> Vec<sophia_protocol::OutputReservation> {
        self.ready
            .iter()
            .enumerate()
            .flat_map(|(slot, ready)| {
                ready
                    .as_ref()
                    .and_then(|ready| match &ready.service {
                        ShellComponentService::Bar(bar) => bar.presented_work_area_bands(),
                        _ => None,
                    })
                    .or_else(|| {
                        self.retained_panel_bands[slot]
                            .as_ref()
                            .map(|(_, bands)| bands.clone())
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    pub fn collect(&mut self) -> ContentEpochAccounting {
        self.processes.collect()
    }

    /// Only after the caller transfers the real final backend/CPU consumers.
    /// Pending process custody or grant cleanup returns that owner unchanged.
    pub fn finish_after_backend_drop<B>(
        &mut self,
        backend: B,
    ) -> std::result::Result<(usize, ContentEpochAccounting), B> {
        if !self.stopping || self.revoked.len() != 0 {
            return Err(backend);
        }
        self.processes.finish_after_backend_drop(backend)
    }
    pub fn phase(&self, key: ComponentConnectionKey) -> Result<ComponentConnectionPhase> {
        Ok(self.processes.phase(key)?)
    }
}

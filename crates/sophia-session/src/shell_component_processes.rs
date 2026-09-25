//! Process custody joined to the single Session component/content registry.
use crate::shell_component_connections::*;
use sophia_config::{MAX_SHELL_COMPONENTS, ShellComponentRole};
use sophia_runtime::*;
use std::path::Path;
use std::time::Duration;

#[cfg(all(test, feature = "native-session"))]
#[path = "../tests/support/component_reconnect/connection.rs"]
pub(crate) mod reconnect_fixture;

#[derive(Default)]
struct ProcessSlot {
    key: Option<ComponentConnectionKey>,
    process: Option<ProcessSupervisor>,
}

#[derive(Debug)]
pub enum ComponentProcessEvent {
    ProcessRetired(ComponentConnectionKey, Result<(), ComponentConnectionError>),
    Failed(ComponentConnectionKey, String),
}
pub struct ComponentProcessVisit {
    pub processes: [Option<ComponentProcessEvent>; MAX_SHELL_COMPONENTS],
    pub negotiations: [Option<ComponentNegotiationEvent>; MAX_SHELL_COMPONENTS],
    pub stop_errors: [Option<(ComponentConnectionKey, String)>; MAX_SHELL_COMPONENTS],
}

pub struct ShellComponentProcesses {
    connections: ShellComponentConnections,
    slots: [ProcessSlot; MAX_SHELL_COMPONENTS],
    cursor: usize,
}
impl ShellComponentProcesses {
    pub fn new() -> Result<Self, ComponentConnectionError> {
        Ok(Self {
            connections: ShellComponentConnections::new()?,
            slots: std::array::from_fn(|_| ProcessSlot::default()),
            cursor: 0,
        })
    }
    pub fn add(
        &mut self,
        id: &str,
        role: ShellComponentRole,
        directory: &Path,
        uid: u32,
    ) -> Result<usize, ComponentConnectionError> {
        self.connections.add(id, role, directory, uid)
    }
    pub fn attempt(&self, slot: usize) -> Option<ComponentConnectionKey> {
        self.slots.get(slot).and_then(|s| s.key)
    }
    /// Reserve before preparing device policy. A failed preparation burns its
    /// epoch. The actual supervisor is installed before spawning, so a returned
    /// launch/negotiation error cannot discard a still-running child.
    pub fn start(
        &mut self,
        slot: usize,
        prepare: impl FnOnce(ComponentConnectionKey, &Path) -> Result<ProcessLaunchSpec, String>,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<ComponentConnectionKey, String> {
        if self.slots.get(slot).is_none_or(|s| s.process.is_some()) {
            return Err("component process busy or unknown".into());
        }
        let key = self
            .connections
            .reserve_attempt(slot)
            .map_err(|e| e.to_string())?;
        self.slots[slot].key = Some(key);
        let result = (|| {
            let path = self
                .connections
                .socket_path(slot)
                .map_err(|e| e.to_string())?;
            let mut spec = prepare(key, path)?;
            if spec.protection_domain.is_none() {
                return Err("component requires protected launch".into());
            }
            spec.environment
                .retain(|(name, _)| name != SOPHIA_SHELL_SOCKET_ENV);
            spec = spec.env(SOPHIA_SHELL_SOCKET_ENV, path).process_group();
            let process = self.slots[slot]
                .process
                .insert(ProcessSupervisor::new(SupervisedProcessKind::Shell, spec));
            process
                .apply(SupervisorCommand::StartProcess {
                    process: SupervisedProcessKind::Shell,
                    delay: Duration::ZERO,
                })
                .map_err(|e| e.to_string())?;
            let evidence = process
                .protection_evidence()
                .ok_or("missing component protection evidence")?
                .clone();
            self.connections
                .begin_negotiation(key, &evidence, Duration::from_secs(5), policy)
                .map_err(|e| e.to_string())?;
            Ok(key)
        })();
        if let Err(error) = result {
            let cleanup = self.request_stop(key).err();
            return Err(format!("{error}; cleanup={cleanup:?}"));
        }
        result
    }
    /// Revoke IPC authority first, then signal without blocking neighboring
    /// service. Errors retain the exact process and prevent replacement.
    pub fn request_stop(&mut self, key: ComponentConnectionKey) -> Result<(), String> {
        let slot = self.slots.get_mut(key.slot).ok_or("unknown component")?;
        if slot.key != Some(key) {
            return Err("stale component process".into());
        }
        let revoke = self.connections.close(key).map_err(|e| e.to_string());
        let signal = slot.process.as_mut().map_or(Ok(()), |p| {
            p.request_termination().map_err(|e| e.to_string())
        });
        revoke.and(signal)
    }
    pub fn visit(&mut self, negotiation_bytes: usize) -> ComponentProcessVisit {
        let mut events = std::array::from_fn(|_| None);
        for (offset, event) in events.iter_mut().enumerate() {
            let index = (self.cursor + offset) % MAX_SHELL_COMPONENTS;
            let slot = &mut self.slots[index];
            let (Some(key), Some(process)) = (slot.key, slot.process.as_mut()) else {
                continue;
            };
            let result = if process.child_id().is_none() {
                Ok(Some(SupervisorEvent::ProcessExited))
            } else {
                process.poll()
            };
            match result {
                Ok(Some(SupervisorEvent::ProcessExited)) => {
                    slot.process = None;
                    *event = Some(ComponentProcessEvent::ProcessRetired(
                        key,
                        self.connections.close(key),
                    ));
                }
                Ok(_) => {}
                Err(error) => {
                    let stop = self.request_stop(key).err();
                    *event = Some(ComponentProcessEvent::Failed(
                        key,
                        format!("{error}; stop={stop:?}"),
                    ));
                }
            }
        }
        self.cursor = (self.cursor + 1) % MAX_SHELL_COMPONENTS;
        let negotiations = self
            .connections
            .poll_negotiations(negotiation_bytes.min(64 * 1024));
        let mut stop_errors = std::array::from_fn(|_| None);
        for (index, (key, result)) in negotiations.iter().flatten().enumerate() {
            if result.is_err()
                && let Err(error) = self.request_stop(*key)
            {
                stop_errors[index] = Some((*key, error));
            }
        }
        ComponentProcessVisit {
            processes: events,
            negotiations,
            stop_errors,
        }
    }
    pub fn with_connection<R>(
        &mut self,
        key: ComponentConnectionKey,
        service: impl FnOnce(&mut ShellTransportConnection<'_>) -> R,
    ) -> Result<R, ComponentConnectionError> {
        self.connections.with_connection(key, service)
    }
    pub fn finish_after_backend_drop<B>(
        &mut self,
        backend: B,
    ) -> Result<(usize, ContentEpochAccounting), B> {
        if self.slots.iter().any(|slot| slot.process.is_some()) {
            return Err(backend);
        }
        self.connections.finish_after_backend_drop(backend)
    }
    pub fn accounting(&self) -> ContentEpochAccounting {
        self.connections.accounting()
    }
    pub fn collect(&mut self) -> ContentEpochAccounting {
        self.connections.collect()
    }
    pub fn phase(
        &self,
        key: ComponentConnectionKey,
    ) -> Result<ComponentConnectionPhase, ComponentConnectionError> {
        self.connections.phase(key)
    }
    pub fn process_retained(&self, key: ComponentConnectionKey) -> bool {
        self.slots
            .get(key.slot)
            .is_some_and(|s| s.key == Some(key) && s.process.is_some())
    }
}

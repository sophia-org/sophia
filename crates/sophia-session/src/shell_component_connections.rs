//! Bounded Session connection ownership. Process launch, role-specific service,
//! focus and native composition are separate owners, not inferred from a socket.
use std::path::Path;
use std::time::Duration;

use sophia_config::{MAX_SHELL_COMPONENTS, ShellComponentRole, ShellTransportSelection};
use sophia_protocol::{ContentGrant, ContentLimits, ShellV1ServerWelcome};
use sophia_runtime::{
    ContentEpochAccounting, ContentEpochRegistry, ContentReconnectAllowance,
    ContentReconnectBudget, ContentStoreError, ContentStoreProfile, ProtectionDomainEvidence,
    ShellComponentTransport, ShellContentAdmissionPolicy, ShellTransportConnection,
    ShellTransportError, content_reconnect_allowance, select_reconnect_limits,
};

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentConnectionKey {
    pub slot: usize,
    pub grant: ContentGrant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentConnectionPhase {
    Reserved,
    Negotiating,
    Connected,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentConnectionError {
    InvalidSelection,
    UnknownComponent,
    StaleAttempt,
    Busy,
    EpochExhausted,
    Transport(ShellTransportError),
}
impl From<ShellTransportError> for ComponentConnectionError {
    fn from(value: ShellTransportError) -> Self {
        Self::Transport(value)
    }
}
impl From<ContentStoreError> for ComponentConnectionError {
    fn from(value: ContentStoreError) -> Self {
        Self::Transport(value.into())
    }
}
impl std::fmt::Display for ComponentConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ComponentConnectionError {}

struct Connection {
    id: String,
    role: ShellComponentRole,
    transport: ShellComponentTransport,
    /// Fixed at registration; a replacement epoch keeps the same wire.
    wire: ShellTransportSelection,
    attempt: Option<(ContentGrant, ComponentConnectionPhase)>,
}

/// One bounded registry for independent connections, with attempt identities burned
/// before reservation. This owner must outlive all connection service borrows
/// and be retained with unresolved content owners on shutdown. Collection never
/// substitutes for native/resource consumer disposition.
pub struct ShellComponentConnections {
    connections: Vec<Connection>,
    epochs: ContentEpochRegistry,
    next_connection: u64,
    next_content: u64,
    cursor: usize,
    connections_have_dock: bool,
}

pub type ComponentNegotiationEvent = (
    ComponentConnectionKey,
    Result<ShellV1ServerWelcome, ShellTransportError>,
);

impl ShellComponentConnections {
    pub fn new() -> Result<Self, ComponentConnectionError> {
        Ok(Self {
            connections: Vec::with_capacity(MAX_SHELL_COMPONENTS),
            epochs: ContentEpochRegistry::with_active_capacity(64 * MIB, MAX_SHELL_COMPONENTS)?,
            next_connection: 1,
            next_content: 1,
            cursor: 0,
            connections_have_dock: false,
        })
    }

    /// Register a selected role before launch. The directory belongs to this
    /// endpoint. Neither the role nor expected UID is protection evidence.
    pub fn add(
        &mut self,
        id: &str,
        role: ShellComponentRole,
        directory: &Path,
        uid: u32,
    ) -> Result<usize, ComponentConnectionError> {
        self.add_with_transport(
            id,
            role,
            directory,
            uid,
            ShellTransportSelection::CurrentIpc,
        )
    }

    /// As [`Self::add`], with the operator's startup wire for this component.
    /// The selection is data: it changes neither admission nor grants.
    pub fn add_with_transport(
        &mut self,
        id: &str,
        role: ShellComponentRole,
        directory: &Path,
        uid: u32,
        wire: ShellTransportSelection,
    ) -> Result<usize, ComponentConnectionError> {
        if self.next_connection != 1
            || self.connections.len() == MAX_SHELL_COMPONENTS
            || id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || self
                .connections
                .iter()
                .any(|c| c.id == id || c.role == role)
        {
            return Err(ComponentConnectionError::InvalidSelection);
        }
        let transport = ShellComponentTransport::bind_for_supervised_uid(directory, uid)?;
        let slot = self.connections.len();
        self.connections_have_dock |= role == ShellComponentRole::Dock;
        self.connections.push(Connection {
            id: id.into(),
            role,
            transport,
            wire,
            attempt: None,
        });
        Ok(slot)
    }

    pub fn transport_selection(
        &self,
        slot: usize,
    ) -> Result<ShellTransportSelection, ComponentConnectionError> {
        Ok(self
            .connections
            .get(slot)
            .ok_or(ComponentConnectionError::UnknownComponent)?
            .wire)
    }

    pub fn socket_path(&self, slot: usize) -> Result<&Path, ComponentConnectionError> {
        Ok(self
            .connections
            .get(slot)
            .ok_or(ComponentConnectionError::UnknownComponent)?
            .transport
            .socket_path())
    }

    /// Called before preparing/spawning the protected process. A refusal burns
    /// both allocated epoch numbers, but never alters the neighboring owner.
    /// A launch failure must close this exact returned key before replacement.
    pub fn reserve_attempt(
        &mut self,
        slot: usize,
    ) -> Result<ComponentConnectionKey, ComponentConnectionError> {
        let connection = self
            .connections
            .get_mut(slot)
            .ok_or(ComponentConnectionError::UnknownComponent)?;
        if connection
            .attempt
            .is_some_and(|(_, phase)| phase != ComponentConnectionPhase::Revoked)
        {
            return Err(ComponentConnectionError::Busy);
        }
        let next_connection = self
            .next_connection
            .checked_add(1)
            .ok_or(ComponentConnectionError::EpochExhausted)?;
        let next_content = self
            .next_content
            .checked_add(1)
            .ok_or(ComponentConnectionError::EpochExhausted)?;
        let grant = ContentGrant {
            connection_epoch: self.next_connection,
            content_grant_epoch: self.next_content,
        };
        self.next_connection = next_connection;
        self.next_content = next_content;
        let profile = match connection.role {
            ShellComponentRole::Bar => ContentStoreProfile::Legacy,
            ShellComponentRole::Dock => ContentStoreProfile::PersistentCatalog,
            ShellComponentRole::ApplicationLauncher => ContentStoreProfile::NativeLauncher,
        };
        // add() enforces unique roles and locks the role set before the first
        // attempt. Thus this immutable nominal envelope has exactly one profile
        // owner, including every retained predecessor of a reduced successor.
        let nominal = role_limits(connection.role, grant, self.connections_have_dock);
        self.epochs.collect();
        let budget = self.epochs.reconnect_budget(profile);
        let allowance = content_reconnect_allowance(&nominal, budget)?;
        let result = select_reconnect_limits(&nominal, budget)
            .map_err(ShellTransportError::from)
            .and_then(|limits| {
                connection.transport.reserve_content_with_profile(
                    &mut self.epochs,
                    limits.clone(),
                    profile,
                )?;
                Ok(limits)
            });
        match result {
            Ok(limits) if limits != nominal => record_reconnect_budget(
                slot,
                connection.role,
                &nominal,
                budget,
                allowance,
                Some(&limits),
            ),
            Ok(_) => {}
            Err(error) => {
                if matches!(
                    error,
                    ShellTransportError::ContentStore(ContentStoreError::Budget)
                ) {
                    record_reconnect_budget(
                        slot,
                        connection.role,
                        &nominal,
                        budget,
                        allowance,
                        None,
                    );
                }
                return Err(error.into());
            }
        }
        connection.attempt = Some((grant, ComponentConnectionPhase::Reserved));
        Ok(ComponentConnectionKey { slot, grant })
    }

    pub fn begin_negotiation(
        &mut self,
        key: ComponentConnectionKey,
        evidence: &ProtectionDomainEvidence,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<(), ComponentConnectionError> {
        self.require_phase(key, ComponentConnectionPhase::Reserved)?;
        let connection = &mut self.connections[key.slot];
        let result = connection
            .transport
            .authorize_protected_peer(evidence)
            .and_then(|()| match connection.wire {
                ShellTransportSelection::CurrentIpc => connection.transport.begin_negotiation(
                    &self.epochs,
                    key.grant.connection_epoch,
                    timeout,
                    policy,
                ),
                ShellTransportSelection::NineP2000L => connection.transport.begin_file_negotiation(
                    &self.epochs,
                    key.grant.connection_epoch,
                    timeout,
                    policy,
                ),
            });
        if let Err(error) = result {
            let _ = self.close(key);
            return Err(error.into());
        }
        connection.attempt = Some((key.grant, ComponentConnectionPhase::Negotiating));
        Ok(())
    }

    /// Rotates first visit and gives each pending connection its own bounded
    /// nonblocking handshake budget. An error is emitted once for its exact
    /// owner; it does not stop the other visit. This is negotiation fairness,
    /// not a claim about process spawning or ordinary role-specific dispatch.
    pub fn poll_negotiations(
        &mut self,
        byte_budget_per_connection: usize,
    ) -> [Option<ComponentNegotiationEvent>; MAX_SHELL_COMPONENTS] {
        let mut events = std::array::from_fn(|_| None);
        let count = self.connections.len();
        if count == 0 {
            return events;
        }
        let first = self.cursor;
        self.cursor = (first + 1) % count;
        for (offset, event) in events.iter_mut().take(count).enumerate() {
            let slot = (first + offset) % count;
            let connection = &mut self.connections[slot];
            let Some((grant, ComponentConnectionPhase::Negotiating)) = connection.attempt else {
                continue;
            };
            match connection
                .transport
                .poll_negotiation(&mut self.epochs, byte_budget_per_connection)
            {
                Ok(None) => {}
                Ok(Some(welcome)) => {
                    connection.attempt = Some((grant, ComponentConnectionPhase::Connected));
                    *event = Some((ComponentConnectionKey { slot, grant }, Ok(welcome)));
                }
                Err(error) => {
                    connection.attempt = Some((grant, ComponentConnectionPhase::Revoked));
                    *event = Some((ComponentConnectionKey { slot, grant }, Err(error)));
                }
            }
        }
        events
    }

    /// The callback cannot retain either mutable owner or lend a second
    /// registry. Admission/disconnect remain on this owner, not this view.
    pub fn with_connection<R>(
        &mut self,
        key: ComponentConnectionKey,
        service: impl FnOnce(&mut ShellTransportConnection<'_>) -> R,
    ) -> Result<R, ComponentConnectionError> {
        self.require_phase(key, ComponentConnectionPhase::Connected)?;
        Ok(service(
            &mut self.connections[key.slot]
                .transport
                .connection(&mut self.epochs),
        ))
    }

    /// Idempotent for this attempt, refusing an old key after replacement.
    /// Disconnection removes authority, not real retained byte consumers.
    pub fn close(&mut self, key: ComponentConnectionKey) -> Result<(), ComponentConnectionError> {
        let connection = self
            .connections
            .get_mut(key.slot)
            .ok_or(ComponentConnectionError::UnknownComponent)?;
        let Some((grant, phase)) = connection.attempt else {
            return Err(ComponentConnectionError::StaleAttempt);
        };
        if grant != key.grant {
            return Err(ComponentConnectionError::StaleAttempt);
        }
        if phase == ComponentConnectionPhase::Revoked {
            return Ok(());
        }
        let result = connection.transport.disconnect(&mut self.epochs);
        connection.attempt = Some((grant, ComponentConnectionPhase::Revoked));
        result.map_err(Into::into)
    }

    /// Final Session backend shutdown only. Refuse without dropping the actual
    /// owner while any connection attempt remains active. The registry then
    /// drops that owner before settling disconnected submissions. A remaining
    /// real consumer keeps accounting non-quiescent; this does not manufacture
    /// native retirement or prove that a worker join disposed its resources.
    pub fn finish_after_backend_drop<B>(
        &mut self,
        backend: B,
    ) -> Result<(usize, ContentEpochAccounting), B> {
        if self.connections.iter().any(|connection| {
            connection
                .attempt
                .is_some_and(|(_, phase)| phase != ComponentConnectionPhase::Revoked)
        }) {
            return Err(backend);
        }
        let settled = self.epochs.finish_after_backend_drop(backend)?;
        Ok((settled, self.accounting()))
    }

    pub fn accounting(&self) -> ContentEpochAccounting {
        self.epochs.accounting()
    }
    pub fn collect(&mut self) -> ContentEpochAccounting {
        self.epochs.collect();
        self.accounting()
    }

    pub fn phase(
        &self,
        key: ComponentConnectionKey,
    ) -> Result<ComponentConnectionPhase, ComponentConnectionError> {
        let connection = self
            .connections
            .get(key.slot)
            .ok_or(ComponentConnectionError::UnknownComponent)?;
        match connection.attempt {
            Some((grant, phase)) if grant == key.grant => Ok(phase),
            _ => Err(ComponentConnectionError::StaleAttempt),
        }
    }
    fn require_phase(
        &self,
        key: ComponentConnectionKey,
        expected: ComponentConnectionPhase,
    ) -> Result<(), ComponentConnectionError> {
        if self.phase(key)? != expected {
            return Err(ComponentConnectionError::Busy);
        }
        Ok(())
    }
}

fn role_limits(role: ShellComponentRole, grant: ContentGrant, has_dock: bool) -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant);
    if has_dock {
        limits.max_staging_bytes = 4 * MIB;
        limits.max_resident_bytes = if role == ShellComponentRole::Bar {
            12 * MIB
        } else {
            8 * MIB
        };
        limits.max_retiring_bytes = 8 * MIB;
    } else if role == ShellComponentRole::ApplicationLauncher {
        limits.max_staging_bytes = 4 * MIB;
        limits.max_resident_bytes = 12 * MIB;
        limits.max_retiring_bytes = 8 * MIB;
    }
    limits
}

fn record_reconnect_budget(
    slot: usize,
    role: ShellComponentRole,
    nominal: &ContentLimits,
    budget: ContentReconnectBudget,
    allowance: ContentReconnectAllowance,
    granted: Option<&ContentLimits>,
) {
    let status = if granted.is_some() {
        "admitted_reduced"
    } else {
        "admission_refused"
    };
    let role = match role {
        ShellComponentRole::Bar => "bar",
        ShellComponentRole::ApplicationLauncher => "application_launcher",
        ShellComponentRole::Dock => "dock",
    };
    let nominal_bytes =
        nominal.max_staging_bytes + nominal.max_resident_bytes + nominal.max_retiring_bytes;
    let ContentReconnectAllowance {
        available_bytes,
        available_backing_bytes,
        required_bytes,
        required_backing_bytes,
    } = allowance;
    let constraint =
        if available_bytes < required_bytes || available_backing_bytes < required_backing_bytes {
            "bytes"
        } else if granted.is_none()
            && (budget.active_epochs >= MAX_SHELL_COMPONENTS
                || budget.active_epochs + budget.retired_epochs
                    >= ContentEpochRegistry::MAX_RETAINED_EPOCHS)
        {
            "epochs"
        } else if granted.is_none() {
            "reservation"
        } else {
            "bytes"
        };
    let (staging_bytes, resident_bytes, retiring_bytes) = granted.map_or((0, 0, 0), |limits| {
        (
            limits.max_staging_bytes,
            limits.max_resident_bytes,
            limits.max_retiring_bytes,
        )
    });
    // Emit typed host accounting before the process owner reduces errors to
    // text. One record per actual attempt, paced by the existing retry owner.
    crate::session_eprintln!(
        "sophia_shell_component schema=1 status={status} cause=content_budget budget_constraint={constraint} slot={slot} role={role} connection_epoch={} content_grant_epoch={} source_capacity_bytes={} backing_capacity_bytes={} nominal_bytes={nominal_bytes} own_retired_bytes={} own_retired_epochs={} reserved_bytes={} reserved_backing_bytes={} available_bytes={available_bytes} available_backing_bytes={available_backing_bytes} required_bytes={required_bytes} required_backing_bytes={required_backing_bytes} active_epochs={} retired_epochs={} active_capacity={} epoch_capacity={} staging_bytes={staging_bytes} resident_bytes={resident_bytes} retiring_bytes={retiring_bytes}",
        nominal.grant.connection_epoch,
        nominal.grant.content_grant_epoch,
        budget.capacity_bytes,
        budget.capacity_backing_bytes,
        budget.own_retired_bytes,
        budget.own_retired_epochs,
        budget.reserved_bytes,
        budget.reserved_backing_bytes,
        budget.active_epochs,
        budget.retired_epochs,
        MAX_SHELL_COMPONENTS,
        ContentEpochRegistry::MAX_RETAINED_EPOCHS,
    );
}

use std::collections::BTreeSet;

use sophia_protocol::{
    PROJECTION_INDICATOR_RECORD_KIND, PROJECTION_OUTPUT_RECORD_KIND,
    PROJECTION_OUTPUT_STATUS_RECORD_KIND, PROJECTION_PLACEMENT_RECORD_KIND,
    SNAPSHOT_ACTION_RECORD_KIND, SNAPSHOT_OUTPUT_RECORD_KIND,
    SNAPSHOT_SESSION_OPERATION_RECORD_KIND, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
    SNAPSHOT_SURFACE_RECORD_KIND, SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_BINDINGS,
    SOPHIA_WM_CAPABILITY_CHROME, SOPHIA_WM_CAPABILITY_CONFIGURATION,
    SOPHIA_WM_CAPABILITY_INDICATORS, SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
    SOPHIA_WM_CAPABILITY_MULTI_OUTPUT, SOPHIA_WM_CAPABILITY_POINTER_INTERACTIONS,
    SOPHIA_WM_CAPABILITY_POLICY_DIRTY, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS, SOPHIA_WM_INTERFACE_REVISION, SOPHIA_WM_MAX_BINDINGS,
    SOPHIA_WM_MAX_OUTPUTS, SOPHIA_WM_MAX_SURFACES, TransactionId, WmV1ClientHello,
    WmV1ProjectionBegin, WmV1ProjectionChunk, WmV1ProjectionEnd, WmV1ProjectionTransfer,
    WmV1ServerWelcome, WmV1SnapshotBegin, WmV1SnapshotChunk, WmV1SnapshotEnd, WmV1SnapshotTransfer,
};

pub const POLICY_MAX_TRANSFER_CHUNKS: usize = 1024;
pub const POLICY_MAX_TRANSFER_BYTES: usize = 512 * 1024;

const POLICY_SUPPORTED_CAPABILITIES: u64 = SOPHIA_WM_CAPABILITY_BINDINGS
    | SOPHIA_WM_CAPABILITY_ACTIONS
    | SOPHIA_WM_CAPABILITY_MULTI_OUTPUT
    | SOPHIA_WM_CAPABILITY_POINTER_INTERACTIONS
    | SOPHIA_WM_CAPABILITY_CHROME
    | SOPHIA_WM_CAPABILITY_POLICY_DIRTY
    | SOPHIA_WM_CAPABILITY_CONFIGURATION
    | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS
    | SOPHIA_WM_CAPABILITY_INDICATORS
    | SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT
    | sophia_protocol::SOPHIA_WM_CAPABILITY_TAB_GROUPS
    | sophia_protocol::SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS
    | sophia_protocol::SOPHIA_WM_CAPABILITY_POINTER_FOCUS
    | sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
    | sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS
    | sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS
    | sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyTransferError {
    NotConnected,
    AlreadyConnected,
    NotNegotiated,
    AlreadyNegotiated,
    UnsupportedRevision,
    InvalidConnectionEpoch,
    InvalidTransaction,
    ReusedTransaction,
    TransferInProgress,
    QueuedTransferPending,
    NoTransfer,
    WrongTransferIdentity,
    InvalidCount,
    ExcessiveCount,
    ExcessiveBytes,
    DuplicateOrReorderedChunk,
    UnknownRecordKind,
    RecordCountMismatch,
    UnsupportedCapability,
}

impl core::fmt::Display for PolicyTransferError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyTransferError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssembledPolicyProjection {
    pub transaction: TransactionId,
    pub connection_epoch: u64,
    pub request_id: u64,
    pub base_generation: u64,
    pub active_output: u64,
    pub output_count: u16,
    pub placement_count: u32,
    pub indicator_count: u16,
    pub status_count: u16,
    pub chunks: Vec<WmV1ProjectionChunk>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssembledPolicySnapshot {
    pub transaction: TransactionId,
    pub connection_epoch: u64,
    pub scene_generation: u64,
    pub active_output: u64,
    pub output_count: u16,
    pub surface_count: u32,
    pub action_count: u16,
    pub session_operation_count: u16,
    /// Frozen ordinary-chunk count. Capability-gated extension chunks in
    /// `chunks` append after this prefix and are deliberately uncounted.
    pub chunk_count: u16,
    pub chunks: Vec<WmV1SnapshotChunk>,
}

impl AssembledPolicyProjection {
    pub fn into_wire_transfer(self) -> WmV1ProjectionTransfer {
        let chunk_count = self
            .chunks
            .iter()
            .take_while(|chunk| chunk.record_kind < 0xff00)
            .count() as u16;
        WmV1ProjectionTransfer {
            transaction: self.transaction,
            begin: WmV1ProjectionBegin {
                connection_epoch: self.connection_epoch,
                request_id: self.request_id,
                base_generation: self.base_generation,
                active_output: self.active_output,
                chunk_count,
                output_count: self.output_count,
                placement_count: self.placement_count,
                indicator_count: self.indicator_count,
                status_count: self.status_count,
            },
            chunks: self.chunks,
            end: WmV1ProjectionEnd {
                connection_epoch: self.connection_epoch,
                request_id: self.request_id,
                base_generation: self.base_generation,
                chunk_count,
            },
        }
    }
}

impl AssembledPolicySnapshot {
    pub fn into_wire_transfer(self) -> WmV1SnapshotTransfer {
        let chunk_count = self.chunk_count;
        WmV1SnapshotTransfer {
            transaction: self.transaction,
            begin: WmV1SnapshotBegin {
                connection_epoch: self.connection_epoch,
                scene_generation: self.scene_generation,
                active_output: self.active_output,
                chunk_count,
                output_count: self.output_count,
                surface_count: self.surface_count,
                action_count: self.action_count,
                session_operation_count: self.session_operation_count,
            },
            chunks: self.chunks,
            end: WmV1SnapshotEnd {
                connection_epoch: self.connection_epoch,
                scene_generation: self.scene_generation,
                chunk_count,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueuedPolicyProjection {
    Admitted(AssembledPolicyProjection),
    Discarded(AssembledPolicyProjection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectionTransfer {
    transaction: TransactionId,
    begin: WmV1ProjectionBegin,
    chunks: Vec<WmV1ProjectionChunk>,
    bytes: usize,
    output_records: usize,
    placement_records: usize,
    indicator_records: usize,
    status_records: usize,
}

/// Session-side connection reducer for the exclusive WM role.
///
/// Socket I/O may queue a complete transfer, but only `settle_queued` can
/// admit it. This preserves the connection epoch across a worker disconnect.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct PolicyConnectionState {
    connected: bool,
    negotiated: bool,
    connection_epoch: u64,
    selected_revision: u16,
    selected_capabilities: u64,
    used_transactions: BTreeSet<TransactionId>,
    transfer: Option<ProjectionTransfer>,
    queued: Option<AssembledPolicyProjection>,
}

impl PolicyConnectionState {
    pub fn connect(&mut self, connection_epoch: u64) -> Result<(), PolicyTransferError> {
        if self.connected {
            return Err(PolicyTransferError::AlreadyConnected);
        }
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(PolicyTransferError::InvalidConnectionEpoch);
        }
        self.connected = true;
        self.negotiated = false;
        self.connection_epoch = connection_epoch;
        self.selected_revision = 0;
        self.selected_capabilities = 0;
        self.used_transactions.clear();
        self.transfer = None;
        Ok(())
    }

    pub fn negotiate(
        &mut self,
        hello: &WmV1ClientHello,
    ) -> Result<WmV1ServerWelcome, PolicyTransferError> {
        self.negotiate_profile_activation(hello, false)
    }

    pub(crate) fn negotiate_profile_activation(
        &mut self,
        hello: &WmV1ClientHello,
        profile_activation: bool,
    ) -> Result<WmV1ServerWelcome, PolicyTransferError> {
        if !self.connected {
            return Err(PolicyTransferError::NotConnected);
        }
        if self.negotiated {
            return Err(PolicyTransferError::AlreadyNegotiated);
        }
        let selected = SOPHIA_WM_INTERFACE_REVISION;
        if hello.minimum_revision == 0
            || hello.minimum_revision > hello.maximum_revision
            || selected < hello.minimum_revision
            || selected > hello.maximum_revision
        {
            return Err(PolicyTransferError::UnsupportedRevision);
        }
        self.negotiated = true;
        self.selected_revision = selected;
        let supported = POLICY_SUPPORTED_CAPABILITIES
            | if profile_activation {
                SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
            } else {
                0
            };
        self.selected_capabilities = hello.capabilities & supported;
        if self.selected_capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN == 0 {
            self.selected_capabilities &=
                !sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT;
        }
        Ok(WmV1ServerWelcome {
            selected_revision: selected,
            capabilities: self.selected_capabilities,
            connection_epoch: self.connection_epoch,
            max_outputs: SOPHIA_WM_MAX_OUTPUTS as u16,
            max_bindings: SOPHIA_WM_MAX_BINDINGS as u16,
            max_surfaces: SOPHIA_WM_MAX_SURFACES as u32,
            max_chunk_bytes: 65_520,
        })
    }

    pub fn begin_projection(
        &mut self,
        transaction: TransactionId,
        begin: WmV1ProjectionBegin,
    ) -> Result<(), PolicyTransferError> {
        self.require_negotiated()?;
        if !transaction.is_valid() {
            return Err(PolicyTransferError::InvalidTransaction);
        }
        if begin.connection_epoch != self.connection_epoch {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if self.transfer.is_some() {
            return Err(PolicyTransferError::TransferInProgress);
        }
        if self.queued.is_some() {
            return Err(PolicyTransferError::QueuedTransferPending);
        }
        if self.used_transactions.contains(&transaction) {
            return Err(PolicyTransferError::ReusedTransaction);
        }
        if begin.request_id == 0
            || begin.base_generation == 0
            || begin.chunk_count == 0
            || begin.output_count == 0
        {
            return Err(PolicyTransferError::InvalidCount);
        }
        if usize::from(begin.chunk_count) > POLICY_MAX_TRANSFER_CHUNKS
            || usize::from(begin.output_count) > SOPHIA_WM_MAX_OUTPUTS
            || begin.placement_count as usize > SOPHIA_WM_MAX_SURFACES
            || usize::from(begin.indicator_count) > sophia_protocol::POLICY_MAX_INDICATORS
            || usize::from(begin.status_count) > sophia_protocol::POLICY_MAX_OUTPUT_STATUSES
        {
            return Err(PolicyTransferError::ExcessiveCount);
        }
        if self.selected_capabilities & SOPHIA_WM_CAPABILITY_INDICATORS == 0
            && (begin.indicator_count != 0 || begin.status_count != 0)
        {
            return Err(PolicyTransferError::UnsupportedCapability);
        }
        self.used_transactions.insert(transaction);
        self.transfer = Some(ProjectionTransfer {
            transaction,
            begin,
            chunks: Vec::new(),
            bytes: 0,
            output_records: 0,
            placement_records: 0,
            indicator_records: 0,
            status_records: 0,
        });
        Ok(())
    }

    pub fn append_projection_chunk(
        &mut self,
        transaction: TransactionId,
        chunk: WmV1ProjectionChunk,
    ) -> Result<(), PolicyTransferError> {
        self.require_negotiated()?;
        let transfer = self
            .transfer
            .as_mut()
            .ok_or(PolicyTransferError::NoTransfer)?;
        if transaction != transfer.transaction || chunk.connection_epoch != self.connection_epoch {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if usize::from(chunk.ordinal) != transfer.chunks.len() {
            return Err(PolicyTransferError::DuplicateOrReorderedChunk);
        }
        if chunk.item_count == 0 || chunk.data.is_empty() {
            return Err(PolicyTransferError::InvalidCount);
        }
        let next_bytes = transfer
            .bytes
            .checked_add(chunk.data.len())
            .filter(|bytes| *bytes <= POLICY_MAX_TRANSFER_BYTES)
            .ok_or(PolicyTransferError::ExcessiveBytes)?;
        let count = chunk.item_count as usize;
        if matches!(
            chunk.record_kind,
            sophia_protocol::PROJECTION_TAB_GROUP_RECORD_KIND
                | sophia_protocol::PROJECTION_TAB_MEMBER_RECORD_KIND
                | sophia_protocol::PROJECTION_TRANSLATION_GROUP_RECORD_KIND
                | sophia_protocol::PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
                | sophia_protocol::PROJECTION_LAUNCH_CONTEXT_RECORD_KIND
                | sophia_protocol::PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND
        ) {
            let translation = matches!(
                chunk.record_kind,
                sophia_protocol::PROJECTION_TRANSLATION_GROUP_RECORD_KIND
                    | sophia_protocol::PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
            );
            let launch_context =
                chunk.record_kind == sophia_protocol::PROJECTION_LAUNCH_CONTEXT_RECORD_KIND;
            let output_context =
                chunk.record_kind == sophia_protocol::PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND;
            let capability = if output_context {
                sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT
            } else if launch_context {
                sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
            } else if translation {
                sophia_protocol::SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS
            } else {
                sophia_protocol::SOPHIA_WM_CAPABILITY_TAB_GROUPS
            };
            if self.selected_capabilities & capability == 0 {
                return Err(PolicyTransferError::UnsupportedCapability);
            }
            if transfer.chunks.len() < usize::from(transfer.begin.chunk_count)
                || transfer.chunks.len() >= POLICY_MAX_TRANSFER_CHUNKS
            {
                return Err(PolicyTransferError::RecordCountMismatch);
            }
            let (size, maximum) = if output_context {
                (
                    sophia_protocol::OUTPUT_LAUNCH_CONTEXT_RECORD_LEN,
                    sophia_protocol::POLICY_MAX_OUTPUTS,
                )
            } else if launch_context {
                (
                    sophia_protocol::LAUNCH_CONTEXT_RECORD_LEN,
                    sophia_protocol::POLICY_MAX_SURFACES,
                )
            } else if translation {
                if chunk.record_kind == sophia_protocol::PROJECTION_TRANSLATION_GROUP_RECORD_KIND {
                    (32, sophia_protocol::POLICY_MAX_OUTPUTS)
                } else {
                    (24, sophia_protocol::POLICY_MAX_SURFACES)
                }
            } else if chunk.record_kind == sophia_protocol::PROJECTION_TAB_GROUP_RECORD_KIND {
                (48, sophia_protocol::POLICY_MAX_TAB_GROUPS)
            } else {
                (24, sophia_protocol::POLICY_MAX_TAB_MEMBERS)
            };
            let prior: usize = transfer
                .chunks
                .iter()
                .filter(|c| c.record_kind == chunk.record_kind)
                .map(|c| c.item_count as usize)
                .sum();
            if count > maximum.saturating_sub(prior) || chunk.data.len() != count * size {
                return Err(PolicyTransferError::RecordCountMismatch);
            }
            transfer.bytes = next_bytes;
            transfer.chunks.push(chunk);
            return Ok(());
        }
        if transfer.chunks.len() >= usize::from(transfer.begin.chunk_count) {
            return Err(PolicyTransferError::RecordCountMismatch);
        }
        match chunk.record_kind {
            PROJECTION_OUTPUT_RECORD_KIND => {
                let next = transfer
                    .output_records
                    .checked_add(count)
                    .filter(|total| *total <= usize::from(transfer.begin.output_count))
                    .ok_or(PolicyTransferError::RecordCountMismatch)?;
                transfer.output_records = next;
            }
            PROJECTION_PLACEMENT_RECORD_KIND => {
                let next = transfer
                    .placement_records
                    .checked_add(count)
                    .filter(|total| *total <= transfer.begin.placement_count as usize)
                    .ok_or(PolicyTransferError::RecordCountMismatch)?;
                transfer.placement_records = next;
            }
            PROJECTION_INDICATOR_RECORD_KIND => {
                let next = transfer
                    .indicator_records
                    .checked_add(count)
                    .filter(|total| *total <= usize::from(transfer.begin.indicator_count))
                    .ok_or(PolicyTransferError::RecordCountMismatch)?;
                transfer.indicator_records = next;
            }
            PROJECTION_OUTPUT_STATUS_RECORD_KIND => {
                let next = transfer
                    .status_records
                    .checked_add(count)
                    .filter(|total| *total <= usize::from(transfer.begin.status_count))
                    .ok_or(PolicyTransferError::RecordCountMismatch)?;
                transfer.status_records = next;
            }
            _ => return Err(PolicyTransferError::UnknownRecordKind),
        }
        transfer.bytes = next_bytes;
        transfer.chunks.push(chunk);
        Ok(())
    }

    pub fn finish_projection(
        &mut self,
        transaction: TransactionId,
        end: WmV1ProjectionEnd,
    ) -> Result<(), PolicyTransferError> {
        self.require_negotiated()?;
        let transfer = self
            .transfer
            .as_ref()
            .ok_or(PolicyTransferError::NoTransfer)?;
        if transaction != transfer.transaction
            || end.connection_epoch != self.connection_epoch
            || end.request_id != transfer.begin.request_id
            || end.base_generation != transfer.begin.base_generation
            || end.chunk_count != transfer.begin.chunk_count
        {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if transfer.chunks.len() < usize::from(transfer.begin.chunk_count)
            || transfer.output_records != usize::from(transfer.begin.output_count)
            || transfer.placement_records != transfer.begin.placement_count as usize
            || transfer.indicator_records != usize::from(transfer.begin.indicator_count)
            || transfer.status_records != usize::from(transfer.begin.status_count)
        {
            return Err(PolicyTransferError::RecordCountMismatch);
        }
        let transfer = self.transfer.take().expect("transfer was checked");
        self.queued = Some(AssembledPolicyProjection {
            transaction: transfer.transaction,
            connection_epoch: transfer.begin.connection_epoch,
            request_id: transfer.begin.request_id,
            base_generation: transfer.begin.base_generation,
            active_output: transfer.begin.active_output,
            output_count: transfer.begin.output_count,
            placement_count: transfer.begin.placement_count,
            indicator_count: transfer.begin.indicator_count,
            status_count: transfer.begin.status_count,
            chunks: transfer.chunks,
        });
        Ok(())
    }

    pub fn settle_queued(&mut self) -> Option<QueuedPolicyProjection> {
        let queued = self.queued.take()?;
        if self.connected && self.negotiated && queued.connection_epoch == self.connection_epoch {
            Some(QueuedPolicyProjection::Admitted(queued))
        } else {
            Some(QueuedPolicyProjection::Discarded(queued))
        }
    }

    pub fn disconnect(&mut self) -> Result<(), PolicyTransferError> {
        if !self.connected {
            return Err(PolicyTransferError::NotConnected);
        }
        self.connected = false;
        self.negotiated = false;
        self.selected_revision = 0;
        self.selected_capabilities = 0;
        self.transfer = None;
        Ok(())
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub const fn negotiated(&self) -> bool {
        self.negotiated
    }

    pub const fn selected_capabilities(&self) -> u64 {
        self.selected_capabilities
    }

    pub fn admit_control_message(
        &mut self,
        transaction: TransactionId,
        connection_epoch: u64,
        required_capability: u64,
    ) -> Result<(), PolicyTransferError> {
        self.require_negotiated()?;
        if !transaction.is_valid() {
            return Err(PolicyTransferError::InvalidTransaction);
        }
        if connection_epoch != self.connection_epoch {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if self.selected_capabilities & required_capability == 0 {
            return Err(PolicyTransferError::UnsupportedCapability);
        }
        if self.transfer.is_some() {
            return Err(PolicyTransferError::TransferInProgress);
        }
        if !self.used_transactions.insert(transaction) {
            return Err(PolicyTransferError::ReusedTransaction);
        }
        Ok(())
    }

    pub(crate) fn admit_server_completion(
        &self,
        transaction: TransactionId,
        connection_epoch: u64,
        required_capability: u64,
    ) -> Result<(), PolicyTransferError> {
        // Completion identities correlate server-originated commands. They do
        // not consume the independent client-control transaction namespace.
        if !transaction.is_valid() {
            return Err(PolicyTransferError::InvalidTransaction);
        }
        self.require_server_control(connection_epoch, required_capability)
    }

    pub(crate) fn require_server_control(
        &self,
        connection_epoch: u64,
        required_capability: u64,
    ) -> Result<(), PolicyTransferError> {
        self.require_negotiated()?;
        if connection_epoch != self.connection_epoch {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if self.selected_capabilities & required_capability == 0 {
            return Err(PolicyTransferError::UnsupportedCapability);
        }
        if self.transfer.is_some() {
            return Err(PolicyTransferError::TransferInProgress);
        }
        Ok(())
    }

    fn require_negotiated(&self) -> Result<(), PolicyTransferError> {
        if !self.connected {
            Err(PolicyTransferError::NotConnected)
        } else if !self.negotiated {
            Err(PolicyTransferError::NotNegotiated)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SnapshotTransfer {
    transaction: TransactionId,
    begin: WmV1SnapshotBegin,
    chunks: Vec<WmV1SnapshotChunk>,
    bytes: usize,
    output_records: usize,
    surface_records: usize,
    action_records: usize,
    session_operation_records: usize,
    classification_records: usize,
}

/// Reference client-side assembler for complete server snapshots.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicySnapshotAssembler {
    connection_epoch: u64,
    selected_capabilities: u64,
    used_transactions: BTreeSet<TransactionId>,
    transfer: Option<SnapshotTransfer>,
}

impl PolicySnapshotAssembler {
    pub fn new(connection_epoch: u64) -> Result<Self, PolicyTransferError> {
        Self::new_with_capabilities(connection_epoch, 0)
    }

    pub fn new_with_capabilities(
        connection_epoch: u64,
        selected_capabilities: u64,
    ) -> Result<Self, PolicyTransferError> {
        if connection_epoch == 0 {
            return Err(PolicyTransferError::InvalidConnectionEpoch);
        }
        Ok(Self {
            connection_epoch,
            selected_capabilities,
            used_transactions: BTreeSet::new(),
            transfer: None,
        })
    }

    pub fn begin(
        &mut self,
        transaction: TransactionId,
        begin: WmV1SnapshotBegin,
    ) -> Result<(), PolicyTransferError> {
        if !transaction.is_valid() {
            return Err(PolicyTransferError::InvalidTransaction);
        }
        if begin.connection_epoch != self.connection_epoch {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if self.transfer.is_some() {
            return Err(PolicyTransferError::TransferInProgress);
        }
        if self.used_transactions.contains(&transaction) {
            return Err(PolicyTransferError::ReusedTransaction);
        }
        if begin.scene_generation == 0 || begin.chunk_count == 0 || begin.output_count == 0 {
            return Err(PolicyTransferError::InvalidCount);
        }
        if usize::from(begin.chunk_count) > POLICY_MAX_TRANSFER_CHUNKS
            || usize::from(begin.output_count) > SOPHIA_WM_MAX_OUTPUTS
            || begin.surface_count as usize > SOPHIA_WM_MAX_SURFACES
            || usize::from(begin.action_count) > SOPHIA_WM_MAX_BINDINGS
            || usize::from(begin.session_operation_count) > SOPHIA_WM_MAX_BINDINGS
        {
            return Err(PolicyTransferError::ExcessiveCount);
        }
        self.used_transactions.insert(transaction);
        self.transfer = Some(SnapshotTransfer {
            transaction,
            begin,
            chunks: Vec::new(),
            bytes: 0,
            output_records: 0,
            surface_records: 0,
            action_records: 0,
            session_operation_records: 0,
            classification_records: 0,
        });
        Ok(())
    }

    pub fn append(
        &mut self,
        transaction: TransactionId,
        chunk: WmV1SnapshotChunk,
    ) -> Result<(), PolicyTransferError> {
        let transfer = self
            .transfer
            .as_mut()
            .ok_or(PolicyTransferError::NoTransfer)?;
        if transaction != transfer.transaction || chunk.connection_epoch != self.connection_epoch {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if usize::from(chunk.ordinal) != transfer.chunks.len() {
            return Err(PolicyTransferError::DuplicateOrReorderedChunk);
        }
        if chunk.item_count == 0 || chunk.data.is_empty() {
            return Err(PolicyTransferError::InvalidCount);
        }
        let next_bytes = transfer
            .bytes
            .checked_add(chunk.data.len())
            .filter(|bytes| *bytes <= POLICY_MAX_TRANSFER_BYTES)
            .ok_or(PolicyTransferError::ExcessiveBytes)?;
        let count = chunk.item_count as usize;
        let ordinary_chunk_count = usize::from(transfer.begin.chunk_count);
        if chunk.record_kind == sophia_protocol::SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND {
            if self.selected_capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS
                == 0
            {
                return Err(PolicyTransferError::UnsupportedCapability);
            }
            let prior: usize = transfer
                .chunks
                .iter()
                .filter(|c| c.record_kind == chunk.record_kind)
                .map(|c| c.item_count as usize)
                .sum();
            if transfer.chunks.len() < ordinary_chunk_count
                || transfer.chunks.len() >= POLICY_MAX_TRANSFER_CHUNKS
                || count > SOPHIA_WM_MAX_OUTPUTS.saturating_sub(prior)
                || chunk.data.len() != count * 24
            {
                return Err(PolicyTransferError::RecordCountMismatch);
            }
            transfer.bytes = next_bytes;
            transfer.chunks.push(chunk);
            return Ok(());
        }
        if chunk.record_kind == sophia_protocol::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND {
            if self.selected_capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN == 0
            {
                return Err(PolicyTransferError::UnsupportedCapability);
            }
            if transfer.chunks.len() < ordinary_chunk_count
                || transfer.chunks.len() >= POLICY_MAX_TRANSFER_CHUNKS
            {
                return Err(PolicyTransferError::DuplicateOrReorderedChunk);
            }
            let prior: usize = transfer
                .chunks
                .iter()
                .filter(|c| c.record_kind == chunk.record_kind)
                .map(|c| c.item_count as usize)
                .sum();
            if count > SOPHIA_WM_MAX_SURFACES.saturating_sub(prior)
                || chunk.data.len() != count * sophia_protocol::LAUNCH_CONTEXT_RECORD_LEN
            {
                return Err(PolicyTransferError::RecordCountMismatch);
            }
            transfer.bytes = next_bytes;
            transfer.chunks.push(chunk);
            return Ok(());
        }
        if chunk.record_kind == SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND {
            if self.selected_capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT == 0 {
                return Err(PolicyTransferError::UnsupportedCapability);
            }
            if transfer.chunks.len() < ordinary_chunk_count {
                return Err(PolicyTransferError::DuplicateOrReorderedChunk);
            }
            let next_total = transfer
                .classification_records
                .checked_add(count)
                .filter(|value| *value <= SOPHIA_WM_MAX_SURFACES)
                .ok_or(PolicyTransferError::RecordCountMismatch)?;
            transfer.classification_records = next_total;
            transfer.bytes = next_bytes;
            transfer.chunks.push(chunk);
            return Ok(());
        }
        if transfer.chunks.len() >= ordinary_chunk_count {
            return Err(PolicyTransferError::RecordCountMismatch);
        }
        let total = match chunk.record_kind {
            SNAPSHOT_OUTPUT_RECORD_KIND => &mut transfer.output_records,
            SNAPSHOT_SURFACE_RECORD_KIND => &mut transfer.surface_records,
            SNAPSHOT_ACTION_RECORD_KIND => &mut transfer.action_records,
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND => &mut transfer.session_operation_records,
            _ => return Err(PolicyTransferError::UnknownRecordKind),
        };
        let maximum = match chunk.record_kind {
            SNAPSHOT_OUTPUT_RECORD_KIND => usize::from(transfer.begin.output_count),
            SNAPSHOT_SURFACE_RECORD_KIND => transfer.begin.surface_count as usize,
            SNAPSHOT_ACTION_RECORD_KIND => usize::from(transfer.begin.action_count),
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND => {
                usize::from(transfer.begin.session_operation_count)
            }
            _ => unreachable!(),
        };
        let next_total = total
            .checked_add(count)
            .filter(|value| *value <= maximum)
            .ok_or(PolicyTransferError::RecordCountMismatch)?;
        *total = next_total;
        transfer.bytes = next_bytes;
        transfer.chunks.push(chunk);
        Ok(())
    }

    pub fn finish(
        &mut self,
        transaction: TransactionId,
        end: WmV1SnapshotEnd,
    ) -> Result<AssembledPolicySnapshot, PolicyTransferError> {
        let transfer = self
            .transfer
            .as_ref()
            .ok_or(PolicyTransferError::NoTransfer)?;
        if transaction != transfer.transaction
            || end.connection_epoch != self.connection_epoch
            || end.scene_generation != transfer.begin.scene_generation
            || end.chunk_count != transfer.begin.chunk_count
        {
            return Err(PolicyTransferError::WrongTransferIdentity);
        }
        if transfer.chunks.len() < usize::from(transfer.begin.chunk_count)
            || transfer.output_records != usize::from(transfer.begin.output_count)
            || transfer.surface_records != transfer.begin.surface_count as usize
            || transfer.action_records != usize::from(transfer.begin.action_count)
            || transfer.session_operation_records
                != usize::from(transfer.begin.session_operation_count)
        {
            return Err(PolicyTransferError::RecordCountMismatch);
        }
        let transfer = self.transfer.take().expect("transfer was checked");
        Ok(AssembledPolicySnapshot {
            transaction: transfer.transaction,
            connection_epoch: transfer.begin.connection_epoch,
            scene_generation: transfer.begin.scene_generation,
            active_output: transfer.begin.active_output,
            output_count: transfer.begin.output_count,
            surface_count: transfer.begin.surface_count,
            action_count: transfer.begin.action_count,
            session_operation_count: transfer.begin.session_operation_count,
            chunk_count: transfer.begin.chunk_count,
            chunks: transfer.chunks,
        })
    }

    pub fn disconnect(&mut self) {
        self.transfer = None;
    }
}

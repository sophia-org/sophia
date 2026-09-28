//! Typed client records from socket frames. Each request scans the inbox by
//! frame kind in arrival order, exactly as the owners once did; a peek leaves
//! the frame queued and the matching take removes that same frame.
use sophia_protocol::{
    CatalogActivation, IpcMessageKind, NativeLauncherActivation, NativeLauncherInputAck,
    SOPHIA_IPC_HEADER_LEN, ShellCatalogActionRecord, ShellContentRecord, ShellIndicatorActivation,
    ShellNativeLauncherRecord, TransactionId, decode_shell_catalog_action_frame,
    decode_shell_content_frame, decode_shell_indicator_activation,
    decode_shell_native_launcher_frame,
};

use super::super::ShellTransportError;
use super::super::content_admission;
use super::super::native_launcher::NativeContentRecord;
use super::super::wire::{CatalogCandidatePart, ContentWant};
use super::{SocketWire, kind};

const NATIVE_INPUT_ACK: u16 = 194;
const NATIVE_ACTIVATE: u16 = 195;
const CATALOG_ACTIVATE: u16 = 200;

fn content_kind(want: ContentWant, kind: u16) -> bool {
    match want {
        ContentWant::Resource => matches!(kind, 165 | 167..=170),
        ContentWant::AllocationRequest => kind == 163,
        ContentWant::Demand => matches!(kind, 176 | 178),
        ContentWant::ActionAck => kind == IpcMessageKind::ShellContentActionAck as u16,
        ContentWant::CandidatePart => matches!(kind, 172..=174),
    }
}

fn native_content_kind(kind: u16) -> bool {
    matches!(
        kind,
        163 | 165 | 167..=170 | 172..=174 | 176 | 178 | 188..=190
    )
}

fn catalog_candidate_kind(kind: u16) -> bool {
    matches!(kind, 172..=174 | 189 | 190 | 198 | 199)
}

impl SocketWire {
    fn position(&self, select: impl Fn(u16) -> bool) -> Option<usize> {
        self.inbox.iter().position(|frame| select(kind(frame)))
    }

    pub(in crate::shell_transport) fn peek_content(
        &self,
        want: ContentWant,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.position(|kind| content_kind(want, kind))
            .map(|at| decode_shell_content_frame(&self.inbox[at]).map_err(Into::into))
            .transpose()
    }

    pub(in crate::shell_transport) fn take_content(
        &mut self,
        want: ContentWant,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.position(|kind| content_kind(want, kind))
            .and_then(|at| self.inbox.remove(at))
            .map(|frame| decode_shell_content_frame(&frame).map_err(Into::into))
            .transpose()
    }

    fn native_launcher_record(
        &self,
        wanted: u16,
    ) -> Result<Option<(TransactionId, ShellNativeLauncherRecord)>, ShellTransportError> {
        self.position(|kind| kind == wanted)
            .map(|at| decode_shell_native_launcher_frame(&self.inbox[at]).map_err(Into::into))
            .transpose()
    }

    fn remove_first(&mut self, wanted: u16) {
        if let Some(at) = self.position(|kind| kind == wanted) {
            self.inbox.remove(at);
        }
    }

    pub(in crate::shell_transport) fn peek_native_input_ack(
        &self,
    ) -> Result<Option<(TransactionId, NativeLauncherInputAck)>, ShellTransportError> {
        match self.native_launcher_record(NATIVE_INPUT_ACK)? {
            None => Ok(None),
            Some((transaction, ShellNativeLauncherRecord::InputAck(ack))) => {
                Ok(Some((transaction, ack)))
            }
            Some(_) => Err(ShellTransportError::WrongContentRecord),
        }
    }

    pub(in crate::shell_transport) fn take_native_input_ack(&mut self) {
        self.remove_first(NATIVE_INPUT_ACK);
    }

    pub(in crate::shell_transport) fn peek_native_activate(
        &self,
    ) -> Result<Option<(TransactionId, NativeLauncherActivation)>, ShellTransportError> {
        match self.native_launcher_record(NATIVE_ACTIVATE)? {
            None => Ok(None),
            Some((transaction, ShellNativeLauncherRecord::Activate(activation))) => {
                Ok(Some((transaction, activation)))
            }
            Some(_) => Err(ShellTransportError::WrongContentRecord),
        }
    }

    pub(in crate::shell_transport) fn take_native_activate(&mut self) {
        self.remove_first(NATIVE_ACTIVATE);
    }

    pub(in crate::shell_transport) fn peek_catalog_activate(
        &self,
    ) -> Result<Option<(TransactionId, CatalogActivation)>, ShellTransportError> {
        let Some(at) = self.position(|kind| kind == CATALOG_ACTIVATE) else {
            return Ok(None);
        };
        let (transaction, record) = decode_shell_catalog_action_frame(&self.inbox[at])?;
        let ShellCatalogActionRecord::Activate(activation) = record else {
            return Err(ShellTransportError::WrongContentRecord);
        };
        Ok(Some((transaction, activation)))
    }

    pub(in crate::shell_transport) fn take_catalog_activate(&mut self) {
        self.remove_first(CATALOG_ACTIVATE);
    }

    pub(in crate::shell_transport) fn peek_indicator_activate(
        &self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        self.position(|kind| kind == IpcMessageKind::ShellIndicatorActivate as u16)
            .map(|at| decode_shell_indicator_activation(&self.inbox[at]).map_err(Into::into))
            .transpose()
    }

    pub(in crate::shell_transport) fn take_indicator_activate(&mut self) {
        self.remove_first(IpcMessageKind::ShellIndicatorActivate as u16);
    }

    /// The payload of the frame at `at`, checked against the advertised bound;
    /// None when it does not fit this visit's remaining allowance.
    fn visit_payload(&self, at: usize) -> Result<Option<usize>, ShellTransportError> {
        let payload = self.inbox[at].len() - SOPHIA_IPC_HEADER_LEN;
        if payload > self.max_frame_payload {
            return Err(ShellTransportError::WrongContentRecord);
        }
        Ok((payload <= self.visit).then_some(payload))
    }

    fn take_visit_frame(&mut self, select: impl Fn(u16) -> bool) {
        if let Some(at) = self.position(select) {
            let payload = self.inbox[at].len() - SOPHIA_IPC_HEADER_LEN;
            self.visit = self.visit.saturating_sub(payload);
            self.inbox.remove(at);
        }
    }

    pub(in crate::shell_transport) fn peek_native_content(
        &self,
    ) -> Result<Option<(TransactionId, NativeContentRecord)>, ShellTransportError> {
        let Some(at) = self.position(native_content_kind) else {
            return Ok(None);
        };
        if self.visit_payload(at)?.is_none() {
            return Ok(None);
        }
        decode_native_content_record(&self.inbox[at]).map(Some)
    }

    pub(in crate::shell_transport) fn take_native_content(&mut self) {
        self.take_visit_frame(native_content_kind);
    }

    pub(in crate::shell_transport) fn peek_catalog_candidate(
        &self,
    ) -> Result<Option<(TransactionId, CatalogCandidatePart)>, ShellTransportError> {
        let Some(at) = self.position(catalog_candidate_kind) else {
            return Ok(None);
        };
        // A different candidate family cannot silently bypass the catalog
        // binding or sit forever as an unserviceable inbox record.
        if !matches!(kind(&self.inbox[at]), 174 | 198 | 199) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if self.visit_payload(at)?.is_none() {
            return Ok(None);
        }
        decode_catalog_candidate(&self.inbox[at]).map(Some)
    }

    pub(in crate::shell_transport) fn take_catalog_candidate(&mut self) {
        self.take_visit_frame(catalog_candidate_kind);
    }
}

fn decode_catalog_candidate(
    frame: &[u8],
) -> Result<(TransactionId, CatalogCandidatePart), ShellTransportError> {
    if kind(frame) == 174 {
        let (transaction, ShellContentRecord::CandidateEnd(end)) =
            decode_shell_content_frame(frame)?
        else {
            return Err(ShellTransportError::WrongContentRecord);
        };
        return Ok((transaction, CatalogCandidatePart::End(end)));
    }
    let (transaction, record) = decode_shell_catalog_action_frame(frame)?;
    Ok((
        transaction,
        match record {
            ShellCatalogActionRecord::CandidateBegin(value) => CatalogCandidatePart::Begin(value),
            ShellCatalogActionRecord::CandidateChunk(value) => CatalogCandidatePart::Chunk(value),
            _ => return Err(ShellTransportError::WrongContentRecord),
        },
    ))
}

fn decode_native_content_record(
    frame: &[u8],
) -> Result<(TransactionId, NativeContentRecord), ShellTransportError> {
    let kind = kind(frame);
    if matches!(kind, 165 | 167..=170 | 174 | 176 | 178) {
        let (transaction, record) = decode_shell_content_frame(frame)?;
        let record = match record {
            ShellContentRecord::CandidateEnd(end) => NativeContentRecord::End(end),
            ShellContentRecord::FrameDemand(value) => NativeContentRecord::Demand(value),
            ShellContentRecord::FrameDemandCancel(value) => NativeContentRecord::Cancel(value),
            value if content_admission::resource_identity(&value).is_some() => {
                NativeContentRecord::Resource(value)
            }
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        Ok((transaction, record))
    } else if (188..=190).contains(&kind) {
        let (transaction, record) = decode_shell_native_launcher_frame(frame)?;
        let record = match record {
            ShellNativeLauncherRecord::AllocationRequest(value) => {
                NativeContentRecord::Allocation(value)
            }
            ShellNativeLauncherRecord::CandidateBegin(value) => NativeContentRecord::Begin(value),
            ShellNativeLauncherRecord::CandidateChunk(value) => NativeContentRecord::Chunk(value),
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        Ok((transaction, record))
    } else {
        Err(ShellTransportError::WrongContentRecord)
    }
}

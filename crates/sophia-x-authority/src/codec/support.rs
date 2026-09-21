use sophia_protocol::{
    AuthorityKind, AuthorityLocalId, BufferSource, IpcCodecError, NamespaceId, PortalDecision,
    PortalTransferId, PortalTransferKind, Rect, Region, SOPHIA_IPC_MAX_ITEMS, Size,
    SurfaceConstraints, SurfaceId, SurfaceTransactionReadiness, TransactionId,
};

use crate::{XAuthorityResponseOutcome, XAuthorityRuntimeError, XResourceId, XSelectionChangeKind};

pub(super) fn encode_response_outcome(outcome: XAuthorityResponseOutcome, out: &mut Vec<u8>) {
    match outcome {
        XAuthorityResponseOutcome::Accepted => {
            push_u16(out, 1);
            push_u16(out, 0);
        }
        XAuthorityResponseOutcome::Rejected(error) => {
            push_u16(out, 2);
            encode_runtime_error(error, out);
        }
    }
}

pub(super) fn decode_response_outcome(
    cursor: &mut Cursor<'_>,
) -> Result<XAuthorityResponseOutcome, IpcCodecError> {
    match cursor.u16()? {
        1 => {
            let reserved = cursor.u16()?;
            if reserved != 0 {
                return Err(IpcCodecError::ReservedNonZero(u32::from(reserved)));
            }
            Ok(XAuthorityResponseOutcome::Accepted)
        }
        2 => Ok(XAuthorityResponseOutcome::Rejected(decode_runtime_error(
            cursor.u16()?,
        )?)),
        other => Err(IpcCodecError::InvalidEnum {
            field: "x_authority_response_outcome",
            value: u32::from(other),
        }),
    }
}

fn encode_runtime_error(error: XAuthorityRuntimeError, out: &mut Vec<u8>) {
    push_u16(
        out,
        match error {
            XAuthorityRuntimeError::InvalidResource => 1,
            XAuthorityRuntimeError::InvalidNamespace => 2,
            XAuthorityRuntimeError::InvalidSurface => 3,
            XAuthorityRuntimeError::UnknownResource => 4,
            XAuthorityRuntimeError::WrongResourceKind => 5,
            XAuthorityRuntimeError::CrossNamespaceDenied => 6,
            XAuthorityRuntimeError::UnknownRequestorNamespace => 7,
            XAuthorityRuntimeError::UnknownSourceOwner => 8,
            XAuthorityRuntimeError::MissingSourceNamespace => 9,
            XAuthorityRuntimeError::SameNamespace => 10,
            XAuthorityRuntimeError::PortalRejected => 11,
            XAuthorityRuntimeError::StaleGeneration => 12,
            XAuthorityRuntimeError::FocusAuthorityUnavailable => 13,
            XAuthorityRuntimeError::InvalidValue => 14,
            XAuthorityRuntimeError::WindowNotViewable => 15,
        },
    );
}

fn decode_runtime_error(value: u16) -> Result<XAuthorityRuntimeError, IpcCodecError> {
    match value {
        1 => Ok(XAuthorityRuntimeError::InvalidResource),
        2 => Ok(XAuthorityRuntimeError::InvalidNamespace),
        3 => Ok(XAuthorityRuntimeError::InvalidSurface),
        4 => Ok(XAuthorityRuntimeError::UnknownResource),
        5 => Ok(XAuthorityRuntimeError::WrongResourceKind),
        6 => Ok(XAuthorityRuntimeError::CrossNamespaceDenied),
        7 => Ok(XAuthorityRuntimeError::UnknownRequestorNamespace),
        8 => Ok(XAuthorityRuntimeError::UnknownSourceOwner),
        9 => Ok(XAuthorityRuntimeError::MissingSourceNamespace),
        10 => Ok(XAuthorityRuntimeError::SameNamespace),
        11 => Ok(XAuthorityRuntimeError::PortalRejected),
        12 => Ok(XAuthorityRuntimeError::StaleGeneration),
        13 => Ok(XAuthorityRuntimeError::FocusAuthorityUnavailable),
        14 => Ok(XAuthorityRuntimeError::InvalidValue),
        15 => Ok(XAuthorityRuntimeError::WindowNotViewable),
        other => Err(IpcCodecError::InvalidEnum {
            field: "x_authority_runtime_error",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_count(count: usize, out: &mut Vec<u8>) -> Result<(), IpcCodecError> {
    if count > SOPHIA_IPC_MAX_ITEMS {
        return Err(IpcCodecError::CountTooLarge {
            count,
            max: SOPHIA_IPC_MAX_ITEMS,
        });
    }
    push_u32(out, count as u32);
    Ok(())
}

pub(super) fn decode_count(cursor: &mut Cursor<'_>) -> Result<usize, IpcCodecError> {
    let count = cursor.u32()? as usize;
    if count > SOPHIA_IPC_MAX_ITEMS {
        return Err(IpcCodecError::CountTooLarge {
            count,
            max: SOPHIA_IPC_MAX_ITEMS,
        });
    }
    Ok(count)
}

pub(super) fn encode_region(region: &Region, out: &mut Vec<u8>) -> Result<(), IpcCodecError> {
    encode_count(region.rects.len(), out)?;
    for rect in &region.rects {
        encode_rect(*rect, out);
    }
    Ok(())
}

pub(super) fn decode_region(cursor: &mut Cursor<'_>) -> Result<Region, IpcCodecError> {
    let count = decode_count(cursor)?;
    let mut rects = Vec::with_capacity(count);
    for _ in 0..count {
        rects.push(decode_rect(cursor)?);
    }
    Ok(Region { rects })
}

pub(super) fn encode_rect(rect: Rect, out: &mut Vec<u8>) {
    push_i32(out, rect.x);
    push_i32(out, rect.y);
    push_i32(out, rect.width);
    push_i32(out, rect.height);
}

pub(super) fn decode_rect(cursor: &mut Cursor<'_>) -> Result<Rect, IpcCodecError> {
    Ok(Rect {
        x: cursor.i32()?,
        y: cursor.i32()?,
        width: cursor.i32()?,
        height: cursor.i32()?,
    })
}

pub(super) fn encode_size(size: Size, out: &mut Vec<u8>) {
    push_i32(out, size.width);
    push_i32(out, size.height);
}

pub(super) fn decode_size(cursor: &mut Cursor<'_>) -> Result<Size, IpcCodecError> {
    Ok(Size {
        width: cursor.i32()?,
        height: cursor.i32()?,
    })
}

pub(super) fn encode_constraints(constraints: SurfaceConstraints, out: &mut Vec<u8>) {
    encode_option_size(constraints.min_size, out);
    encode_option_size(constraints.max_size, out);
}

pub(super) fn decode_constraints(
    cursor: &mut Cursor<'_>,
) -> Result<SurfaceConstraints, IpcCodecError> {
    Ok(SurfaceConstraints {
        min_size: decode_option_size(cursor)?,
        max_size: decode_option_size(cursor)?,
    })
}

fn encode_option_size(size: Option<Size>, out: &mut Vec<u8>) {
    match size {
        Some(size) => {
            push_u8(out, 1);
            push_i32(out, size.width);
            push_i32(out, size.height);
        }
        None => push_u8(out, 0),
    }
}

fn decode_option_size(cursor: &mut Cursor<'_>) -> Result<Option<Size>, IpcCodecError> {
    match cursor.u8()? {
        0 => Ok(None),
        1 => Ok(Some(Size {
            width: cursor.i32()?,
            height: cursor.i32()?,
        })),
        other => Err(IpcCodecError::InvalidBool {
            field: "option_size",
            value: other,
        }),
    }
}

pub(super) fn encode_buffer_source(source: BufferSource, out: &mut Vec<u8>) {
    match source {
        BufferSource::None => push_u16(out, 0),
        BufferSource::XPixmap { pixmap } => {
            push_u16(out, 1);
            push_u32(out, pixmap);
        }
        BufferSource::DmaBuf { handle } => {
            push_u16(out, 2);
            push_u64(out, handle);
        }
        BufferSource::CpuBuffer { handle } => {
            push_u16(out, 3);
            push_u64(out, handle);
        }
    }
}

pub(super) fn decode_buffer_source(cursor: &mut Cursor<'_>) -> Result<BufferSource, IpcCodecError> {
    match cursor.u16()? {
        0 => Ok(BufferSource::None),
        1 => Ok(BufferSource::XPixmap {
            pixmap: cursor.u32()?,
        }),
        2 => Ok(BufferSource::DmaBuf {
            handle: cursor.u64()?,
        }),
        3 => Ok(BufferSource::CpuBuffer {
            handle: cursor.u64()?,
        }),
        other => Err(IpcCodecError::InvalidEnum {
            field: "buffer_source",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_authority_kind(kind: AuthorityKind, out: &mut Vec<u8>) {
    push_u16(
        out,
        match kind {
            AuthorityKind::SophiaX => 1,
            AuthorityKind::SophiaNative => 3,
        },
    );
}

pub(super) fn decode_authority_kind(value: u16) -> Result<AuthorityKind, IpcCodecError> {
    match value {
        1 => Ok(AuthorityKind::SophiaX),
        3 => Ok(AuthorityKind::SophiaNative),
        other => Err(IpcCodecError::InvalidEnum {
            field: "authority_kind",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_readiness(readiness: SurfaceTransactionReadiness, out: &mut Vec<u8>) {
    push_u16(
        out,
        match readiness {
            SurfaceTransactionReadiness::Pending => 1,
            SurfaceTransactionReadiness::Ready => 2,
            SurfaceTransactionReadiness::Failed => 3,
            SurfaceTransactionReadiness::TimedOut => 4,
        },
    );
}

pub(super) fn decode_readiness(value: u16) -> Result<SurfaceTransactionReadiness, IpcCodecError> {
    match value {
        1 => Ok(SurfaceTransactionReadiness::Pending),
        2 => Ok(SurfaceTransactionReadiness::Ready),
        3 => Ok(SurfaceTransactionReadiness::Failed),
        4 => Ok(SurfaceTransactionReadiness::TimedOut),
        other => Err(IpcCodecError::InvalidEnum {
            field: "surface_transaction_readiness",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_portal_transfer_kind(kind: PortalTransferKind, out: &mut Vec<u8>) {
    push_u16(
        out,
        match kind {
            PortalTransferKind::Clipboard => 1,
            PortalTransferKind::DragAndDrop => 2,
            PortalTransferKind::FileHandoff => 3,
            PortalTransferKind::ScreenCapture => 4,
            PortalTransferKind::Notification => 5,
            PortalTransferKind::ScreenRecording => 6,
            PortalTransferKind::UriOpen => 7,
        },
    );
}

pub(super) fn decode_portal_transfer_kind(value: u16) -> Result<PortalTransferKind, IpcCodecError> {
    match value {
        1 => Ok(PortalTransferKind::Clipboard),
        2 => Ok(PortalTransferKind::DragAndDrop),
        3 => Ok(PortalTransferKind::FileHandoff),
        4 => Ok(PortalTransferKind::ScreenCapture),
        5 => Ok(PortalTransferKind::Notification),
        6 => Ok(PortalTransferKind::ScreenRecording),
        7 => Ok(PortalTransferKind::UriOpen),
        other => Err(IpcCodecError::InvalidEnum {
            field: "portal_transfer_kind",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_portal_decision(decision: PortalDecision, out: &mut Vec<u8>) {
    push_u16(
        out,
        match decision {
            PortalDecision::Pending => 1,
            PortalDecision::Allowed => 2,
            PortalDecision::Denied => 3,
            PortalDecision::Revoked => 4,
        },
    );
}

pub(super) fn decode_portal_decision(value: u16) -> Result<PortalDecision, IpcCodecError> {
    match value {
        1 => Ok(PortalDecision::Pending),
        2 => Ok(PortalDecision::Allowed),
        3 => Ok(PortalDecision::Denied),
        4 => Ok(PortalDecision::Revoked),
        other => Err(IpcCodecError::InvalidEnum {
            field: "portal_decision",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_selection_change_kind(kind: XSelectionChangeKind) -> u16 {
    match kind {
        XSelectionChangeKind::SetOwner => 1,
        XSelectionChangeKind::ClearOwner => 2,
        XSelectionChangeKind::SelectionWindowDestroyed => 3,
        XSelectionChangeKind::SelectionClientClosed => 4,
        XSelectionChangeKind::Unknown => 5,
    }
}

pub(super) fn decode_selection_change_kind(
    value: u16,
) -> Result<XSelectionChangeKind, IpcCodecError> {
    match value {
        1 => Ok(XSelectionChangeKind::SetOwner),
        2 => Ok(XSelectionChangeKind::ClearOwner),
        3 => Ok(XSelectionChangeKind::SelectionWindowDestroyed),
        4 => Ok(XSelectionChangeKind::SelectionClientClosed),
        5 => Ok(XSelectionChangeKind::Unknown),
        other => Err(IpcCodecError::InvalidEnum {
            field: "x_selection_change_kind",
            value: u32::from(other),
        }),
    }
}

pub(super) fn encode_bool(value: bool, out: &mut Vec<u8>) {
    push_u8(out, u8::from(value));
}

pub(super) fn decode_bool(cursor: &mut Cursor<'_>) -> Result<bool, IpcCodecError> {
    match cursor.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(IpcCodecError::InvalidBool {
            field: "bool",
            value: other,
        }),
    }
}

pub(super) fn encode_optional_text(
    out: &mut Vec<u8>,
    field: &'static str,
    value: Option<&str>,
    max: usize,
) -> Result<(), IpcCodecError> {
    match value {
        Some(value) => {
            push_u8(out, 1);
            encode_text(out, field, value, max)?;
        }
        None => push_u8(out, 0),
    }
    Ok(())
}

pub(super) fn decode_optional_text(
    cursor: &mut Cursor<'_>,
    field: &'static str,
    max: usize,
) -> Result<Option<String>, IpcCodecError> {
    match cursor.u8()? {
        0 => Ok(None),
        1 => decode_text(cursor, field, max).map(Some),
        other => Err(IpcCodecError::InvalidBool {
            field,
            value: other,
        }),
    }
}

pub(super) fn encode_text(
    out: &mut Vec<u8>,
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<(), IpcCodecError> {
    let bytes = value.as_bytes();
    if bytes.len() > max {
        return Err(IpcCodecError::TextTooLarge {
            field,
            len: bytes.len(),
            max,
        });
    }
    push_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn decode_text(
    cursor: &mut Cursor<'_>,
    field: &'static str,
    max: usize,
) -> Result<String, IpcCodecError> {
    let len = cursor.u32()? as usize;
    if len > max {
        return Err(IpcCodecError::TextTooLarge { field, len, max });
    }
    let bytes = cursor.slice(len)?;
    core::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| IpcCodecError::InvalidUtf8 { field })
}

pub(super) fn encode_x_resource_id(id: XResourceId, out: &mut Vec<u8>) {
    encode_authority_local_id(id.local, out);
}

pub(super) fn decode_x_resource_id(cursor: &mut Cursor<'_>) -> Result<XResourceId, IpcCodecError> {
    Ok(XResourceId {
        local: decode_authority_local_id(cursor)?,
    })
}

pub(super) fn encode_optional_x_resource_id(id: Option<XResourceId>, out: &mut Vec<u8>) {
    match id {
        Some(id) => {
            push_u8(out, 1);
            encode_x_resource_id(id, out);
        }
        None => push_u8(out, 0),
    }
}

pub(super) fn decode_optional_x_resource_id(
    cursor: &mut Cursor<'_>,
) -> Result<Option<XResourceId>, IpcCodecError> {
    match cursor.u8()? {
        0 => Ok(None),
        1 => decode_x_resource_id(cursor).map(Some),
        other => Err(IpcCodecError::InvalidBool {
            field: "optional_x_resource_id",
            value: other,
        }),
    }
}

pub(super) fn encode_authority_local_id(id: AuthorityLocalId, out: &mut Vec<u8>) {
    push_u64(out, id.raw());
    push_u32(out, id.generation());
}

pub(super) fn decode_authority_local_id(
    cursor: &mut Cursor<'_>,
) -> Result<AuthorityLocalId, IpcCodecError> {
    Ok(AuthorityLocalId::new(cursor.u64()?, cursor.u32()?))
}

pub(super) fn encode_surface_id(id: SurfaceId, out: &mut Vec<u8>) {
    push_u32(out, id.index());
    push_u32(out, id.generation());
}

pub(super) fn decode_surface_id(cursor: &mut Cursor<'_>) -> Result<SurfaceId, IpcCodecError> {
    Ok(SurfaceId::new(cursor.u32()?, cursor.u32()?))
}

pub(super) fn encode_namespace_id(id: NamespaceId, out: &mut Vec<u8>) {
    push_u64(out, id.raw());
}

pub(super) fn decode_namespace_id(cursor: &mut Cursor<'_>) -> Result<NamespaceId, IpcCodecError> {
    Ok(NamespaceId::from_raw(cursor.u64()?))
}

pub(super) fn encode_optional_namespace_id(id: Option<NamespaceId>, out: &mut Vec<u8>) {
    match id {
        Some(id) => {
            push_u8(out, 1);
            encode_namespace_id(id, out);
        }
        None => push_u8(out, 0),
    }
}

pub(super) fn decode_optional_namespace_id(
    cursor: &mut Cursor<'_>,
) -> Result<Option<NamespaceId>, IpcCodecError> {
    match cursor.u8()? {
        0 => Ok(None),
        1 => decode_namespace_id(cursor).map(Some),
        other => Err(IpcCodecError::InvalidBool {
            field: "optional_namespace_id",
            value: other,
        }),
    }
}

pub(super) fn encode_transaction_id(id: TransactionId, out: &mut Vec<u8>) {
    push_u64(out, id.raw());
}

pub(super) fn decode_transaction_id(
    cursor: &mut Cursor<'_>,
) -> Result<TransactionId, IpcCodecError> {
    Ok(TransactionId::from_raw(cursor.u64()?))
}

pub(super) fn encode_portal_transfer_id(id: PortalTransferId, out: &mut Vec<u8>) {
    push_u64(out, id.raw());
}

pub(super) fn decode_portal_transfer_id(
    cursor: &mut Cursor<'_>,
) -> Result<PortalTransferId, IpcCodecError> {
    Ok(PortalTransferId::from_raw(cursor.u64()?))
}

pub(super) fn push_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

pub(super) fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(super) fn finish(&self) -> Result<(), IpcCodecError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining == 0 {
            Ok(())
        } else {
            Err(IpcCodecError::TrailingBytes(remaining))
        }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], IpcCodecError> {
        let end = self.offset.checked_add(N).ok_or(IpcCodecError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(IpcCodecError::Truncated)?;
        self.offset = end;
        let mut out = [0; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    pub(super) fn slice(&mut self, len: usize) -> Result<&'a [u8], IpcCodecError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(IpcCodecError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(IpcCodecError::Truncated)?;
        self.offset = end;
        Ok(slice)
    }

    pub(super) fn u8(&mut self) -> Result<u8, IpcCodecError> {
        Ok(self.take::<1>()?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, IpcCodecError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    pub(super) fn u32(&mut self) -> Result<u32, IpcCodecError> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    pub(super) fn u64(&mut self) -> Result<u64, IpcCodecError> {
        Ok(u64::from_le_bytes(self.take()?))
    }

    pub(super) fn i32(&mut self) -> Result<i32, IpcCodecError> {
        Ok(i32::from_le_bytes(self.take()?))
    }
}

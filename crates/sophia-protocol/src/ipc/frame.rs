use crate::TransactionId;

use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use super::types::{
    IpcCodecError, IpcFrameHeader, IpcMessageKind, SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAGIC,
    SOPHIA_IPC_MAX_PAYLOAD_LEN, SOPHIA_IPC_VERSION,
};

pub fn encode_frame(
    message_kind: IpcMessageKind,
    transaction: TransactionId,
    payload: &[u8],
) -> Result<Vec<u8>, IpcCodecError> {
    if payload.len() > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(IpcCodecError::PayloadTooLarge(payload.len()));
    }

    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload.len());
    push_u32(&mut frame, SOPHIA_IPC_MAGIC);
    push_u16(&mut frame, SOPHIA_IPC_VERSION);
    push_u16(&mut frame, message_kind as u16);
    push_u64(&mut frame, transaction.raw());
    push_u32(&mut frame, payload.len() as u32);
    push_u32(&mut frame, 0);
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub fn decode_frame(frame: &[u8]) -> Result<(IpcFrameHeader, &[u8]), IpcCodecError> {
    if frame.len() < SOPHIA_IPC_HEADER_LEN {
        return Err(IpcCodecError::Truncated);
    }

    let mut cursor = Cursor::new(frame);
    let magic = cursor.u32()?;
    if magic != SOPHIA_IPC_MAGIC {
        return Err(IpcCodecError::BadMagic);
    }

    let version = cursor.u16()?;
    if version != SOPHIA_IPC_VERSION {
        return Err(IpcCodecError::UnsupportedVersion(version));
    }

    let message_kind = match cursor.u16()? {
        3 => IpcMessageKind::BrokerHealth,
        4 => IpcMessageKind::XAuthorityRequest,
        5 => IpcMessageKind::XAuthorityResponse,
        6 => IpcMessageKind::PortalBrokerRequest,
        7 => IpcMessageKind::PortalBrokerResponse,
        8 => IpcMessageKind::PortalClipboardPayload,
        32 => IpcMessageKind::WmV1ClientHello,
        33 => IpcMessageKind::WmV1ServerWelcome,
        34 => IpcMessageKind::WmV1SnapshotBegin,
        35 => IpcMessageKind::WmV1SnapshotChunk,
        36 => IpcMessageKind::WmV1SnapshotEnd,
        37 => IpcMessageKind::WmV1ProjectionRequest,
        38 => IpcMessageKind::WmV1ProjectionBegin,
        39 => IpcMessageKind::WmV1ProjectionChunk,
        40 => IpcMessageKind::WmV1ProjectionEnd,
        41 => IpcMessageKind::WmV1ProjectionOutcome,
        42 => IpcMessageKind::WmV1PolicyConfiguration,
        43 => IpcMessageKind::WmV1PolicyConfigurationOutcome,
        44 => IpcMessageKind::WmV1PolicyDirty,
        45 => IpcMessageKind::WmV1SessionOperationRequest,
        46 => IpcMessageKind::WmV1SessionOperationOutcome,
        47 => IpcMessageKind::WmV1ProfilePrepare,
        48 => IpcMessageKind::WmV1ProfilePrepared,
        49 => IpcMessageKind::WmV1ProfileActivate,
        50 => IpcMessageKind::WmV1ProfileActive,
        51 => IpcMessageKind::WmV1ProfileRollback,
        52 => IpcMessageKind::WmV1ProfileRolledBack,
        53 => IpcMessageKind::WmV1OutputActionRequest,
        64 => IpcMessageKind::OutputV1ClientHello,
        65 => IpcMessageKind::OutputV1ServerWelcome,
        66 => IpcMessageKind::OutputV1Snapshot,
        67 => IpcMessageKind::OutputV1Proposal,
        68 => IpcMessageKind::OutputV1Outcome,
        80 => IpcMessageKind::BrokerV1ClientHello,
        81 => IpcMessageKind::BrokerV1ServerWelcome,
        82 => IpcMessageKind::BrokerV1Request,
        83 => IpcMessageKind::BrokerV1Response,
        96 => IpcMessageKind::ShellV1ClientHello,
        97 => IpcMessageKind::ShellV1ServerWelcome,
        98 => IpcMessageKind::ShellV1DescriptorSnapshot,
        99 => IpcMessageKind::ShellV1Candidate,
        100 => IpcMessageKind::ShellV1CandidateOutcome,
        101 => IpcMessageKind::ShellV1Activation,
        102 => IpcMessageKind::ShellV1ActivationAck,
        103 => IpcMessageKind::ShellTabsBegin,
        104 => IpcMessageKind::ShellTabsGroup,
        105 => IpcMessageKind::ShellTabsEntry,
        106 => IpcMessageKind::ShellTabsEnd,
        107 => IpcMessageKind::ShellTabsCandidate,
        108 => IpcMessageKind::ShellShortcutsBegin,
        109 => IpcMessageKind::ShellShortcutsEntry,
        110 => IpcMessageKind::ShellShortcutsEnd,
        111 => IpcMessageKind::ShellReferenceRequest,
        112 => IpcMessageKind::ShellReferenceCandidate,
        113 => IpcMessageKind::ShellReferenceOutcome,
        114 => IpcMessageKind::ShellApplicationsBegin,
        115 => IpcMessageKind::ShellApplicationsEntry,
        116 => IpcMessageKind::ShellApplicationsEnd,
        117 => IpcMessageKind::ShellLauncherRequest,
        118 => IpcMessageKind::ShellLauncherCandidate,
        119 => IpcMessageKind::ShellLauncherOutcome,
        120 => IpcMessageKind::ShellLauncherActivation,
        121 => IpcMessageKind::ShellLauncherActivationAck,
        122 => IpcMessageKind::ShellLaunchOutcome,
        160 => IpcMessageKind::ShellContentAdmissionRefused,
        161 => IpcMessageKind::ShellContentLimits,
        162 => IpcMessageKind::ShellContentOutputFacts,
        163 => IpcMessageKind::ShellContentAllocationRequest,
        164 => IpcMessageKind::ShellContentAllocationResult,
        165 => IpcMessageKind::ShellContentResourceBegin,
        166 => IpcMessageKind::ShellContentResourceStatus,
        167 => IpcMessageKind::ShellContentResourceChunk,
        168 => IpcMessageKind::ShellContentResourceEnd,
        169 => IpcMessageKind::ShellContentResourceCancel,
        170 => IpcMessageKind::ShellContentResourceRetire,
        171 => IpcMessageKind::ShellContentResourceReleased,
        172 => IpcMessageKind::ShellContentCandidateBegin,
        173 => IpcMessageKind::ShellContentCandidateChunk,
        174 => IpcMessageKind::ShellContentCandidateEnd,
        175 => IpcMessageKind::ShellContentCandidateOutcome,
        176 => IpcMessageKind::ShellContentFrameDemand,
        177 => IpcMessageKind::ShellContentFramePermit,
        178 => IpcMessageKind::ShellContentFrameDemandCancel,
        179 => IpcMessageKind::ShellContentAction,
        180 => IpcMessageKind::ShellContentActionAck,
        181 => IpcMessageKind::ShellIndicatorsBegin,
        182 => IpcMessageKind::ShellIndicatorsOutputStatus,
        183 => IpcMessageKind::ShellIndicatorsEntry,
        184 => IpcMessageKind::ShellIndicatorsEnd,
        185 => IpcMessageKind::ShellIndicatorActivate,
        186 => IpcMessageKind::ShellIndicatorActivateOutcome,
        187 => IpcMessageKind::ShellNativeLauncherOpening,
        188 => IpcMessageKind::ShellNativeLauncherAllocationRequest,
        189 => IpcMessageKind::ShellNativeLauncherCandidateBegin,
        190 => IpcMessageKind::ShellNativeLauncherCandidateChunk,
        191 => IpcMessageKind::ShellNativeLauncherFocus,
        192 => IpcMessageKind::ShellNativeLauncherFocusRevoked,
        193 => IpcMessageKind::ShellNativeLauncherInput,
        194 => IpcMessageKind::ShellNativeLauncherInputAck,
        195 => IpcMessageKind::ShellNativeLauncherActivate,
        196 => IpcMessageKind::ShellNativeLauncherActivationOutcome,
        197 => IpcMessageKind::ShellNativeLauncherClosed,
        198 => IpcMessageKind::ShellCatalogCandidateBegin,
        199 => IpcMessageKind::ShellCatalogCandidateChunk,
        200 => IpcMessageKind::ShellCatalogActivate,
        201 => IpcMessageKind::ShellCatalogActivationOutcome,
        202 => IpcMessageKind::ShellCatalogIdentity,

        other => return Err(IpcCodecError::UnknownMessageKind(other)),
    };
    let transaction = TransactionId::from_raw(cursor.u64()?);
    let payload_len = cursor.u32()?;
    let reserved = cursor.u32()?;
    if reserved != 0 {
        return Err(IpcCodecError::ReservedNonZero(reserved));
    }

    let payload_len_usize = payload_len as usize;
    if payload_len_usize > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(IpcCodecError::PayloadTooLarge(payload_len_usize));
    }
    let expected_len = SOPHIA_IPC_HEADER_LEN
        .checked_add(payload_len_usize)
        .ok_or(IpcCodecError::PayloadTooLarge(payload_len_usize))?;
    if frame.len() < expected_len {
        return Err(IpcCodecError::Truncated);
    }
    if frame.len() > expected_len {
        return Err(IpcCodecError::TrailingBytes(frame.len() - expected_len));
    }

    Ok((
        IpcFrameHeader {
            message_kind,
            transaction,
            payload_len,
        },
        &frame[SOPHIA_IPC_HEADER_LEN..expected_len],
    ))
}

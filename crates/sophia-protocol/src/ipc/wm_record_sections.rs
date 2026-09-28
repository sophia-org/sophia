//! Compatibility framing around complete record arrays. Only this layer
//! assigns legacy chunk sizes, ordinals and connection envelopes; the rows
//! and their semantics belong to `crate::wm_records`.
use super::{
    IpcCodecError, SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN, WmV1ProjectionChunk, WmV1SnapshotChunk,
    WmV1SnapshotTransfer,
};
use crate::wm_records::{
    PROJECTION_LAUNCH_CONTEXT_RECORD_KIND, PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
    PROJECTION_TAB_GROUP_RECORD_KIND, PROJECTION_TAB_GROUP_RECORD_LEN,
    PROJECTION_TAB_MEMBER_RECORD_KIND, PROJECTION_TAB_MEMBER_RECORD_LEN,
    PROJECTION_TRANSLATION_GROUP_RECORD_KIND, PROJECTION_TRANSLATION_GROUP_RECORD_LEN,
    PROJECTION_TRANSLATION_MEMBER_RECORD_KIND, PROJECTION_TRANSLATION_MEMBER_RECORD_LEN,
    PolicyRecordSection, PolicyRecordSectionRef, SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
    decode_policy_launch_contexts_records, decode_policy_output_launch_contexts_records,
    decode_policy_presentation_records, decode_policy_tab_groups_records,
    decode_policy_translation_groups_records, encode_policy_launch_contexts_records,
    encode_policy_output_launch_contexts_records, encode_policy_presentation_records,
    encode_policy_tab_groups_records, encode_policy_translation_groups_records,
    encode_wm_launch_context_records, wm_presentation_record_layout,
};
use crate::{
    PolicyLaunchContext, PolicyOutputLaunchContext, PolicyPresentation, PolicyTabGroup,
    PolicyTranslationGroup,
};

// Each extension kept its own error label on the legacy wire.
fn invalid(field: &'static str) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value: 0 }
}

pub(super) fn projection_sections(
    chunks: &[WmV1ProjectionChunk],
) -> Vec<PolicyRecordSectionRef<'_>> {
    chunks
        .iter()
        .map(|chunk| PolicyRecordSectionRef {
            kind: chunk.record_kind,
            count: chunk.item_count,
            bytes: &chunk.data,
        })
        .collect()
}

pub(super) fn projection_chunks(
    sections: Vec<PolicyRecordSection>,
    epoch: u64,
    ordinal: u16,
    layout: impl Fn(u16) -> Option<usize>,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    let invalid = || IpcCodecError::InvalidEnum {
        field: "projection_section",
        value: 0,
    };
    let mut chunks = Vec::new();
    for section in sections {
        let size = layout(section.kind).ok_or_else(invalid)?;
        if size == 0 || size > 65520 {
            return Err(invalid());
        }
        for bytes in section.bytes.chunks((65520 / size) * size) {
            chunks.push(WmV1ProjectionChunk {
                connection_epoch: epoch,
                ordinal: ordinal
                    .checked_add(u16::try_from(chunks.len()).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?,
                record_kind: section.kind,
                item_count: u32::try_from(bytes.len() / size).map_err(|_| invalid())?,
                data: bytes.to_vec(),
            });
        }
    }
    Ok(chunks)
}

pub fn encode_wm_tab_groups(
    groups: &[PolicyTabGroup],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    projection_chunks(
        encode_policy_tab_groups_records(groups)?,
        epoch,
        ordinal,
        |kind| match kind {
            PROJECTION_TAB_GROUP_RECORD_KIND => Some(PROJECTION_TAB_GROUP_RECORD_LEN),
            PROJECTION_TAB_MEMBER_RECORD_KIND => Some(PROJECTION_TAB_MEMBER_RECORD_LEN),
            _ => None,
        },
    )
    .map_err(|_| invalid("tab_group"))
}

pub fn decode_wm_tab_groups(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyTabGroup>, IpcCodecError> {
    decode_policy_tab_groups_records(&projection_sections(chunks))
}

pub fn encode_wm_translation_groups(
    groups: &[PolicyTranslationGroup],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    projection_chunks(
        encode_policy_translation_groups_records(groups)?,
        epoch,
        ordinal,
        |kind| match kind {
            PROJECTION_TRANSLATION_GROUP_RECORD_KIND => {
                Some(PROJECTION_TRANSLATION_GROUP_RECORD_LEN)
            }
            PROJECTION_TRANSLATION_MEMBER_RECORD_KIND => {
                Some(PROJECTION_TRANSLATION_MEMBER_RECORD_LEN)
            }
            _ => None,
        },
    )
    .map_err(|_| invalid("translation_group"))
}

pub fn decode_wm_translation_groups(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyTranslationGroup>, IpcCodecError> {
    decode_policy_translation_groups_records(&projection_sections(chunks))
}

pub fn encode_wm_presentation(
    presentation: Option<&PolicyPresentation>,
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    projection_chunks(
        encode_policy_presentation_records(presentation, epoch)?,
        epoch,
        ordinal,
        |kind| wm_presentation_record_layout(kind).map(|r| r.0),
    )
    .map_err(|_| invalid("wm_presentation"))
}

pub fn decode_wm_presentation(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Option<PolicyPresentation>, IpcCodecError> {
    decode_policy_presentation_records(&projection_sections(chunks))
}

pub fn encode_wm_launch_contexts(
    records: &[PolicyLaunchContext],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    Ok(encode_policy_launch_contexts_records(records, epoch)?
        .into_iter()
        .map(|section| WmV1ProjectionChunk {
            connection_epoch: epoch,
            ordinal,
            record_kind: section.kind,
            item_count: section.count,
            data: section.bytes,
        })
        .collect())
}

pub fn decode_wm_launch_contexts(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyLaunchContext>, IpcCodecError> {
    let mut records = Vec::new();
    for chunk in chunks
        .iter()
        .filter(|c| c.record_kind == PROJECTION_LAUNCH_CONTEXT_RECORD_KIND)
    {
        records.extend(decode_policy_launch_contexts_records(
            chunk.connection_epoch,
            &[PolicyRecordSectionRef {
                kind: chunk.record_kind,
                count: chunk.item_count,
                bytes: &chunk.data,
            }],
        )?);
    }
    encode_wm_launch_context_records(&records)?;
    Ok(records)
}

/// Preserve the frozen counted prefix; origin records are only sent to a peer
/// which selected the capability. A previous-epoch bookmark is not replayed.
pub fn append_wm_launch_origins(
    transfer: &mut WmV1SnapshotTransfer,
    records: &[PolicyLaunchContext],
    capabilities: u64,
) -> Result<(), IpcCodecError> {
    if capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN == 0 || records.is_empty() {
        return Ok(());
    }
    if records
        .iter()
        .any(|r| r.epoch != transfer.begin.connection_epoch)
    {
        return Err(invalid("launch_origin"));
    }
    transfer.chunks.push(WmV1SnapshotChunk {
        connection_epoch: transfer.begin.connection_epoch,
        ordinal: u16::try_from(transfer.chunks.len()).map_err(|_| invalid("launch_origin"))?,
        record_kind: SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
        item_count: records.len() as u32,
        data: encode_wm_launch_context_records(records)?,
    });
    Ok(())
}

pub fn encode_wm_output_launch_contexts(
    records: &[PolicyOutputLaunchContext],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    Ok(
        encode_policy_output_launch_contexts_records(records, epoch)?
            .into_iter()
            .map(|s| WmV1ProjectionChunk {
                connection_epoch: epoch,
                ordinal,
                record_kind: s.kind,
                item_count: s.count,
                data: s.bytes,
            })
            .collect(),
    )
}

pub fn decode_wm_output_launch_contexts(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyOutputLaunchContext>, IpcCodecError> {
    let mut epoch = None;
    for c in chunks
        .iter()
        .filter(|c| c.record_kind == PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND)
    {
        if epoch.is_some_and(|e| e != c.connection_epoch) {
            return Err(invalid("output_launch_context"));
        }
        epoch = Some(c.connection_epoch);
    }
    decode_policy_output_launch_contexts_records(epoch.unwrap_or(0), &projection_sections(chunks))
}

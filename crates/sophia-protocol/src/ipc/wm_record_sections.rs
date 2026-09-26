//! Compatibility framing around complete record arrays. Only this layer
//! assigns legacy chunk sizes, ordinals and connection envelopes.
use super::{IpcCodecError, PolicyRecordSection, PolicyRecordSectionRef, WmV1ProjectionChunk};

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

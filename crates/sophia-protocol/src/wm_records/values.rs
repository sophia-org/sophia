//! Field encodings shared by several WM record rows. Zero pairs mean absence;
//! every other value must be a complete, valid domain value.
use crate::{BinaryCodecError, PolicyPresentationState, Size, SurfaceId};

use super::PolicyRecordSection;

const POLICY_PRESENTATION_FULLSCREEN: u16 = 1 << 0;
const POLICY_PRESENTATION_MAXIMIZED: u16 = 1 << 1;
const POLICY_PRESENTATION_MINIMIZED: u16 = 1 << 2;
const POLICY_PRESENTATION_SUPPORTED: u16 =
    POLICY_PRESENTATION_FULLSCREEN | POLICY_PRESENTATION_MAXIMIZED | POLICY_PRESENTATION_MINIMIZED;

pub(super) fn push_policy_section(
    sections: &mut Vec<PolicyRecordSection>,
    record_kind: u16,
    count: usize,
    data: Vec<u8>,
) -> Result<(), BinaryCodecError> {
    if count == 0 {
        return Ok(());
    }
    sections.push(PolicyRecordSection {
        kind: record_kind,
        count: u32::try_from(count).map_err(|_| BinaryCodecError::CountTooLarge {
            count,
            max: u32::MAX as usize,
        })?,
        bytes: data,
    });
    Ok(())
}

pub(super) fn encode_presentation(state: PolicyPresentationState) -> u16 {
    (u16::from(state.fullscreen) * POLICY_PRESENTATION_FULLSCREEN)
        | (u16::from(state.maximized) * POLICY_PRESENTATION_MAXIMIZED)
        | (u16::from(state.minimized) * POLICY_PRESENTATION_MINIMIZED)
}

pub(super) fn decode_presentation(
    bits: u16,
    field: &'static str,
) -> Result<PolicyPresentationState, BinaryCodecError> {
    if bits & !POLICY_PRESENTATION_SUPPORTED != 0 {
        return Err(invalid(field, u32::from(bits)));
    }
    Ok(PolicyPresentationState {
        fullscreen: bits & POLICY_PRESENTATION_FULLSCREEN != 0,
        maximized: bits & POLICY_PRESENTATION_MAXIMIZED != 0,
        minimized: bits & POLICY_PRESENTATION_MINIMIZED != 0,
    })
}

pub(super) fn encode_optional_size(size: Option<Size>) -> (i32, i32) {
    size.map(|size| (size.width, size.height)).unwrap_or((0, 0))
}

pub(super) fn decode_optional_size(
    width: i32,
    height: i32,
    field: &'static str,
) -> Result<Option<Size>, BinaryCodecError> {
    if width == 0 && height == 0 {
        Ok(None)
    } else if width > 0 && height > 0 {
        Ok(Some(Size { width, height }))
    } else {
        Err(invalid(field, 0))
    }
}

/// A zero pair is absence; any nonzero generation names a surface. Index
/// validity is left to the caller, as it always was.
pub(crate) fn decode_optional_surface(
    index: u32,
    generation: u32,
    field: &'static str,
) -> Result<Option<SurfaceId>, BinaryCodecError> {
    if index == 0 && generation == 0 {
        Ok(None)
    } else if generation != 0 {
        Ok(Some(SurfaceId::new(index, generation)))
    } else {
        Err(invalid(field, index))
    }
}

pub(crate) fn require_count(actual: usize, expected: usize) -> Result<(), BinaryCodecError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid("record_count", actual as u32))
    }
}

pub(super) fn invalid(field: &'static str, value: u32) -> BinaryCodecError {
    BinaryCodecError::InvalidEnum { field, value }
}

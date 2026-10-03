//! The lock object, negotiation and entry bodies of
//! `protocol/sophia-lock-files-v1.kdl`, with their declared byte constraints.
//! Decoding refuses a malformed body; an encoder refuses to produce one.
//! Semantic checks (stale epochs, unknown allocations, reserved chords,
//! budgets) belong to the export.

use super::{LockFileClass, LockFileKind, invalid, nonzero, range, reserved, zero_u16};
use crate::BinaryCodecError;
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

pub const LOCK_FILE_MAX_OUTPUTS: usize = 16;
pub const LOCK_FILE_MAX_CHORDS: usize = 8;
pub const LOCK_FILE_MAX_PIXELS_PER_SIDE: u32 = 16_384;
pub const LOCK_FILE_MAX_RESOURCE_BYTES: u64 = 1 << 30;
pub const LOCK_FILE_CAPABILITY_PRESENT: u64 = 1 << 0;
pub const LOCK_FILE_CAPABILITY_CHORDS: u64 = 1 << 1;
/// Shift, control, alt and super.
pub const LOCK_FILE_MODIFIER_MASK: u16 = 0b1111;
const SHIFT: u16 = 1;

/// One object a connection epoch's limits fix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFileLimits {
    pub max_outputs: u16,
    pub upload_slots: u16,
    pub max_chords: u16,
    pub max_width_px: u32,
    pub max_height_px: u32,
    pub max_resource_bytes: u64,
    pub max_live_resources: u16,
    pub journal_records: u32,
    pub journal_bytes: u32,
    pub assembly_timeout_ms: u32,
    pub ack_progress_timeout_ms: u32,
}

impl LockFileLimits {
    pub const BYTES: usize = 48;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        range(
            u64::from(self.max_outputs),
            1,
            LOCK_FILE_MAX_OUTPUTS as u64,
            "max_outputs",
        )?;
        range(u64::from(self.upload_slots), 1, 4, "upload_slots")?;
        range(
            u64::from(self.max_chords),
            0,
            LOCK_FILE_MAX_CHORDS as u64,
            "max_chords",
        )?;
        range(
            u64::from(self.max_width_px),
            1,
            u64::from(LOCK_FILE_MAX_PIXELS_PER_SIDE),
            "max_width_px",
        )?;
        range(
            u64::from(self.max_height_px),
            1,
            u64::from(LOCK_FILE_MAX_PIXELS_PER_SIDE),
            "max_height_px",
        )?;
        range(
            self.max_resource_bytes,
            4,
            LOCK_FILE_MAX_RESOURCE_BYTES,
            "max_resource_bytes",
        )?;
        range(
            u64::from(self.max_live_resources),
            2,
            32,
            "max_live_resources",
        )?;
        range(u64::from(self.journal_records), 8, 256, "journal_records")?;
        range(
            u64::from(self.journal_bytes),
            2_048,
            65_536,
            "journal_bytes",
        )?;
        range(
            u64::from(self.assembly_timeout_ms),
            1,
            12_000,
            "assembly_timeout_ms",
        )?;
        range(
            u64::from(self.ack_progress_timeout_ms),
            1,
            2_000,
            "ack_progress_timeout_ms",
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(Self::BYTES);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, self.max_outputs);
        push_u16(&mut bytes, self.upload_slots);
        push_u16(&mut bytes, self.max_chords);
        push_u32(&mut bytes, self.max_width_px);
        push_u32(&mut bytes, self.max_height_px);
        push_u64(&mut bytes, self.max_resource_bytes);
        push_u16(&mut bytes, self.max_live_resources);
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, self.journal_records);
        push_u32(&mut bytes, self.journal_bytes);
        push_u32(&mut bytes, self.assembly_timeout_ms);
        push_u32(&mut bytes, self.ack_progress_timeout_ms);
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.u16()? != 1 {
            return Err(invalid("interface_revision"));
        }
        let mut limits = Self {
            max_outputs: cursor.u16()?,
            upload_slots: cursor.u16()?,
            max_chords: cursor.u16()?,
            max_width_px: cursor.u32()?,
            max_height_px: cursor.u32()?,
            max_resource_bytes: cursor.u64()?,
            max_live_resources: cursor.u16()?,
            journal_records: 0,
            journal_bytes: 0,
            assembly_timeout_ms: 0,
            ack_progress_timeout_ms: 0,
        };
        if cursor.u16()? != 1 {
            return Err(invalid("pixel_format_mask"));
        }
        limits.journal_records = cursor.u32()?;
        limits.journal_bytes = cursor.u32()?;
        limits.assembly_timeout_ms = cursor.u32()?;
        limits.ack_progress_timeout_ms = cursor.u32()?;
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        limits.validate()?;
        Ok(limits)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum LockPhase {
    Unlocked = 1,
    Locking = 2,
    Locked = 3,
    Unlocking = 4,
}

impl LockPhase {
    fn decode(value: u16) -> Result<Self, BinaryCodecError> {
        Ok(match value {
            1 => Self::Unlocked,
            2 => Self::Locking,
            3 => Self::Locked,
            4 => Self::Unlocking,
            _ => return Err(invalid("phase")),
        })
    }

    /// Whether this phase draws the cover and grants allocations.
    pub const fn covers(self) -> bool {
        matches!(self, Self::Locking | Self::Locked)
    }
}

/// One whole-output allocation for the current lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockAllocation {
    pub output_id: u64,
    pub output_generation: u64,
    pub allocation_id: u64,
    pub allocation_generation: u64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_numerator: u32,
    pub scale_denominator: u32,
}

impl LockAllocation {
    pub const BYTES: usize = 56;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.output_id, "output_id")?;
        nonzero(self.output_generation, "output_generation")?;
        nonzero(self.allocation_id, "allocation_id")?;
        nonzero(self.allocation_generation, "allocation_generation")?;
        range(
            u64::from(self.pixel_width),
            1,
            u64::from(LOCK_FILE_MAX_PIXELS_PER_SIDE),
            "pixel_width",
        )?;
        range(
            u64::from(self.pixel_height),
            1,
            u64::from(LOCK_FILE_MAX_PIXELS_PER_SIDE),
            "pixel_height",
        )?;
        range(u64::from(self.scale_numerator), 1, 32, "scale_numerator")?;
        range(u64::from(self.scale_denominator), 1, 4, "scale_denominator")
    }

    fn push(&self, bytes: &mut Vec<u8>) {
        push_u64(bytes, self.output_id);
        push_u64(bytes, self.output_generation);
        push_u64(bytes, self.allocation_id);
        push_u64(bytes, self.allocation_generation);
        push_u32(bytes, self.pixel_width);
        push_u32(bytes, self.pixel_height);
        push_u32(bytes, self.scale_numerator);
        push_u32(bytes, self.scale_denominator);
        push_u64(bytes, 0);
    }

    fn read(cursor: &mut Cursor<'_>) -> Result<Self, BinaryCodecError> {
        let allocation = Self {
            output_id: cursor.u64()?,
            output_generation: cursor.u64()?,
            allocation_id: cursor.u64()?,
            allocation_generation: cursor.u64()?,
            pixel_width: cursor.u32()?,
            pixel_height: cursor.u32()?,
            scale_numerator: cursor.u32()?,
            scale_denominator: cursor.u32()?,
        };
        reserved(cursor, 8)?;
        allocation.validate()?;
        Ok(allocation)
    }
}

/// The `lock` object: the phase, the lock epoch and its allocations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockObject {
    /// Zero exactly when unlocked.
    pub lock_epoch: u64,
    pub topology_generation: u64,
    pub phase: LockPhase,
    pub allocations: Vec<LockAllocation>,
}

impl LockObject {
    const PREFIX: usize = 32;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.topology_generation, "topology_generation")?;
        if self.allocations.len() > LOCK_FILE_MAX_OUTPUTS {
            return Err(invalid("allocation_count"));
        }
        // Allocations exist only while a lock covers the outputs, and a
        // covering phase always names its lock.
        // A zero lock epoch is exactly the unlocked phase.
        if (self.phase == LockPhase::Unlocked) != (self.lock_epoch == 0) {
            return Err(invalid("lock_epoch"));
        }
        if !self.phase.covers() && !self.allocations.is_empty() {
            return Err(invalid("allocation_count"));
        }
        self.allocations
            .iter()
            .try_for_each(LockAllocation::validate)
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes =
            Vec::with_capacity(Self::PREFIX + self.allocations.len() * LockAllocation::BYTES);
        push_u64(&mut bytes, self.lock_epoch);
        push_u64(&mut bytes, self.topology_generation);
        push_u16(&mut bytes, self.phase as u16);
        push_u16(&mut bytes, self.allocations.len() as u16);
        bytes.extend_from_slice(&[0; 12]);
        for allocation in &self.allocations {
            allocation.push(&mut bytes);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let lock_epoch = cursor.u64()?;
        let topology_generation = cursor.u64()?;
        let phase = LockPhase::decode(cursor.u16()?)?;
        let count = usize::from(cursor.u16()?);
        reserved(&mut cursor, 12)?;
        if count > LOCK_FILE_MAX_OUTPUTS {
            return Err(invalid("allocation_count"));
        }
        let allocations = (0..count)
            .map(|_| LockAllocation::read(&mut cursor))
            .collect::<Result<Vec<_>, _>>()?;
        cursor.finish()?;
        let object = Self {
            lock_epoch,
            topology_generation,
            phase,
            allocations,
        };
        object.validate()?;
        Ok(object)
    }
}

/// A chord a provider asks to receive while locked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockChordRequest {
    pub keysym: u32,
    pub modifiers: u16,
}

impl LockChordRequest {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        if self.keysym == 0 {
            return Err(invalid("keysym"));
        }
        if self.modifiers == 0 || self.modifiers & !LOCK_FILE_MODIFIER_MASK != 0 {
            return Err(invalid("modifiers"));
        }
        Ok(())
    }

    /// Whether the chord holds a modifier other than shift. Shift alone
    /// edits the secret, so the owner refuses such a chord as invalid.
    pub const fn has_non_shift_modifier(&self) -> bool {
        self.modifiers & !SHIFT != 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockNegotiate {
    pub minimum_revision: u16,
    pub maximum_revision: u16,
    pub requested_capabilities: u64,
    pub chords: Vec<LockChordRequest>,
}

impl LockNegotiate {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        if self.chords.len() > LOCK_FILE_MAX_CHORDS {
            return Err(invalid("chord_count"));
        }
        self.chords
            .iter()
            .try_for_each(LockChordRequest::validate)?;
        let mut bytes = Vec::with_capacity(16 + 8 * self.chords.len());
        push_u16(&mut bytes, self.minimum_revision);
        push_u16(&mut bytes, self.maximum_revision);
        push_u16(&mut bytes, self.chords.len() as u16);
        push_u16(&mut bytes, 0);
        push_u64(&mut bytes, self.requested_capabilities);
        for chord in &self.chords {
            push_u32(&mut bytes, chord.keysym);
            push_u16(&mut bytes, chord.modifiers);
            push_u16(&mut bytes, 0);
        }
        Ok(bytes)
    }

    /// Revision ranges and capability bits are the owner's to judge; a
    /// malformed chord is refused here.
    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let minimum_revision = cursor.u16()?;
        let maximum_revision = cursor.u16()?;
        let count = usize::from(cursor.u16()?);
        zero_u16(&mut cursor)?;
        let requested_capabilities = cursor.u64()?;
        if count > LOCK_FILE_MAX_CHORDS {
            return Err(invalid("chord_count"));
        }
        let chords = (0..count)
            .map(|_| {
                let chord = LockChordRequest {
                    keysym: cursor.u32()?,
                    modifiers: cursor.u16()?,
                };
                zero_u16(&mut cursor)?;
                chord.validate()?;
                Ok(chord)
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?;
        cursor.finish()?;
        Ok(Self {
            minimum_revision,
            maximum_revision,
            requested_capabilities,
            chords,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockNegotiated {
    pub granted_chords: u16,
    pub granted_capabilities: u64,
}

impl LockNegotiated {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        range(
            u64::from(self.granted_chords),
            0,
            LOCK_FILE_MAX_CHORDS as u64,
            "granted_chords",
        )?;
        let known = LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_CHORDS;
        if self.granted_capabilities & !known != 0
            || self.granted_capabilities & LOCK_FILE_CAPABILITY_PRESENT == 0
        {
            return Err(invalid("granted_capabilities"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(16);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, self.granted_chords);
        push_u32(&mut bytes, 0);
        push_u64(&mut bytes, self.granted_capabilities);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.u16()? != 1 {
            return Err(invalid("selected_revision"));
        }
        let granted_chords = cursor.u16()?;
        reserved(&mut cursor, 4)?;
        let negotiated = Self {
            granted_chords,
            granted_capabilities: cursor.u64()?,
        };
        cursor.finish()?;
        negotiated.validate()?;
        Ok(negotiated)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum LockRefusal {
    UnsupportedRevision = 1,
    PresentationRequired = 2,
    InvalidChord = 3,
}

impl LockRefusal {
    pub fn encode(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8);
        push_u16(&mut bytes, self as u16);
        bytes.extend_from_slice(&[0; 6]);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let refusal = match cursor.u16()? {
            1 => Self::UnsupportedRevision,
            2 => Self::PresentationRequired,
            3 => Self::InvalidChord,
            _ => return Err(invalid("refusal")),
        };
        reserved(&mut cursor, 6)?;
        cursor.finish()?;
        Ok(refusal)
    }
}

/// The submission a candidate record was accepted under.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockSubmitted {
    pub submission_id: u64,
    pub candidate_kind: LockFileKind,
}

impl LockSubmitted {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        if self.candidate_kind.class() != LockFileClass::Candidate {
            return Err(invalid("candidate_kind"));
        }
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, nonzero(self.submission_id, "submission_id")?);
        push_u16(&mut bytes, self.candidate_kind as u16);
        bytes.extend_from_slice(&[0; 6]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let submission_id = nonzero(cursor.u64()?, "submission_id")?;
        let candidate_kind = LockFileKind::decode(cursor.u16()?)
            .ok()
            .filter(|kind| kind.class() == LockFileClass::Candidate)
            .ok_or_else(|| invalid("candidate_kind"))?;
        reserved(&mut cursor, 6)?;
        cursor.finish()?;
        Ok(Self {
            submission_id,
            candidate_kind,
        })
    }
}

/// A new generation of the lock object, at its file's qid path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockObjectPublished {
    pub object_generation: u64,
    pub qid_path: u64,
}

impl LockObjectPublished {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(24);
        push_u16(&mut bytes, LockFileKind::Lock as u16);
        bytes.extend_from_slice(&[0; 6]);
        push_u64(
            &mut bytes,
            nonzero(self.object_generation, "object_generation")?,
        );
        push_u64(&mut bytes, nonzero(self.qid_path, "qid_path")?);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.u16()? != LockFileKind::Lock as u16 {
            return Err(invalid("object_kind"));
        }
        reserved(&mut cursor, 6)?;
        let published = Self {
            object_generation: nonzero(cursor.u64()?, "object_generation")?,
            qid_path: nonzero(cursor.u64()?, "qid_path")?,
        };
        cursor.finish()?;
        Ok(published)
    }
}

/// What the lock's secret did, never what it holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum LockEntryKind {
    Insert = 1,
    Delete = 2,
    Clear = 3,
    Submit = 4,
    Checking = 5,
    Failed = 6,
    Unavailable = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockEntry {
    pub lock_epoch: u64,
    pub entry: LockEntryKind,
    pub empty_after: bool,
}

impl LockEntry {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, nonzero(self.lock_epoch, "lock_epoch")?);
        push_u16(&mut bytes, self.entry as u16);
        push_u16(&mut bytes, u16::from(self.empty_after));
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let lock_epoch = nonzero(cursor.u64()?, "lock_epoch")?;
        let entry = match cursor.u16()? {
            1 => LockEntryKind::Insert,
            2 => LockEntryKind::Delete,
            3 => LockEntryKind::Clear,
            4 => LockEntryKind::Submit,
            5 => LockEntryKind::Checking,
            6 => LockEntryKind::Failed,
            7 => LockEntryKind::Unavailable,
            _ => return Err(invalid("entry")),
        };
        let empty_after = match cursor.u16()? {
            0 => false,
            1 => true,
            _ => return Err(invalid("empty_after")),
        };
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        Ok(Self {
            lock_epoch,
            entry,
            empty_after,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockChord {
    pub lock_epoch: u64,
    pub chord: u16,
}

impl LockChord {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        range(
            u64::from(self.chord),
            0,
            LOCK_FILE_MAX_CHORDS as u64 - 1,
            "chord",
        )?;
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, nonzero(self.lock_epoch, "lock_epoch")?);
        push_u16(&mut bytes, self.chord);
        bytes.extend_from_slice(&[0; 6]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let chord = Self {
            lock_epoch: nonzero(cursor.u64()?, "lock_epoch")?,
            chord: cursor.u16()?,
        };
        reserved(&mut cursor, 6)?;
        cursor.finish()?;
        range(
            u64::from(chord.chord),
            0,
            LOCK_FILE_MAX_CHORDS as u64 - 1,
            "chord",
        )?;
        Ok(chord)
    }
}

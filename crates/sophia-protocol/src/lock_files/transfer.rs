//! The resource, candidate and pacing bodies of
//! `protocol/sophia-lock-files-v1.kdl`, with their declared byte constraints.

use super::{LOCK_FILE_MAX_PIXELS_PER_SIDE, invalid, nonzero, range, reserved};
use crate::BinaryCodecError;
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

/// A resource identity, scoped to the connection epoch.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LockResourceId {
    pub id: u64,
    pub generation: u64,
}

impl LockResourceId {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, BinaryCodecError> {
        Ok(Self {
            id: nonzero(cursor.u64()?, "resource_id")?,
            generation: nonzero(cursor.u64()?, "resource_generation")?,
        })
    }

    fn push(&self, bytes: &mut Vec<u8>) -> Result<(), BinaryCodecError> {
        nonzero(self.id, "resource_id")?;
        nonzero(self.generation, "resource_generation")?;
        push_u64(bytes, self.id);
        push_u64(bytes, self.generation);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockResourceBegin {
    pub transaction: u64,
    pub resource: LockResourceId,
    pub width_px: u32,
    pub height_px: u32,
    pub slot: u16,
}

impl LockResourceBegin {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.transaction, "transaction")?;
        range(
            u64::from(self.width_px),
            1,
            u64::from(LOCK_FILE_MAX_PIXELS_PER_SIDE),
            "width_px",
        )?;
        range(
            u64::from(self.height_px),
            1,
            u64::from(LOCK_FILE_MAX_PIXELS_PER_SIDE),
            "height_px",
        )?;
        range(u64::from(self.slot), 0, 3, "slot")
    }

    /// `width_px * 4 * height_px`, the bytes its upload must carry.
    pub const fn total_bytes(&self) -> u64 {
        self.width_px as u64 * 4 * self.height_px as u64
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(40);
        push_u64(&mut bytes, self.transaction);
        self.resource.push(&mut bytes)?;
        push_u32(&mut bytes, self.width_px);
        push_u32(&mut bytes, self.height_px);
        push_u16(&mut bytes, self.slot);
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let begin = Self {
            transaction: cursor.u64()?,
            resource: LockResourceId::read(&mut cursor)?,
            width_px: cursor.u32()?,
            height_px: cursor.u32()?,
            slot: cursor.u16()?,
        };
        if cursor.u16()? != 1 {
            return Err(invalid("pixel_format"));
        }
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        begin.validate()?;
        Ok(begin)
    }
}

/// `ResourceEnd`, `ResourceCancel` and `ResourceRetire` name a transaction
/// and a resource; `ResourceEnd` adds the bytes it delivered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockResourceStep {
    pub transaction: u64,
    pub resource: LockResourceId,
    /// Only for `ResourceEnd`.
    pub total_bytes: Option<u64>,
}

impl LockResourceStep {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        nonzero(self.transaction, "transaction")?;
        let mut bytes = Vec::with_capacity(32);
        push_u64(&mut bytes, self.transaction);
        self.resource.push(&mut bytes)?;
        if let Some(total) = self.total_bytes {
            push_u64(&mut bytes, nonzero(total, "total_bytes")?);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8], with_total: bool) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let step = Self {
            transaction: nonzero(cursor.u64()?, "transaction")?,
            resource: LockResourceId::read(&mut cursor)?,
            total_bytes: if with_total {
                Some(nonzero(cursor.u64()?, "total_bytes")?)
            } else {
                None
            },
        };
        cursor.finish()?;
        Ok(step)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum LockResourceState {
    Admitted = 1,
    Accepted = 2,
    Rejected = 3,
    Cancelled = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockResourceStatus {
    pub transaction: u64,
    pub resource: LockResourceId,
    pub status: LockResourceState,
    pub reason: u16,
    pub admitted_bytes: u64,
}

impl LockResourceStatus {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        nonzero(self.transaction, "transaction")?;
        let mut bytes = Vec::with_capacity(40);
        push_u64(&mut bytes, self.transaction);
        self.resource.push(&mut bytes)?;
        push_u16(&mut bytes, self.status as u16);
        push_u16(&mut bytes, self.reason);
        push_u32(&mut bytes, 0);
        push_u64(&mut bytes, self.admitted_bytes);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let transaction = nonzero(cursor.u64()?, "transaction")?;
        let resource = LockResourceId::read(&mut cursor)?;
        let status = match cursor.u16()? {
            1 => LockResourceState::Admitted,
            2 => LockResourceState::Accepted,
            3 => LockResourceState::Rejected,
            4 => LockResourceState::Cancelled,
            _ => return Err(invalid("status")),
        };
        let reason = cursor.u16()?;
        reserved(&mut cursor, 4)?;
        let admitted_bytes = cursor.u64()?;
        cursor.finish()?;
        Ok(Self {
            transaction,
            resource,
            status,
            reason,
            admitted_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockResourceReleased {
    pub transaction: u64,
    pub resource: LockResourceId,
    pub reason: u16,
}

impl LockResourceReleased {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        nonzero(self.transaction, "transaction")?;
        let mut bytes = Vec::with_capacity(32);
        push_u64(&mut bytes, self.transaction);
        self.resource.push(&mut bytes)?;
        push_u16(&mut bytes, self.reason);
        bytes.extend_from_slice(&[0; 6]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let released = Self {
            transaction: nonzero(cursor.u64()?, "transaction")?,
            resource: LockResourceId::read(&mut cursor)?,
            reason: cursor.u16()?,
        };
        reserved(&mut cursor, 6)?;
        cursor.finish()?;
        Ok(released)
    }
}

/// One image for one output's allocation in one lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockCandidate {
    pub transaction: u64,
    pub lock_epoch: u64,
    pub output_id: u64,
    pub output_generation: u64,
    pub allocation_id: u64,
    pub allocation_generation: u64,
    pub candidate_generation: u64,
    pub pacing_permit: u64,
    pub resource: LockResourceId,
}

impl LockCandidate {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        for (value, field) in [
            (self.transaction, "transaction"),
            (self.lock_epoch, "lock_epoch"),
            (self.output_id, "output_id"),
            (self.output_generation, "output_generation"),
            (self.allocation_id, "allocation_id"),
            (self.allocation_generation, "allocation_generation"),
            (self.candidate_generation, "candidate_generation"),
            (self.pacing_permit, "pacing_permit"),
        ] {
            nonzero(value, field)?;
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(96);
        for value in [
            self.transaction,
            self.lock_epoch,
            self.output_id,
            self.output_generation,
            self.allocation_id,
            self.allocation_generation,
            self.candidate_generation,
            self.pacing_permit,
        ] {
            push_u64(&mut bytes, value);
        }
        self.resource.push(&mut bytes)?;
        bytes.extend_from_slice(&[0; 16]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let candidate = Self {
            transaction: cursor.u64()?,
            lock_epoch: cursor.u64()?,
            output_id: cursor.u64()?,
            output_generation: cursor.u64()?,
            allocation_id: cursor.u64()?,
            allocation_generation: cursor.u64()?,
            candidate_generation: cursor.u64()?,
            pacing_permit: cursor.u64()?,
            resource: LockResourceId::read(&mut cursor)?,
        };
        reserved(&mut cursor, 16)?;
        cursor.finish()?;
        candidate.validate()?;
        Ok(candidate)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum LockCandidateStatus {
    Prepared = 1,
    Presented = 2,
    Rejected = 3,
    Superseded = 4,
    Revoked = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockCandidateOutcome {
    pub transaction: u64,
    pub lock_epoch: u64,
    pub output_id: u64,
    pub allocation_id: u64,
    pub candidate_generation: u64,
    pub status: LockCandidateStatus,
    pub reason: u16,
}

impl LockCandidateOutcome {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(56);
        for (value, field) in [
            (self.transaction, "transaction"),
            (self.lock_epoch, "lock_epoch"),
            (self.output_id, "output_id"),
            (self.allocation_id, "allocation_id"),
            (self.candidate_generation, "candidate_generation"),
        ] {
            push_u64(&mut bytes, nonzero(value, field)?);
        }
        push_u16(&mut bytes, self.status as u16);
        push_u16(&mut bytes, self.reason);
        bytes.extend_from_slice(&[0; 12]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let transaction = nonzero(cursor.u64()?, "transaction")?;
        let lock_epoch = nonzero(cursor.u64()?, "lock_epoch")?;
        let output_id = nonzero(cursor.u64()?, "output_id")?;
        let allocation_id = nonzero(cursor.u64()?, "allocation_id")?;
        let candidate_generation = nonzero(cursor.u64()?, "candidate_generation")?;
        let status = match cursor.u16()? {
            1 => LockCandidateStatus::Prepared,
            2 => LockCandidateStatus::Presented,
            3 => LockCandidateStatus::Rejected,
            4 => LockCandidateStatus::Superseded,
            5 => LockCandidateStatus::Revoked,
            _ => return Err(invalid("status")),
        };
        let reason = cursor.u16()?;
        reserved(&mut cursor, 12)?;
        cursor.finish()?;
        Ok(Self {
            transaction,
            lock_epoch,
            output_id,
            allocation_id,
            candidate_generation,
            status,
            reason,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFrameDemand {
    pub transaction: u64,
    pub lock_epoch: u64,
    pub allocation_id: u64,
    pub allocation_generation: u64,
    pub demand_id: u64,
}

impl LockFrameDemand {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(40);
        for (value, field) in [
            (self.transaction, "transaction"),
            (self.lock_epoch, "lock_epoch"),
            (self.allocation_id, "allocation_id"),
            (self.allocation_generation, "allocation_generation"),
            (self.demand_id, "demand_id"),
        ] {
            push_u64(&mut bytes, nonzero(value, field)?);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let demand = Self {
            transaction: nonzero(cursor.u64()?, "transaction")?,
            lock_epoch: nonzero(cursor.u64()?, "lock_epoch")?,
            allocation_id: nonzero(cursor.u64()?, "allocation_id")?,
            allocation_generation: nonzero(cursor.u64()?, "allocation_generation")?,
            demand_id: nonzero(cursor.u64()?, "demand_id")?,
        };
        cursor.finish()?;
        Ok(demand)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFramePermit {
    pub lock_epoch: u64,
    pub allocation_id: u64,
    pub allocation_generation: u64,
    pub demand_id: u64,
    pub pacing_permit: u64,
    pub expires_after_ms: u32,
}

impl LockFramePermit {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        range(u64::from(self.expires_after_ms), 1, 250, "expires_after_ms")?;
        let mut bytes = Vec::with_capacity(48);
        for (value, field) in [
            (self.lock_epoch, "lock_epoch"),
            (self.allocation_id, "allocation_id"),
            (self.allocation_generation, "allocation_generation"),
            (self.demand_id, "demand_id"),
            (self.pacing_permit, "pacing_permit"),
        ] {
            push_u64(&mut bytes, nonzero(value, field)?);
        }
        push_u32(&mut bytes, self.expires_after_ms);
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let permit = Self {
            lock_epoch: nonzero(cursor.u64()?, "lock_epoch")?,
            allocation_id: nonzero(cursor.u64()?, "allocation_id")?,
            allocation_generation: nonzero(cursor.u64()?, "allocation_generation")?,
            demand_id: nonzero(cursor.u64()?, "demand_id")?,
            pacing_permit: nonzero(cursor.u64()?, "pacing_permit")?,
            expires_after_ms: cursor.u32()?,
        };
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        range(
            u64::from(permit.expires_after_ms),
            1,
            250,
            "expires_after_ms",
        )?;
        Ok(permit)
    }
}

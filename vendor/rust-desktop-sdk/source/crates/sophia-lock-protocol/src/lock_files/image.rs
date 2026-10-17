//! The image-channel bodies of `docs/sophia-image-channel.md`, bound to the
//! lock role (role code 2). Byte shape only: admission, budgets, topology and
//! grants are the lock export's to judge. Nothing here advertises lock r2;
//! discovery, records and packets exist so codecs and tests can speak the
//! contract before any runtime enables it.

use super::{invalid, nonzero, range, reserved};
use crate::BinaryCodecError;
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

pub const LOCK_IMAGE_CHANNEL_ROLE: u16 = 2;
pub const LOCK_IMAGE_SOURCE_SEALED_MEMORY: u32 = 1;
pub const LOCK_IMAGE_SOURCE_DMA_BUF: u32 = 2;
pub const LOCK_IMAGE_SEALED_FORMAT: u32 = 1;
pub const LOCK_IMAGE_MAX_PLANES: usize = 4;
pub const LOCK_IMAGE_MAX_FORMAT_ROWS: usize = 4096;
pub const LOCK_IMAGE_FORMAT_FLAG_OPAQUE: u32 = 1;
pub const LOCK_IMAGE_FORMAT_FLAG_VALIDATED: u32 = 2;

fn nonzero_u32(value: u32, field: &'static str) -> Result<u32, BinaryCodecError> {
    if value == 0 {
        return Err(invalid(field));
    }
    Ok(value)
}

/// The 96-byte metadata shared by `ImageBegin`, `ImageAdmitted` and the
/// descriptor packet. Validation is intrinsic: identity fields are nonzero,
/// sealed sources are tight BGRA8, and declared bytes are checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImageMetadata {
    pub offer_id: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub admission_epoch: u64,
    pub output_id: u64,
    pub allocation_id: u64,
    pub topology_generation: u64,
    pub source_kind: u32,
    pub source_format: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub plane_count: u16,
    pub descriptor_count: u16,
    pub declared_bytes: u64,
    pub modifier: u64,
}

impl LockImageMetadata {
    pub const BYTES: usize = 96;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        for (value, field) in [
            (self.offer_id, "offer_id"),
            (self.resource, "resource_id"),
            (self.resource_generation, "resource_generation"),
            (self.admission_epoch, "admission_epoch"),
            (self.output_id, "output_id"),
            (self.allocation_id, "allocation_id"),
            (self.topology_generation, "topology_generation"),
        ] {
            nonzero(value, field)?;
        }
        // No role maximum here: the shared contract leaves per-role width
        // and height limits to the caller's role-limit check.
        nonzero_u32(self.width, "width")?;
        nonzero_u32(self.height, "height")?;
        let bytes = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| invalid("declared_bytes"))?;
        if self.declared_bytes != bytes {
            return Err(invalid("declared_bytes"));
        }
        if self.descriptor_count != self.plane_count {
            return Err(invalid("descriptor_count"));
        }
        match self.source_kind {
            LOCK_IMAGE_SOURCE_SEALED_MEMORY => {
                if self.source_format != LOCK_IMAGE_SEALED_FORMAT {
                    return Err(invalid("source_format"));
                }
                if self.plane_count != 1 {
                    return Err(invalid("plane_count"));
                }
                let stride = self.width.checked_mul(4).ok_or_else(|| invalid("stride"))?;
                if self.stride != stride {
                    return Err(invalid("stride"));
                }
                if self.modifier != 0 {
                    return Err(invalid("modifier"));
                }
            }
            LOCK_IMAGE_SOURCE_DMA_BUF => {
                nonzero_u32(self.source_format, "source_format")?;
                range(
                    u64::from(self.plane_count),
                    1,
                    LOCK_IMAGE_MAX_PLANES as u64,
                    "plane_count",
                )?;
                if self.stride != 0 {
                    return Err(invalid("stride"));
                }
            }
            _ => return Err(invalid("source_kind")),
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<[u8; Self::BYTES], BinaryCodecError> {
        self.validate()?;
        // Fill the fixed array directly; encoding a metadata value on the
        // packet path must not allocate a scratch buffer.
        let mut out = [0; Self::BYTES];
        out[0..8].copy_from_slice(&self.offer_id.to_le_bytes());
        out[8..16].copy_from_slice(&self.resource.to_le_bytes());
        out[16..24].copy_from_slice(&self.resource_generation.to_le_bytes());
        out[24..32].copy_from_slice(&self.admission_epoch.to_le_bytes());
        out[32..40].copy_from_slice(&self.output_id.to_le_bytes());
        out[40..48].copy_from_slice(&self.allocation_id.to_le_bytes());
        out[48..56].copy_from_slice(&self.topology_generation.to_le_bytes());
        out[56..60].copy_from_slice(&self.source_kind.to_le_bytes());
        out[60..64].copy_from_slice(&self.source_format.to_le_bytes());
        out[64..68].copy_from_slice(&self.width.to_le_bytes());
        out[68..72].copy_from_slice(&self.height.to_le_bytes());
        out[72..76].copy_from_slice(&self.stride.to_le_bytes());
        out[76..78].copy_from_slice(&self.plane_count.to_le_bytes());
        out[78..80].copy_from_slice(&self.descriptor_count.to_le_bytes());
        out[80..88].copy_from_slice(&self.declared_bytes.to_le_bytes());
        out[88..96].copy_from_slice(&self.modifier.to_le_bytes());
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let metadata = Self {
            offer_id: cursor.u64()?,
            resource: cursor.u64()?,
            resource_generation: cursor.u64()?,
            admission_epoch: cursor.u64()?,
            output_id: cursor.u64()?,
            allocation_id: cursor.u64()?,
            topology_generation: cursor.u64()?,
            source_kind: cursor.u32()?,
            source_format: cursor.u32()?,
            width: cursor.u32()?,
            height: cursor.u32()?,
            stride: cursor.u32()?,
            plane_count: cursor.u16()?,
            descriptor_count: cursor.u16()?,
            declared_bytes: cursor.u64()?,
            modifier: cursor.u64()?,
        };
        cursor.finish()?;
        metadata.validate()?;
        Ok(metadata)
    }
}

/// One DMA-BUF plane row. Sealed sources use a single zero row past the
/// fields the metadata already carries, with offset zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImagePlane {
    pub offset: u64,
    pub stride: u32,
    pub fd_index: u16,
}

impl LockImagePlane {
    fn push(&self, bytes: &mut Vec<u8>) {
        push_u64(bytes, self.offset);
        push_u32(bytes, self.stride);
        push_u16(bytes, self.fd_index);
        push_u16(bytes, 0);
    }

    fn read(cursor: &mut Cursor<'_>) -> Result<Self, BinaryCodecError> {
        let plane = Self {
            offset: cursor.u64()?,
            stride: cursor.u32()?,
            fd_index: cursor.u16()?,
        };
        reserved(cursor, 2)?;
        Ok(plane)
    }
}

/// The fixed 104-byte `ImageLimits` body (136 bytes with the record header).
/// Generations and ceilings only; whether a sealed or DMA-BUF field may be
/// nonzero depends on the advertised source kinds, which the export judges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImageLimits {
    pub channel_generation: u64,
    pub limits_generation: u64,
    pub max_resource_bytes: u64,
    pub max_sealed_backing_bytes: u64,
    pub max_dma_source_bytes: u64,
    pub max_renderer_allocation_bytes: u64,
    pub max_renderer_entries: u32,
    pub max_offers: u32,
    pub max_validation_jobs: u32,
    pub max_capture_jobs: u32,
    pub max_admitted_outputs: u32,
    pub max_format_rows_per_output: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub validation_timeout_ms: u32,
    pub capture_interest_timeout_ms: u32,
    pub validation_chunk_bytes: u32,
    pub max_packets_per_turn: u32,
}

impl LockImageLimits {
    pub const BYTES: usize = 104;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.channel_generation, "channel_generation")?;
        nonzero(self.limits_generation, "limits_generation")?;
        nonzero(self.max_resource_bytes, "max_resource_bytes")?;
        range(u64::from(self.max_offers), 1, 4, "max_offers")?;
        range(
            u64::from(self.max_admitted_outputs),
            1,
            16,
            "max_admitted_outputs",
        )?;
        range(
            u64::from(self.max_format_rows_per_output),
            1,
            256,
            "max_format_rows_per_output",
        )?;
        nonzero_u32(self.max_width, "max_width")?;
        nonzero_u32(self.max_height, "max_height")?;
        nonzero_u32(self.validation_timeout_ms, "validation_timeout_ms")?;
        nonzero_u32(
            self.capture_interest_timeout_ms,
            "capture_interest_timeout_ms",
        )?;
        nonzero_u32(self.validation_chunk_bytes, "validation_chunk_bytes")?;
        nonzero_u32(self.max_packets_per_turn, "max_packets_per_turn")?;
        // Sealed group: both zero, or validation jobs within the offer
        // bound and backing room for one maximum-size resource.
        if (self.max_validation_jobs != 0 || self.max_sealed_backing_bytes != 0)
            && (self.max_validation_jobs == 0
                || self.max_validation_jobs > self.max_offers
                || self.max_sealed_backing_bytes < self.max_resource_bytes)
        {
            return Err(invalid("max_validation_jobs"));
        }
        // DMA-BUF group: all four reservations zero, or all positive with
        // capture jobs within the offer bound.
        let dma = [
            self.max_dma_source_bytes != 0,
            self.max_renderer_allocation_bytes != 0,
            self.max_renderer_entries != 0,
            self.max_capture_jobs != 0,
        ];
        if dma.iter().any(|set| *set)
            && (dma.iter().any(|set| !*set) || self.max_capture_jobs > self.max_offers)
        {
            return Err(invalid("max_capture_jobs"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(Self::BYTES);
        for value in [
            self.channel_generation,
            self.limits_generation,
            self.max_resource_bytes,
            self.max_sealed_backing_bytes,
            self.max_dma_source_bytes,
            self.max_renderer_allocation_bytes,
        ] {
            push_u64(&mut bytes, value);
        }
        for value in [
            self.max_renderer_entries,
            self.max_offers,
            self.max_validation_jobs,
            self.max_capture_jobs,
            self.max_admitted_outputs,
            self.max_format_rows_per_output,
            self.max_width,
            self.max_height,
            self.validation_timeout_ms,
            self.capture_interest_timeout_ms,
            self.validation_chunk_bytes,
            self.max_packets_per_turn,
        ] {
            push_u32(&mut bytes, value);
        }
        push_u64(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let limits = Self {
            channel_generation: cursor.u64()?,
            limits_generation: cursor.u64()?,
            max_resource_bytes: cursor.u64()?,
            max_sealed_backing_bytes: cursor.u64()?,
            max_dma_source_bytes: cursor.u64()?,
            max_renderer_allocation_bytes: cursor.u64()?,
            max_renderer_entries: cursor.u32()?,
            max_offers: cursor.u32()?,
            max_validation_jobs: cursor.u32()?,
            max_capture_jobs: cursor.u32()?,
            max_admitted_outputs: cursor.u32()?,
            max_format_rows_per_output: cursor.u32()?,
            max_width: cursor.u32()?,
            max_height: cursor.u32()?,
            validation_timeout_ms: cursor.u32()?,
            capture_interest_timeout_ms: cursor.u32()?,
            validation_chunk_bytes: cursor.u32()?,
            max_packets_per_turn: cursor.u32()?,
        };
        reserved(&mut cursor, 8)?;
        cursor.finish()?;
        limits.validate()?;
        Ok(limits)
    }
}

/// One advertised DMA-BUF import pair for one output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImageFormatRow {
    pub output: u64,
    pub device_major: u32,
    pub device_minor: u32,
    pub fourcc: u32,
    pub flags: u32,
    pub modifier: u64,
    pub max_width: u32,
    pub max_height: u32,
}

impl LockImageFormatRow {
    fn key(&self) -> (u64, u32, u32, u32, u64) {
        (
            self.output,
            self.device_major,
            self.device_minor,
            self.fourcc,
            self.modifier,
        )
    }

    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.output, "output")?;
        nonzero_u32(self.fourcc, "fourcc")?;
        if !matches!(
            self.flags,
            LOCK_IMAGE_FORMAT_FLAG_OPAQUE | LOCK_IMAGE_FORMAT_FLAG_VALIDATED
        ) {
            return Err(invalid("flags"));
        }
        nonzero_u32(self.max_width, "max_width")?;
        nonzero_u32(self.max_height, "max_height")?;
        Ok(())
    }

    fn push(&self, bytes: &mut Vec<u8>) {
        push_u64(bytes, self.output);
        push_u32(bytes, self.device_major);
        push_u32(bytes, self.device_minor);
        push_u32(bytes, self.fourcc);
        push_u32(bytes, self.flags);
        push_u64(bytes, self.modifier);
        push_u32(bytes, self.max_width);
        push_u32(bytes, self.max_height);
    }

    fn read(cursor: &mut Cursor<'_>) -> Result<Self, BinaryCodecError> {
        let row = Self {
            output: cursor.u64()?,
            device_major: cursor.u32()?,
            device_minor: cursor.u32()?,
            fourcc: cursor.u32()?,
            flags: cursor.u32()?,
            modifier: cursor.u64()?,
            max_width: cursor.u32()?,
            max_height: cursor.u32()?,
        };
        row.validate()?;
        Ok(row)
    }
}

/// The `ImageFormats` body: a 32-byte snapshot prefix plus ordered rows.
/// Generation coherence against discovery is the reader's to judge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockImageFormats {
    pub channel_generation: u64,
    pub formats_generation: u64,
    pub limits_generation: u64,
    pub rows: Vec<LockImageFormatRow>,
}

impl LockImageFormats {
    const PREFIX: usize = 32;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.channel_generation, "channel_generation")?;
        nonzero(self.formats_generation, "formats_generation")?;
        nonzero(self.limits_generation, "limits_generation")?;
        if self.rows.len() > LOCK_IMAGE_MAX_FORMAT_ROWS {
            return Err(invalid("row_count"));
        }
        if self
            .rows
            .windows(2)
            .any(|pair| pair[0].key() >= pair[1].key())
        {
            return Err(invalid("row_order"));
        }
        self.rows.iter().try_for_each(LockImageFormatRow::validate)
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(Self::PREFIX + self.rows.len() * 40);
        push_u64(&mut bytes, self.channel_generation);
        push_u64(&mut bytes, self.formats_generation);
        push_u64(&mut bytes, self.limits_generation);
        push_u32(&mut bytes, self.rows.len() as u32);
        push_u32(&mut bytes, 0);
        for row in &self.rows {
            row.push(&mut bytes);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let channel_generation = cursor.u64()?;
        let formats_generation = cursor.u64()?;
        let limits_generation = cursor.u64()?;
        let count = usize::try_from(cursor.u32()?).map_err(|_| invalid("row_count"))?;
        reserved(&mut cursor, 4)?;
        if count > LOCK_IMAGE_MAX_FORMAT_ROWS {
            return Err(invalid("row_count"));
        }
        let rows = (0..count)
            .map(|_| LockImageFormatRow::read(&mut cursor))
            .collect::<Result<Vec<_>, _>>()?;
        cursor.finish()?;
        let formats = Self {
            channel_generation,
            formats_generation,
            limits_generation,
            rows,
        };
        formats.validate()?;
        Ok(formats)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LockSourceDisposition {
    Retained = 1,
    CaptureComplete = 2,
    RefusedBeforeUse = 3,
    CancelledBeforeImport = 4,
    DrainedAfterSubmission = 5,
}

/// The 32-byte `SourceReleased` body. Recorded once per admitted offer on a
/// live journal; it never replaces `ResourceStatus` or `ResourceReleased`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockSourceReleased {
    pub offer_id: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub disposition: LockSourceDisposition,
}

impl LockSourceReleased {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(32);
        push_u64(&mut bytes, nonzero(self.offer_id, "offer_id")?);
        push_u64(&mut bytes, nonzero(self.resource, "resource_id")?);
        push_u64(
            &mut bytes,
            nonzero(self.resource_generation, "resource_generation")?,
        );
        push_u32(&mut bytes, self.disposition as u32);
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let released = Self {
            offer_id: nonzero(cursor.u64()?, "offer_id")?,
            resource: nonzero(cursor.u64()?, "resource_id")?,
            resource_generation: nonzero(cursor.u64()?, "resource_generation")?,
            disposition: match cursor.u32()? {
                1 => LockSourceDisposition::Retained,
                2 => LockSourceDisposition::CaptureComplete,
                3 => LockSourceDisposition::RefusedBeforeUse,
                4 => LockSourceDisposition::CancelledBeforeImport,
                5 => LockSourceDisposition::DrainedAfterSubmission,
                _ => return Err(invalid("disposition")),
            },
        };
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        Ok(released)
    }
}

/// The 16-byte `ChannelReady` body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockChannelReady {
    pub channel_generation: u64,
    pub granted_source_kinds: u32,
}

impl LockChannelReady {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        nonzero(self.channel_generation, "channel_generation")?;
        if self.granted_source_kinds == 0 || self.granted_source_kinds & !0b11 != 0 {
            return Err(invalid("granted_source_kinds"));
        }
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, self.channel_generation);
        push_u32(&mut bytes, self.granted_source_kinds);
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let ready = Self {
            channel_generation: nonzero(cursor.u64()?, "channel_generation")?,
            granted_source_kinds: cursor.u32()?,
        };
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        if ready.granted_source_kinds == 0 || ready.granted_source_kinds & !0b11 != 0 {
            return Err(invalid("granted_source_kinds"));
        }
        Ok(ready)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LockChannelClosedReason {
    PeerDisconnect = 1,
    ProtocolFault = 2,
    HelloTimeout = 3,
    GrantRevoked = 4,
}

/// The 16-byte `ChannelClosed` body. It names channel state only and never
/// implies that a capture has finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockChannelClosed {
    pub channel_generation: u64,
    pub reason: LockChannelClosedReason,
}

impl LockChannelClosed {
    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(16);
        push_u64(
            &mut bytes,
            nonzero(self.channel_generation, "channel_generation")?,
        );
        push_u32(&mut bytes, self.reason as u32);
        push_u32(&mut bytes, 0);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let closed = Self {
            channel_generation: nonzero(cursor.u64()?, "channel_generation")?,
            reason: match cursor.u32()? {
                1 => LockChannelClosedReason::PeerDisconnect,
                2 => LockChannelClosedReason::ProtocolFault,
                3 => LockChannelClosedReason::HelloTimeout,
                4 => LockChannelClosedReason::GrantRevoked,
                _ => return Err(invalid("reason")),
            },
        };
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        Ok(closed)
    }
}

/// One validated channel packet for the lock role. Descriptor custody is
/// deliberately not represented: counts must agree here, while actually
/// received references belong to the channel endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockImagePacket {
    Hello {
        connection_epoch: u64,
        channel_generation: u64,
    },
    Source {
        connection_epoch: u64,
        channel_generation: u64,
        metadata: LockImageMetadata,
        planes: [LockImagePlane; LOCK_IMAGE_MAX_PLANES],
    },
}

impl LockImagePacket {
    pub const HELLO_BYTES: usize = 40;
    pub const SOURCE_BYTES: usize = 200;

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        let mut bytes = Vec::with_capacity(Self::SOURCE_BYTES);
        match self {
            Self::Hello {
                connection_epoch,
                channel_generation,
            } => {
                push_u32(&mut bytes, Self::HELLO_BYTES as u32);
                push_u16(&mut bytes, 1);
                push_u16(&mut bytes, 1);
                push_u16(&mut bytes, LOCK_IMAGE_CHANNEL_ROLE);
                push_u16(&mut bytes, 0);
                push_u32(&mut bytes, 0);
                push_u64(&mut bytes, nonzero(*connection_epoch, "connection_epoch")?);
                push_u64(
                    &mut bytes,
                    nonzero(*channel_generation, "channel_generation")?,
                );
                push_u64(&mut bytes, 0);
            }
            Self::Source {
                connection_epoch,
                channel_generation,
                metadata,
                planes,
            } => {
                let count = metadata.descriptor_count;
                if usize::from(count) > LOCK_IMAGE_MAX_PLANES {
                    return Err(invalid("descriptor_count"));
                }
                for (index, plane) in planes.iter().enumerate() {
                    let used = index < usize::from(count);
                    if used && plane.fd_index != index as u16 {
                        return Err(invalid("fd_index"));
                    }
                    if used
                        && metadata.source_kind == LOCK_IMAGE_SOURCE_SEALED_MEMORY
                        && (plane.offset != 0 || plane.stride != metadata.stride)
                    {
                        return Err(invalid("plane_layout"));
                    }
                    if !used && (plane.offset != 0 || plane.stride != 0 || plane.fd_index != 0) {
                        return Err(invalid("plane_layout"));
                    }
                }
                push_u32(&mut bytes, Self::SOURCE_BYTES as u32);
                push_u16(&mut bytes, 1);
                push_u16(&mut bytes, 2);
                push_u16(&mut bytes, LOCK_IMAGE_CHANNEL_ROLE);
                push_u16(&mut bytes, count);
                push_u32(&mut bytes, 0);
                push_u64(&mut bytes, nonzero(*connection_epoch, "connection_epoch")?);
                push_u64(
                    &mut bytes,
                    nonzero(*channel_generation, "channel_generation")?,
                );
                push_u64(&mut bytes, metadata.offer_id);
                bytes.extend_from_slice(&metadata.encode()?);
                for plane in planes {
                    plane.push(&mut bytes);
                }
            }
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let total = cursor.u32()? as usize;
        if total != bytes.len() {
            return Err(invalid("total_bytes"));
        }
        if cursor.u16()? != 1 {
            return Err(invalid("channel_version"));
        }
        let kind = cursor.u16()?;
        if cursor.u16()? != LOCK_IMAGE_CHANNEL_ROLE {
            return Err(invalid("role"));
        }
        let descriptor_count = cursor.u16()?;
        reserved(&mut cursor, 4)?;
        let connection_epoch = nonzero(cursor.u64()?, "connection_epoch")?;
        let channel_generation = nonzero(cursor.u64()?, "channel_generation")?;
        let offer_id = cursor.u64()?;
        match kind {
            1 => {
                if total != Self::HELLO_BYTES || descriptor_count != 0 || offer_id != 0 {
                    return Err(invalid("hello"));
                }
                cursor.finish()?;
                Ok(Self::Hello {
                    connection_epoch,
                    channel_generation,
                })
            }
            2 => {
                if total != Self::SOURCE_BYTES {
                    return Err(invalid("total_bytes"));
                }
                nonzero(offer_id, "offer_id")?;
                let metadata = LockImageMetadata::decode(cursor.slice(96)?)?;
                if metadata.offer_id != offer_id {
                    return Err(invalid("offer_id"));
                }
                if metadata.descriptor_count != descriptor_count {
                    return Err(invalid("descriptor_count"));
                }
                let mut planes = [LockImagePlane {
                    offset: 0,
                    stride: 0,
                    fd_index: 0,
                }; LOCK_IMAGE_MAX_PLANES];
                for plane in &mut planes {
                    *plane = LockImagePlane::read(&mut cursor)?;
                }
                cursor.finish()?;
                for (index, plane) in planes.iter().enumerate() {
                    let used = index < usize::from(descriptor_count);
                    if used && plane.fd_index != index as u16 {
                        return Err(invalid("fd_index"));
                    }
                    if used
                        && metadata.source_kind == LOCK_IMAGE_SOURCE_SEALED_MEMORY
                        && (plane.offset != 0 || plane.stride != metadata.stride)
                    {
                        return Err(invalid("plane_layout"));
                    }
                    if !used && (plane.offset != 0 || plane.stride != 0 || plane.fd_index != 0) {
                        return Err(invalid("plane_layout"));
                    }
                }
                Ok(Self::Source {
                    connection_epoch,
                    channel_generation,
                    metadata,
                    planes,
                })
            }
            _ => Err(invalid("packet_kind")),
        }
    }
}

/// The canonical `image-channel` discovery text for the lock role. Version 1
/// refuses unknown, duplicate, missing, reordered or noncanonical fields.
/// Only `max_packet_bytes=200` and `max_fds=4` are fixed by the contract;
/// every other budget and timeout is carried as the positive value the
/// publisher chose, never compared against the fixture's example numbers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockImageDiscovery {
    pub connection_epoch: u64,
    pub channel_generation: u64,
    pub endpoint: String,
    pub source_kinds: u32,
    pub max_offers: u32,
    pub max_packets_per_turn: u32,
    pub hello_timeout_ms: u32,
    pub validation_timeout_ms: u32,
    pub validation_chunk_bytes: u32,
    pub capture_interest_timeout_ms: u32,
    pub formats_generation: u64,
    pub limits_generation: u64,
}

impl LockImageDiscovery {
    pub const MAX_BYTES: usize = 2048;

    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.connection_epoch, "connection_epoch")?;
        nonzero(self.channel_generation, "channel_generation")?;
        if self.endpoint.is_empty()
            || self.endpoint.len() > 64
            || self.endpoint == "."
            || self.endpoint == ".."
            || !self
                .endpoint
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(invalid("endpoint"));
        }
        if self.source_kinds == 0 || self.source_kinds & !0b11 != 0 {
            return Err(invalid("source_kinds"));
        }
        range(u64::from(self.max_offers), 1, 4, "max_offers")?;
        nonzero_u32(self.max_packets_per_turn, "max_packets_per_turn")?;
        nonzero_u32(self.hello_timeout_ms, "hello_timeout_ms")?;
        nonzero_u32(self.validation_timeout_ms, "validation_timeout_ms")?;
        nonzero_u32(self.validation_chunk_bytes, "validation_chunk_bytes")?;
        nonzero_u32(
            self.capture_interest_timeout_ms,
            "capture_interest_timeout_ms",
        )?;
        nonzero(self.formats_generation, "formats_generation")?;
        nonzero(self.limits_generation, "limits_generation")?;
        Ok(())
    }

    pub fn encode(&self) -> Result<String, BinaryCodecError> {
        self.validate()?;
        Ok(format!(
            "sophia-image-channel version=1\n\
             role=2\n\
             connection_epoch={}\n\
             channel_generation={}\n\
             endpoint={}\n\
             source_kinds={}\n\
             max_packet_bytes=200\n\
             max_fds=4\n\
             max_offers={}\n\
             max_packets_per_turn={}\n\
             hello_timeout_ms={}\n\
             validation_timeout_ms={}\n\
             validation_chunk_bytes={}\n\
             capture_interest_timeout_ms={}\n\
             formats_generation={}\n\
             limits_generation={}\n",
            self.connection_epoch,
            self.channel_generation,
            self.endpoint,
            self.source_kinds,
            self.max_offers,
            self.max_packets_per_turn,
            self.hello_timeout_ms,
            self.validation_timeout_ms,
            self.validation_chunk_bytes,
            self.capture_interest_timeout_ms,
            self.formats_generation,
            self.limits_generation,
        ))
    }

    pub fn decode(text: &str) -> Result<Self, BinaryCodecError> {
        if text.len() > Self::MAX_BYTES || !text.ends_with('\n') {
            return Err(invalid("discovery_length"));
        }
        let mut lines = text.split('\n');
        if lines.next() != Some("sophia-image-channel version=1") {
            return Err(invalid("discovery_header"));
        }
        let mut field = |name: &str| -> Result<&str, BinaryCodecError> {
            let line = lines.next().ok_or_else(|| invalid("discovery_fields"))?;
            line.strip_prefix(name)
                .and_then(|value| value.strip_prefix('='))
                .ok_or_else(|| invalid("discovery_fields"))
        };
        fn decimal<T>(text: &str, field: &'static str) -> Result<T, BinaryCodecError>
        where
            T: std::str::FromStr,
        {
            if text.is_empty()
                || !text.bytes().all(|byte| byte.is_ascii_digit())
                || (text.len() > 1 && text.starts_with('0'))
            {
                return Err(invalid(field));
            }
            text.parse().map_err(|_| invalid(field))
        }
        if decimal::<u16>(field("role")?, "role")? != LOCK_IMAGE_CHANNEL_ROLE {
            return Err(invalid("role"));
        }
        let connection_epoch = nonzero(
            decimal(field("connection_epoch")?, "connection_epoch")?,
            "connection_epoch",
        )?;
        let channel_generation = nonzero(
            decimal(field("channel_generation")?, "channel_generation")?,
            "channel_generation",
        )?;
        let endpoint = field("endpoint")?.to_owned();
        let source_kinds = decimal(field("source_kinds")?, "source_kinds")?;
        // Only these two constants are fixed by the contract.
        if decimal::<u32>(field("max_packet_bytes")?, "max_packet_bytes")? != 200 {
            return Err(invalid("max_packet_bytes"));
        }
        if decimal::<u32>(field("max_fds")?, "max_fds")? != 4 {
            return Err(invalid("max_fds"));
        }
        let discovery = Self {
            connection_epoch,
            channel_generation,
            endpoint,
            source_kinds,
            max_offers: decimal(field("max_offers")?, "max_offers")?,
            max_packets_per_turn: decimal(field("max_packets_per_turn")?, "max_packets_per_turn")?,
            hello_timeout_ms: decimal(field("hello_timeout_ms")?, "hello_timeout_ms")?,
            validation_timeout_ms: decimal(
                field("validation_timeout_ms")?,
                "validation_timeout_ms",
            )?,
            validation_chunk_bytes: decimal(
                field("validation_chunk_bytes")?,
                "validation_chunk_bytes",
            )?,
            capture_interest_timeout_ms: decimal(
                field("capture_interest_timeout_ms")?,
                "capture_interest_timeout_ms",
            )?,
            formats_generation: nonzero(
                decimal(field("formats_generation")?, "formats_generation")?,
                "formats_generation",
            )?,
            limits_generation: nonzero(
                decimal(field("limits_generation")?, "limits_generation")?,
                "limits_generation",
            )?,
        };
        if lines.next() != Some("") || lines.next().is_some() {
            return Err(invalid("discovery_fields"));
        }
        discovery.validate()?;
        Ok(discovery)
    }
}

/// The authenticated role's own bounds, from its negotiated limits,
/// supplied by the caller. Nothing here infers them from discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImageRoleBounds {
    pub max_resource_bytes: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub max_admitted_outputs: u32,
    pub transfer_slots: u32,
}

/// What the owner expects on this channel. Bounds come from the limits
/// object; `granted_source_kinds` is the mask granted by ChannelReady (or,
/// immediately after coherence, the discovery mask the caller then
/// narrows with the actual Ready record). This module never narrows it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImageExpect {
    pub role: u16,
    pub connection_epoch: u64,
    pub channel_generation: u64,
    pub granted_source_kinds: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub max_resource_bytes: u64,
}

impl LockImageExpect {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        if self.role != LOCK_IMAGE_CHANNEL_ROLE {
            return Err(invalid("role"));
        }
        nonzero(self.connection_epoch, "connection_epoch")?;
        nonzero(self.channel_generation, "channel_generation")?;
        if self.granted_source_kinds == 0 || self.granted_source_kinds & !0b11 != 0 {
            return Err(invalid("granted_source_kinds"));
        }
        nonzero_u32(self.max_width, "max_width")?;
        nonzero_u32(self.max_height, "max_height")?;
        nonzero(self.max_resource_bytes, "max_resource_bytes")?;
        Ok(())
    }
}

impl LockImageMetadata {
    /// Shape plus the limits-derived bounds and granted source kinds in
    /// `expect`. A kind the grant does not name is refused here, before any
    /// admission state exists.
    pub fn admissible(&self, expect: &LockImageExpect) -> Result<(), BinaryCodecError> {
        self.validate()?;
        expect.validate()?;
        let bit = match self.source_kind {
            LOCK_IMAGE_SOURCE_SEALED_MEMORY => 1,
            LOCK_IMAGE_SOURCE_DMA_BUF => 2,
            _ => return Err(invalid("source_kind")),
        };
        if expect.granted_source_kinds & bit == 0 {
            return Err(invalid("source_kind"));
        }
        if self.width > expect.max_width || self.height > expect.max_height {
            return Err(invalid("dimensions"));
        }
        if self.declared_bytes > expect.max_resource_bytes {
            return Err(invalid("declared_bytes"));
        }
        Ok(())
    }
}

impl LockImagePacket {
    /// Decode against caller-supplied channel context and the descriptor
    /// count actually received. Existing shape decoding stays context-free;
    /// this method adds role, epoch, generation, actual-FD count and
    /// admissibility checks without inferring any grant.
    pub fn decode_with_context(
        bytes: &[u8],
        received_descriptors: u16,
        expect: &LockImageExpect,
    ) -> Result<Self, BinaryCodecError> {
        expect.validate()?;
        let packet = Self::decode(bytes)?;
        let (connection_epoch, channel_generation, descriptor_count) = match &packet {
            Self::Hello {
                connection_epoch,
                channel_generation,
            } => (*connection_epoch, *channel_generation, 0),
            Self::Source {
                connection_epoch,
                channel_generation,
                metadata,
                ..
            } => (
                *connection_epoch,
                *channel_generation,
                metadata.descriptor_count,
            ),
        };
        if connection_epoch != expect.connection_epoch {
            return Err(invalid("connection_epoch"));
        }
        if channel_generation != expect.channel_generation {
            return Err(invalid("channel_generation"));
        }
        if descriptor_count != received_descriptors {
            return Err(invalid("descriptor_count"));
        }
        if let Self::Source { metadata, .. } = &packet {
            metadata.admissible(expect)?;
        }
        Ok(packet)
    }
}

impl LockImageDiscovery {
    /// Cross-object coherence for one discovery set: this text, the decoded
    /// limits and formats bodies, and the connection epochs carried by
    /// their record headers (supplied explicitly, since bodies do not
    /// carry them). On success returns the packet expectation with the
    /// discovery source-kind mask; the caller narrows that mask with the
    /// granted ChannelReady record before admitting Sources.
    pub fn coherent(
        &self,
        limits: &LockImageLimits,
        formats: &LockImageFormats,
        limits_connection_epoch: u64,
        formats_connection_epoch: u64,
        role_bounds: &LockImageRoleBounds,
    ) -> Result<LockImageExpect, BinaryCodecError> {
        // Typed structs can be constructed or mutated past decode, so the
        // intrinsic rules are rechecked here (broader than the C API,
        // which documents decoded inputs; no allocation is needed).
        self.validate()?;
        limits.validate()?;
        formats.validate()?;
        if limits_connection_epoch != self.connection_epoch
            || formats_connection_epoch != self.connection_epoch
            || limits.channel_generation != self.channel_generation
            || formats.channel_generation != self.channel_generation
            || limits.limits_generation != self.limits_generation
            || formats.limits_generation != self.limits_generation
            || formats.formats_generation != self.formats_generation
            || limits.max_offers != self.max_offers
            || limits.validation_timeout_ms != self.validation_timeout_ms
            || limits.capture_interest_timeout_ms != self.capture_interest_timeout_ms
            || limits.validation_chunk_bytes != self.validation_chunk_bytes
            || limits.max_packets_per_turn != self.max_packets_per_turn
        {
            return Err(invalid("coherence"));
        }
        let sealed = self.source_kinds & 1 != 0;
        let dma_buf = self.source_kinds & 2 != 0;
        if !formats.rows.is_empty() && !dma_buf {
            return Err(invalid("formats_unsupported"));
        }
        if sealed != (limits.max_validation_jobs != 0) || dma_buf != (limits.max_capture_jobs != 0)
        {
            return Err(invalid("capacity"));
        }
        if limits.max_resource_bytes > role_bounds.max_resource_bytes
            || limits.max_width > role_bounds.max_width
            || limits.max_height > role_bounds.max_height
            || limits.max_admitted_outputs > role_bounds.max_admitted_outputs
            || limits.max_offers != role_bounds.transfer_slots.min(4)
        {
            return Err(invalid("capacity"));
        }
        // Per-output format bounds: rows are strictly ordered by decode, so
        // one pass counts outputs and rows per output.
        let mut outputs = 0u32;
        let mut per_output = 0u32;
        let mut current = 0u64;
        for row in &formats.rows {
            if row.output != current {
                current = row.output;
                outputs += 1;
                per_output = 0;
                if outputs > limits.max_admitted_outputs {
                    return Err(invalid("capacity"));
                }
            }
            per_output += 1;
            if per_output > limits.max_format_rows_per_output
                || row.max_width > limits.max_width
                || row.max_height > limits.max_height
            {
                return Err(invalid("capacity"));
            }
        }
        Ok(LockImageExpect {
            role: LOCK_IMAGE_CHANNEL_ROLE,
            connection_epoch: self.connection_epoch,
            channel_generation: self.channel_generation,
            granted_source_kinds: self.source_kinds,
            max_width: limits.max_width,
            max_height: limits.max_height,
            max_resource_bytes: limits.max_resource_bytes,
        })
    }
}

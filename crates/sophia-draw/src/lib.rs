//! Experimental bounded content producer; no presentation or placement authority.
//! DRA1 is a disposable prototype encoding, not a negotiated public ABI.
mod raster;
mod wire;
pub use raster::Rasterizer;
use sophia_protocol::{
    ContentGrant, ContentLimits, ContentResourceBegin, ContentResourceId, TransactionId,
};

pub const MAX_BATCH_BYTES: usize = 4096;
pub const MAX_COMMANDS: usize = 32;
pub const MAX_TEXT_BYTES: usize = 256;
pub const MAX_PIXELS: usize = 2048 * 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Rect {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        rgb: [u8; 3],
    },
    Text {
        x: u16,
        y: u16,
        size: u8,
        rgb: [u8; 3],
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub transaction: u64,
    pub grant: ContentGrant,
    pub resource: ContentResourceId,
    pub width: u16,
    pub height: u16,
    pub commands: Vec<Command>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Limit,
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

impl Batch {
    pub fn validate(&self) -> Result<(), Error> {
        if self.transaction == 0
            || self.grant.connection_epoch == 0
            || self.grant.content_grant_epoch == 0
            || self.resource.id == 0
            || self.resource.generation == 0
            || self.width == 0
            || self.height == 0
        {
            return Err(Error::Invalid);
        }
        if self.width > 2048
            || self.height > 64
            || self.commands.is_empty()
            || self.commands.len() > MAX_COMMANDS
        {
            return Err(Error::Limit);
        }
        let mut text_bytes = 0;
        for cmd in &self.commands {
            match cmd {
                Command::Rect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    if *width == 0
                        || *height == 0
                        || u32::from(*x) + u32::from(*width) > u32::from(self.width)
                        || u32::from(*y) + u32::from(*height) > u32::from(self.height)
                    {
                        return Err(Error::Invalid);
                    }
                }
                Command::Text {
                    x, y, size, text, ..
                } => {
                    if *x >= self.width
                        || *y >= self.height
                        || !(8..=24).contains(size)
                        || text.is_empty()
                        || text.len() > 64
                        || !text.bytes().all(|b| (32..=126).contains(&b))
                    {
                        return Err(Error::Invalid);
                    }
                    text_bytes += text.len();
                }
            }
        }
        if text_bytes > MAX_TEXT_BYTES {
            return Err(Error::Limit);
        }
        Ok(())
    }
    pub fn pixel_bytes(&self) -> usize {
        usize::from(self.width) * usize::from(self.height) * 4
    }
    pub fn description(&self, limits: &ContentLimits) -> Result<ContentResourceBegin, Error> {
        self.validate()?;
        let rows = limits.max_chunk_bytes / (u32::from(self.width) * 4);
        if rows == 0 {
            return Err(Error::Limit);
        }
        let description = ContentResourceBegin {
            grant: self.grant,
            resource: self.resource,
            width_px: self.width.into(),
            height_px: self.height.into(),
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: u32::from(self.height).div_ceil(rows),
            total_bytes: self.pixel_bytes() as u64,
        };
        description.layout(limits).map_err(|_| Error::Invalid)?;
        Ok(description)
    }
    pub fn transaction_id(&self) -> TransactionId {
        TransactionId::from_raw(self.transaction)
    }
}

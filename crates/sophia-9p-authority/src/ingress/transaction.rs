//! Translating 9P synthetic filesystem writes into Sophia visual transactions.

use sophia_protocol::geometry::{Rect, Size};
use sophia_protocol::ids::SurfaceId;

/// A visual transaction generated from 9P client writes, ready for engine intake.
#[derive(Clone, Debug, PartialEq)]
pub struct NinePVisualTransaction {
    pub surface_id: SurfaceId,
    pub damage: Rect,
    pub size: Size,
    pub buffer: Vec<u8>,
    pub epoch: u64,
}

impl NinePVisualTransaction {
    pub fn new(surface_id: SurfaceId, size: Size, buffer: Vec<u8>, epoch: u64) -> Self {
        let damage = Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        };
        Self {
            surface_id,
            damage,
            size,
            buffer,
            epoch,
        }
    }
}

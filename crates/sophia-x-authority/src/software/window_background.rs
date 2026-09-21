//! Painting a window with its background when it becomes viewable.

use super::*;

impl XSoftwareBufferStore {
    /// Paints a window's whole area with its background.
    ///
    /// X11 does this when a window becomes viewable and no earlier contents
    /// for it are remembered: the window is tiled with its background pixmap,
    /// or filled with its background pixel when it has none. Without it a
    /// newly mapped window reads back as whatever its buffer happened to
    /// hold, which is zero, and a client that set a background sees black.
    pub fn paint_window_background(
        &mut self,
        drawable: XResourceId,
        size: Size,
        pixel: u32,
        tile: Option<XResourceId>,
    ) -> Option<()> {
        // The tile is copied out first: painting borrows the destination
        // mutably, and a window may legitimately be backed by a pixmap this
        // same store holds.
        let tile = tile.and_then(|tile| {
            self.buffers.get(&tile).map(|buffer| {
                (
                    buffer.bytes.as_ref().clone(),
                    buffer.size,
                    usize::try_from(buffer.stride).unwrap_or(0),
                )
            })
        });
        let handle = self.allocate_handle();
        let (buffer, _) = self.ensure(drawable, size, handle)?;
        match tile.as_ref() {
            Some((bytes, tile_size, tile_stride)) => {
                raster_ops::tile_solid(buffer, bytes, *tile_size, *tile_stride);
            }
            None => raster_ops::fill_solid(buffer, pixel),
        }
        buffer.generation = buffer.generation.checked_add(1)?;
        self.note_export_damage(
            drawable,
            false,
            Some(Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            }),
        );
        Some(())
    }

    /// Whether this drawable has any contents of its own.
    ///
    /// A window with an undefined background that nothing has drawn into has
    /// none, and reading it back must show whatever is underneath rather than
    /// a rectangle of zeros. `image_region` cannot say so: it answers a
    /// zero-filled buffer for an absent one, which is right for a client
    /// reading that window and wrong for compositing it over its parent.
    pub fn has_backing(&self, drawable: XResourceId) -> bool {
        self.buffers.contains_key(&drawable)
    }
}

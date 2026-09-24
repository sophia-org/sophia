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
        tile: Option<(XResourceId, (i32, i32))>,
    ) -> Option<()> {
        // The tile is the one captured for the window the background comes
        // from, which the pixmap may have outlived. It is copied out first:
        // painting borrows the destination mutably.
        let tile = tile.and_then(|(owner, origin)| {
            self.window_tiles.get(&owner).map(|buffer| {
                (
                    buffer.bytes.as_ref().clone(),
                    buffer.size,
                    usize::try_from(buffer.stride).unwrap_or(0),
                    origin,
                )
            })
        });
        let handle = self.allocate_handle();
        let (buffer, _) = self.ensure(drawable, size, handle)?;
        match tile.as_ref() {
            Some((bytes, tile_size, tile_stride, origin)) => {
                let whole = Rect {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                };
                raster_ops::tile_solid(buffer, bytes, *tile_size, *tile_stride, *origin, whole);
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

    /// Keep a window's background tile as the pixmap is now. The protocol
    /// lets the pixmap be freed at once, and later drawing into it leaves
    /// the background undefined, so a copy is what the window keeps. The
    /// copy shares the pixmap's bytes until either is written.
    pub(crate) fn capture_window_tile(
        &mut self,
        window: XResourceId,
        pixmap: XResourceId,
        size: Size,
    ) {
        let handle = self.allocate_handle();
        let snapshot = match self.buffers.get(&pixmap) {
            Some(buffer) => buffer.clone(),
            None => match self.ensure(pixmap, size, handle) {
                Some((buffer, _)) => buffer.clone(),
                None => return,
            },
        };
        self.window_tiles.insert(window, snapshot);
    }

    pub(crate) fn forget_window_tile(&mut self, window: XResourceId) {
        self.window_tiles.remove(&window);
    }

    /// ClearArea on a window with a background tile: the area tiled from
    /// `origin`, with the tile captured for `owner`, the window the
    /// background comes from.
    pub(crate) fn clear_tiled(
        &mut self,
        drawable: XResourceId,
        size: Size,
        rect: Rect,
        (owner, origin): (XResourceId, (i32, i32)),
    ) -> Option<XAuthorityCpuDrawResult> {
        let tile = self.window_tiles.get(&owner)?.clone();
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        let stride = usize::try_from(tile.stride).unwrap_or(0);
        raster_ops::tile_solid(buffer, &tile.bytes, tile.size, stride, origin, rect);
        let published_damage = Some(rect);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    /// A resized window's buffer under its bit-gravity: a new buffer of
    /// `size`, with the old contents at `offset` when they are kept and
    /// nothing when they are discarded (t215). What the old contents do not
    /// cover is returned, for the caller to repaint and expose.
    pub(crate) fn relocate_window_contents(
        &mut self,
        window: XResourceId,
        size: Size,
        offset: Option<(i32, i32)>,
    ) -> Option<Vec<Rect>> {
        let whole = Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        };
        let previous = self.buffers.remove(&window);
        let handle = self.allocate_handle();
        let (buffer, _) = self.ensure(window, size, handle)?;
        let kept = match (previous, offset) {
            (Some(previous), Some((x, y))) => raster_ops::copy_buffer_region(
                &previous,
                buffer,
                Rect {
                    x: 0,
                    y: 0,
                    width: previous.size.width,
                    height: previous.size.height,
                },
                x,
                y,
            ),
            _ => None,
        };
        buffer.generation = buffer.generation.checked_add(1)?;
        self.note_export_damage(window, true, Some(whole));
        Some(match kept {
            Some(kept) => sophia_protocol::geometry::region_algebra::subtract(&[whole], &[kept]),
            None => vec![whole],
        })
    }

    /// Paint parts of a window with its background, as a repaint after a
    /// resize does: a pixel, or a tile from `origin` taken from `owner`.
    pub(crate) fn paint_background_rects(
        &mut self,
        window: XResourceId,
        size: Size,
        rects: &[Rect],
        pixel: u32,
        tile: Option<(XResourceId, (i32, i32))>,
    ) {
        for rect in rects {
            match tile {
                Some(tile) => {
                    self.clear_tiled(window, size, *rect, tile);
                }
                None => {
                    self.clear(window, size, *rect, pixel);
                }
            }
        }
    }
}

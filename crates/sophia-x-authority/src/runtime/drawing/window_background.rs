// What a window is painted with, and when. Split from drawing.rs for size;
// the impl block is the same runtime, included at the same scope.

impl XAuthorityRuntime {
    /// Sets what a window is painted with when it becomes viewable.
    pub fn set_window_background(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        background: crate::XWindowBackground,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_window_access(namespace, window)?;
        match background {
            crate::XWindowBackground::Pixmap(pixmap) => {
                let size = self.pixmap_size(namespace, pixmap)?;
                self.software_buffers.capture_window_tile(window, pixmap, size);
            }
            _ => self.software_buffers.forget_window_tile(window),
        }
        self.window_backgrounds.insert(window, background);
        Ok(())
    }

    /// The background actually used for a window, following ParentRelative up
    /// the tree, with the window it was taken from and that window's origin
    /// in this one's coordinates, which is where a tile is aligned. A chain
    /// that never resolves is undefined, which is what an unrooted
    /// ParentRelative means.
    fn resolved_background(
        &self,
        window: crate::XResourceId,
    ) -> (crate::XWindowBackground, crate::XResourceId, (i32, i32)) {
        let mut candidate = window;
        let mut origin = (0, 0);
        for _ in 0..64 {
            match self.window_backgrounds.get(&candidate) {
                None => return (crate::XWindowBackground::Undefined, candidate, origin),
                Some(crate::XWindowBackground::ParentRelative) => {
                    let Some(record) = self.windows.get(candidate) else {
                        return (crate::XWindowBackground::Undefined, candidate, origin);
                    };
                    if record.parent == candidate {
                        return (crate::XWindowBackground::Undefined, candidate, origin);
                    }
                    origin = (origin.0 - record.geometry.x, origin.1 - record.geometry.y);
                    candidate = record.parent;
                }
                Some(background) => return (*background, candidate, origin),
            }
        }
        (crate::XWindowBackground::Undefined, candidate, origin)
    }

    /// The window and every descendant of it that is viewable, parents first.
    pub(crate) fn viewable_subtree(&self, window: crate::XResourceId) -> Vec<crate::XResourceId> {
        let mut found = vec![window];
        let mut index = 0;
        // Bounded by the store, and each window is visited once because a
        // child is only ever reached from its own parent.
        while index < found.len() && index < 4096 {
            let parent = found[index];
            index += 1;
            for child in self.windows.direct_children_any_namespace(parent) {
                if self
                    .windows
                    .get(child)
                    .is_some_and(|record| record.map_state == crate::XMapState::Viewable)
                {
                    found.push(child);
                }
            }
        }
        found
    }

    /// Paints a window with its background, as becoming viewable requires.
    ///
    /// "When the window or one of its inferiors becomes viewable and no
    /// earlier contents for it are remembered, then the window is tiled with
    /// its background." We keep no earlier contents, so this applies every
    /// time. Silent when the window has no size or no backing yet: a window
    /// nobody can see owes nobody a repaint.
    pub(crate) fn paint_window_background(&mut self, window: crate::XResourceId) {
        let Some(record) = self.windows.get(window) else {
            return;
        };
        let size = Size {
            width: record.geometry.width,
            height: record.geometry.height,
        };
        let (background, owner, origin) = self.resolved_background(window);
        let (pixel, tile) = match background {
            // Undefined is not black: the window is not painted, and whatever
            // was on the screen underneath it shows through.
            crate::XWindowBackground::Undefined | crate::XWindowBackground::ParentRelative => {
                return;
            }
            crate::XWindowBackground::Pixel(pixel) => (pixel, None),
            crate::XWindowBackground::Pixmap(_) => (0, Some((owner, origin))),
        };
        self.software_buffers
            .paint_window_background(window, size, pixel, tile);
    }

    /// ClearArea: the area restored to the background the window has now --
    /// its pixel, or its tile from the origin it is aligned with -- and left
    /// alone where the background is None, as the protocol says.
    pub fn apply_clear_background(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        area: Rect,
    ) -> XAuthorityResponsePacket {
        if let Err(error) = self.validate_window_access(namespace, window) {
            return XAuthorityResponsePacket::rejected(transaction, error);
        }
        let (background, owner, origin) = self.resolved_background(window);
        match background {
            crate::XWindowBackground::Pixel(pixel) => {
                self.apply_clear_with_pixel(transaction, namespace, window, Region::single(area), pixel)
            }
            crate::XWindowBackground::Pixmap(_) => {
                let Some(record) = self.windows.get(window) else {
                    return XAuthorityResponsePacket::rejected(
                        transaction,
                        XAuthorityRuntimeError::UnknownResource,
                    );
                };
                let size = Size {
                    width: record.geometry.width,
                    height: record.geometry.height,
                };
                let generation = record.generation;
                let Some(buffer) =
                    self.software_buffers
                        .clear_tiled(window, size, area, (owner, origin))
                else {
                    return XAuthorityResponsePacket::accepted(transaction);
                };
                let handle = buffer.handle();
                // The journal holds no pattern pixels.
                self.pending_raster_command = Some(XAuthorityRasterCommand::Unsupported(
                    XRasterUnsupportedKind::FillPattern,
                ));
                self.finish_drawing_update(XDrawingUpdate::core_draw(
                    transaction,
                    namespace,
                    window,
                    handle,
                    Region::single(area),
                    generation,
                    250,
                ))
            }
            crate::XWindowBackground::Undefined | crate::XWindowBackground::ParentRelative => {
                XAuthorityResponsePacket::accepted(transaction)
            }
        }
    }
}

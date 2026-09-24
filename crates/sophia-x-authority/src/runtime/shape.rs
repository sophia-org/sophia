/// The three shapes a window may carry, each unset until a client sets it.
///
/// The tri-state per kind is the load-bearing part, and it is a lesson taken
/// from yserver rather than invented here. `None` means unset, and the
/// effective shape is then whatever the window's live geometry says; storing
/// the default instead would freeze an extent that stops tracking a resize,
/// which in their case blacked out a monitor after a mode change.
/// `Some(empty)` is a client explicitly asking for nothing -- a window that
/// draws nothing, or passes every click through -- and is a different answer
/// from unset in every reply this extension makes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XWindowShapeState {
    pub bounding: Option<Vec<Rect>>,
    /// Stored, reported, and consumed by nothing. Clip bounds where a window
    /// draws its own contents inside its bounding shape; Sophia composes
    /// whole client buffers and draws no window borders, so there is no
    /// point at which Bounding and Clip could differ visibly. Kept because
    /// a client that sets it expects to read it back.
    pub clip: Option<Vec<Rect>>,
    pub input: Option<Vec<Rect>>,
}

impl XWindowShapeState {
    fn kind(&self, kind: u8) -> &Option<Vec<Rect>> {
        match kind {
            crate::X_SHAPE_KIND_CLIP => &self.clip,
            crate::X_SHAPE_KIND_INPUT => &self.input,
            _ => &self.bounding,
        }
    }

    fn kind_mut(&mut self, kind: u8) -> &mut Option<Vec<Rect>> {
        match kind {
            crate::X_SHAPE_KIND_CLIP => &mut self.clip,
            crate::X_SHAPE_KIND_INPUT => &mut self.input,
            _ => &mut self.bounding,
        }
    }

    fn is_empty(&self) -> bool {
        self.bounding.is_none() && self.clip.is_none() && self.input.is_none()
    }
}

/// A shape change worth telling subscribers about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XShapeChange {
    pub window: crate::XResourceId,
    pub kind: u8,
    /// Whether the kind is set at all, which is what the protocol's `shaped`
    /// byte reports -- not whether the region has any area. A client that
    /// sets an empty shape has shaped its window.
    pub shaped: bool,
    pub extents: Rect,
}

/// Why a shape request was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XShapeError {
    UnknownWindow,
    UnknownPixmap,
    /// A kind, operation, or ordering the protocol does not define.
    InvalidValue,
    /// A mask source that is not a depth-1 pixmap.
    NotABitmap,
}

impl XAuthorityRuntime {
    /// Whether a kind names one of the three shapes, for callers outside
    /// this module that validate before they act.
    pub(crate) fn shape_kind_is_valid_public(kind: u8) -> bool {
        Self::shape_kind_is_valid(kind)
    }

    fn shape_kind_is_valid(kind: u8) -> bool {
        matches!(
            kind,
            crate::X_SHAPE_KIND_BOUNDING | crate::X_SHAPE_KIND_CLIP | crate::X_SHAPE_KIND_INPUT
        )
    }

    fn shape_op_is_valid(op: u8) -> bool {
        op <= crate::X_SHAPE_OP_INVERT
    }

    /// The window's own bounds, which every kind defaults to.
    ///
    /// The protocol's bounding default includes the border, as
    /// `(-bw, -bw, w + 2bw, h + 2bw)`. Sophia tracks no border width -- a
    /// window record has none and nothing draws one -- so every kind's
    /// default is the window rectangle. If borders ever arrive, this is the
    /// one place bounding has to diverge.
    fn shape_default_region(&self, window: crate::XResourceId) -> Option<Rect> {
        self.windows.get(window).map(|record| Rect {
            x: 0,
            y: 0,
            width: record.geometry.width,
            height: record.geometry.height,
        })
    }

    /// Whether the kind is set, and the rectangles it effectively covers.
    ///
    /// An unset kind reports the window's live bounds, so a resize moves the
    /// answer without anything having to write the store.
    pub fn effective_shape(&self, window: crate::XResourceId, kind: u8) -> (bool, Vec<Rect>) {
        let stored = self
            .window_shapes
            .get(&window)
            .and_then(|state| state.kind(kind).clone());
        match stored {
            Some(rects) => (true, rects),
            None => (
                false,
                self.shape_default_region(window)
                    .filter(|rect| !rect.is_empty())
                    .into_iter()
                    .collect(),
            ),
        }
    }

    /// Combine a source region into one of a window's shapes.
    ///
    /// Returns the change to announce, or `None` when the effective shape
    /// did not move. That gating is not an optimisation: window managers
    /// re-assert the same shape constantly -- yserver names xfwm4 hammering
    /// `ShapeMask(Input, None)` -- and a notify for every re-assertion broke
    /// panel buttons in their tree.
    pub(crate) fn combine_shape_region(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        kind: u8,
        op: u8,
        source: Vec<Rect>,
    ) -> Result<Option<XShapeChange>, XShapeError> {
        if !Self::shape_kind_is_valid(kind) || !Self::shape_op_is_valid(op) {
            return Err(XShapeError::InvalidValue);
        }
        self.validate_window_access(namespace, window)
            .map_err(|_| XShapeError::UnknownWindow)?;
        let (_, before) = self.effective_shape(window, kind);
        let stored = self
            .window_shapes
            .get(&window)
            .and_then(|state| state.kind(kind).clone());

        use sophia_protocol::geometry::region_algebra as region;
        let combined = match stored {
            // An operation against an unset kind has nothing to combine
            // with, so it becomes the source. Xorg does the same; the
            // alternative would be combining against a default the client
            // never asked for.
            None => region::canonicalize(&source),
            Some(current) => match op {
                crate::X_SHAPE_OP_SET => region::canonicalize(&source),
                crate::X_SHAPE_OP_UNION => region::union(&current, &source),
                crate::X_SHAPE_OP_INTERSECT => region::intersect(&current, &source),
                crate::X_SHAPE_OP_SUBTRACT => region::subtract(&current, &source),
                // Invert is the mirror of Subtract, not a complement: the
                // source with the destination taken out of it.
                _ => region::subtract(&source, &current),
            },
        };
        self.write_shape(window, kind, Some(combined), &before)
    }

    /// Return a kind to its default, so it tracks live geometry again.
    pub(crate) fn reset_shape(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        kind: u8,
    ) -> Result<Option<XShapeChange>, XShapeError> {
        if !Self::shape_kind_is_valid(kind) {
            return Err(XShapeError::InvalidValue);
        }
        self.validate_window_access(namespace, window)
            .map_err(|_| XShapeError::UnknownWindow)?;
        let (_, before) = self.effective_shape(window, kind);
        self.write_shape(window, kind, None, &before)
    }

    /// Move a shape, leaving an unset kind unset.
    pub(crate) fn offset_shape(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        kind: u8,
        dx: i32,
        dy: i32,
    ) -> Result<Option<XShapeChange>, XShapeError> {
        if !Self::shape_kind_is_valid(kind) {
            return Err(XShapeError::InvalidValue);
        }
        self.validate_window_access(namespace, window)
            .map_err(|_| XShapeError::UnknownWindow)?;
        let (_, before) = self.effective_shape(window, kind);
        let Some(current) = self
            .window_shapes
            .get(&window)
            .and_then(|state| state.kind(kind).clone())
        else {
            // Offsetting a shape the window does not have leaves it without
            // one, rather than materialising the default at an offset.
            return Ok(None);
        };
        let moved = sophia_protocol::geometry::region_algebra::translate(&current, dx, dy);
        self.write_shape(window, kind, Some(moved), &before)
    }

    /// Store a kind's new value and report whether the effective shape moved.
    fn write_shape(
        &mut self,
        window: crate::XResourceId,
        kind: u8,
        value: Option<Vec<Rect>>,
        before: &[Rect],
    ) -> Result<Option<XShapeChange>, XShapeError> {
        let was_set = self
            .window_shapes
            .get(&window)
            .is_some_and(|state| state.kind(kind).is_some());
        let now_set = value.is_some();
        {
            let state = self.window_shapes.entry(window).or_default();
            *state.kind_mut(kind) = value;
            if state.is_empty() {
                self.window_shapes.remove(&window);
            }
        }
        let (shaped, after) = self.effective_shape(window, kind);
        // Two ways to change: the area moved, or the kind went from unset to
        // set (or back) while covering the same area. The second still
        // changes what QueryExtents reports, so it is still a change.
        let area_moved = !sophia_protocol::geometry::region_algebra::regions_equal(before, &after);
        if !area_moved && was_set == now_set {
            return Ok(None);
        }
        Ok(Some(XShapeChange {
            window,
            kind,
            shaped,
            extents: sophia_protocol::geometry::region_algebra::extents(&after).unwrap_or_default(),
        }))
    }

    /// The rectangles covered by a depth-1 pixmap's set bits.
    ///
    /// Adapted from yserver's `bitmap_to_yx_banded_rects` (MIT, Copyright
    /// (c) 2026 Jos Dehaes): run-length encode each row, then merge adjacent
    /// rows carrying identical runs into bands. Sophia's store keeps every
    /// depth regardless of its width as one little-endian `u32` per pixel,
    /// so the bit under test is the low one rather than a packed bitmap row.
    pub(crate) fn shape_mask_rects(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<Vec<Rect>, XShapeError> {
        let (size, depth) = self
            .pixmap_geometry(namespace, pixmap)
            .map_err(|_| XShapeError::UnknownPixmap)?;
        if depth != 1 {
            return Err(XShapeError::NotABitmap);
        }
        let region = Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        };
        // A pixmap nothing has drawn into has no backing yet, and its
        // contents are undefined until something does. Reading it as empty
        // is the answer that cannot surprise anyone.
        let Some(bytes) = self.software_buffers.image_region(pixmap, region) else {
            return Ok(Vec::new());
        };
        let width = usize::try_from(size.width).unwrap_or(0);
        let height = usize::try_from(size.height).unwrap_or(0);
        let mut rects: Vec<Rect> = Vec::new();
        let mut previous_runs: Vec<(i32, i32)> = Vec::new();
        let mut band_top = 0i32;
        for y in 0..height {
            let mut runs: Vec<(i32, i32)> = Vec::new();
            let mut run_start: Option<usize> = None;
            for x in 0..width {
                let offset = (y * width + x) * 4;
                let set = bytes
                    .get(offset..offset + 4)
                    .map(|slot| u32::from_le_bytes(slot.try_into().unwrap_or([0; 4])) & 1 != 0)
                    .unwrap_or(false);
                match (set, run_start) {
                    (true, None) => run_start = Some(x),
                    (false, Some(start)) => {
                        runs.push((start as i32, x as i32));
                        run_start = None;
                    }
                    _ => {}
                }
            }
            if let Some(start) = run_start {
                runs.push((start as i32, width as i32));
            }
            let y = y as i32;
            if runs != previous_runs {
                for (start, end) in &previous_runs {
                    rects.push(Rect {
                        x: *start,
                        y: band_top,
                        width: end - start,
                        height: y - band_top,
                    });
                }
                previous_runs = runs;
                band_top = y;
            }
        }
        for (start, end) in &previous_runs {
            rects.push(Rect {
                x: *start,
                y: band_top,
                width: end - start,
                height: height as i32 - band_top,
            });
        }
        Ok(sophia_protocol::geometry::region_algebra::canonicalize(
            &rects,
        ))
    }

    /// Record or clear a client's interest in a window's shape changes.
    pub(crate) fn select_shape_input(
        &mut self,
        namespace: NamespaceId,
        client: u64,
        window: crate::XResourceId,
        enable: bool,
    ) -> Result<(), XShapeError> {
        self.validate_window_access(namespace, window)
            .map_err(|_| XShapeError::UnknownWindow)?;
        if enable {
            self.shape_selections.insert((client, window));
        } else {
            self.shape_selections.remove(&(client, window));
        }
        Ok(())
    }

    pub(crate) fn shape_input_selected(
        &self,
        namespace: NamespaceId,
        client: u64,
        window: crate::XResourceId,
    ) -> Result<bool, XShapeError> {
        self.validate_window_access(namespace, window)
            .map_err(|_| XShapeError::UnknownWindow)?;
        Ok(self.shape_selections.contains(&(client, window)))
    }

    /// Drop everything a window's shapes and subscriptions held.
    pub(crate) fn forget_window_shapes(&mut self, window: crate::XResourceId) {
        self.window_shapes.remove(&window);
        self.shape_selections.retain(|(_, id)| *id != window);
    }
}

impl XAuthorityRuntime {
    /// Republish a window whose bounding shape moved.
    ///
    /// Drawing is what normally carries a shape to the screen, and a client
    /// that changes its shape without drawing again would otherwise keep the
    /// old outline until something else happened to it. The damage is the
    /// whole window because the change can both add and remove covered area,
    /// and the area it removed has to be recomposed from whatever is beneath.
    pub(crate) fn republish_shaped_window(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Option<XAuthorityResponsePacket> {
        let record = self.windows.get(window)?;
        let generation = record.generation;
        let size = Size {
            width: record.geometry.width,
            height: record.geometry.height,
        };
        if size.width <= 0 || size.height <= 0 {
            return None;
        }
        // Only a window that has already presented something can be
        // republished; one that has never drawn has nothing to reshape.
        if !self.software_buffers.has_cpu_backing(window) {
            return None;
        }
        let shape = match self.effective_shape(window, crate::X_SHAPE_KIND_BOUNDING) {
            (true, rects) => Some(rects),
            (false, _) => None,
        };
        let whole = Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        };
        let update = self.software_buffers.present_window_damage(
            window,
            size,
            window,
            0,
            0,
            &[whole],
            shape.as_deref(),
            &crate::XPresentStacking::default(),
        )?;
        let handle = update.handle();
        // The composed raster has no journal representation, so density
        // variants fall back to scaling the 1x raster, as they do for every
        // other operation that reaches pixels this way.
        self.pending_raster_command = Some(XAuthorityRasterCommand::Unsupported(
            XRasterUnsupportedKind::RenderOperation,
        ));
        Some(self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            Region::single(whole),
            generation,
            250,
        )))
    }
}

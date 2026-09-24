// A window's win-gravity: where a child goes when its parent is resized
// (t199). Included by runtime.rs beside the other window paths, so it
// shares their imports.

/// `UnmapGravity`: the child is unmapped rather than moved.
pub const X_UNMAP_GRAVITY: u8 = 0;
/// `NorthWestGravity`, the default: the child keeps its position.
pub const X_NORTH_WEST_GRAVITY: u8 = 1;
/// `StaticGravity`: the child keeps its place on the screen.
pub const X_STATIC_GRAVITY: u8 = 10;

/// `ForgetGravity`: a resized window's contents are discarded.
pub const X_FORGET_GRAVITY: u8 = 0;

/// What a parent's resize did to one of its children.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XGravityOutcome {
    /// The child moved to this position in its parent; a GravityNotify is
    /// owed to it and to its parent.
    Moved { x: i32, y: i32 },
    /// The child has UnmapGravity and is to be unmapped.
    Unmap,
}

/// dix's `GravityTranslate`: where a child at `(x, y)` goes when its parent
/// grows by `(dw, dh)` and moves by `(dx, dy)`.
pub fn win_gravity_translate(gravity: u8, (x, y): (i32, i32), (dw, dh): (i32, i32), (dx, dy): (i32, i32)) -> (i32, i32) {
    match gravity {
        2 => (x + dw / 2, y),          // North
        3 => (x + dw, y),              // NorthEast
        4 => (x, y + dh / 2),          // West
        5 => (x + dw / 2, y + dh / 2), // Center
        6 => (x + dw, y + dh / 2),     // East
        7 => (x, y + dh),              // SouthWest
        8 => (x + dw / 2, y + dh),     // South
        9 => (x + dw, y + dh),         // SouthEast
        X_STATIC_GRAVITY => (x - dx, y - dy),
        _ => (x, y),
    }
}

impl XAuthorityRuntime {
    /// Record a window's win-gravity.
    pub fn set_window_gravity(&mut self, window: crate::XResourceId, gravity: u8) {
        if gravity == X_NORTH_WEST_GRAVITY {
            self.window_gravities.remove(&window);
        } else {
            self.window_gravities.insert(window, gravity);
        }
    }

    pub fn window_gravity(&self, window: crate::XResourceId) -> u8 {
        self.window_gravities
            .get(&window)
            .copied()
            .unwrap_or(X_NORTH_WEST_GRAVITY)
    }

    /// dix's `ResizeChildrenWinSize`: move each child of a resized parent by
    /// its win-gravity, topmost first, and say what happened to each child
    /// that moved or is to be unmapped. A resize of no size moves nothing:
    /// gravity answers a change of size, and a moved parent carries its
    /// children with it.
    pub fn apply_win_gravity(
        &mut self,
        namespace: NamespaceId,
        parent: crate::XResourceId,
        before: Rect,
        after: Rect,
        generation: u64,
    ) -> Vec<(crate::XResourceId, XGravityOutcome)> {
        let grown = (after.width - before.width, after.height - before.height);
        if grown == (0, 0) {
            return Vec::new();
        }
        let moved = (after.x - before.x, after.y - before.y);
        let mut outcomes = Vec::new();
        for child in self
            .windows
            .direct_children_bottom_to_top(namespace, parent)
            .into_iter()
            .rev()
        {
            let gravity = self.window_gravity(child);
            let Some(record) = self.windows.get(child) else {
                continue;
            };
            if gravity == X_UNMAP_GRAVITY {
                if record.map_state != crate::XMapState::Unmapped {
                    outcomes.push((child, XGravityOutcome::Unmap));
                }
                continue;
            }
            let (x, y) = (record.geometry.x, record.geometry.y);
            let (nx, ny) = win_gravity_translate(gravity, (x, y), grown, moved);
            if (nx, ny) == (x, y) {
                continue;
            }
            let update = XWindowGeometryUpdate {
                x: i16::try_from(nx).ok(),
                y: i16::try_from(ny).ok(),
                width: None,
                height: None,
                generation,
            };
            if self.configure_window_geometry(namespace, child, update).is_ok() {
                outcomes.push((child, XGravityOutcome::Moved { x: nx, y: ny }));
            }
        }
        outcomes
    }

    /// Record a window's bit-gravity.
    pub fn set_window_bit_gravity(&mut self, window: crate::XResourceId, gravity: u8) {
        if gravity == X_FORGET_GRAVITY {
            self.window_bit_gravities.remove(&window);
        } else {
            self.window_bit_gravities.insert(window, gravity);
        }
    }

    pub fn window_bit_gravity(&self, window: crate::XResourceId) -> u8 {
        self.window_bit_gravities
            .get(&window)
            .copied()
            .unwrap_or(X_FORGET_GRAVITY)
    }

    /// A resized window's contents under its bit-gravity (t215): kept where
    /// the gravity places them, or discarded under ForgetGravity, and what is
    /// newly uncovered painted with the window's background. The uncovered
    /// rectangles are returned for the caller to expose and present; nothing
    /// for a window that did not change size, is InputOnly, or has never
    /// been drawn.
    pub(crate) fn apply_bit_gravity(&mut self, window: crate::XResourceId, before: Rect, after: Rect) -> Vec<Rect> {
        let grown = (after.width - before.width, after.height - before.height);
        if grown == (0, 0) || self.window_is_input_only(window) || !self.software_buffers.has_backing(window) {
            return Vec::new();
        }
        let gravity = self.window_bit_gravity(window);
        let offset = (gravity != X_FORGET_GRAVITY).then(|| {
            win_gravity_translate(gravity, (0, 0), grown, (after.x - before.x, after.y - before.y))
        });
        let size = Size {
            width: after.width,
            height: after.height,
        };
        let Some(uncovered) = self.software_buffers.relocate_window_contents(window, size, offset) else {
            return Vec::new();
        };
        let (background, owner, origin) = self.resolved_background(window);
        match background {
            crate::XWindowBackground::Pixel(pixel) => {
                self.software_buffers.paint_background_rects(window, size, &uncovered, pixel, None);
            }
            crate::XWindowBackground::Pixmap(_) => {
                self.software_buffers
                    .paint_background_rects(window, size, &uncovered, 0, Some((owner, origin)));
            }
            crate::XWindowBackground::Undefined | crate::XWindowBackground::ParentRelative => {}
        }
        uncovered
    }

    /// A resize's effect on the window's own contents, presented: the
    /// uncovered rectangles to expose, and the presentation of the whole
    /// window when anything was redrawn.
    pub(crate) fn resize_window_contents(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        before: Rect,
        after: Rect,
    ) -> (Vec<Rect>, Option<XAuthorityResponsePacket>) {
        let uncovered = self.apply_bit_gravity(window, before, after);
        if uncovered.is_empty() {
            return (uncovered, None);
        }
        let Some(record) = self.windows.get(window) else {
            return (uncovered, None);
        };
        let generation = record.generation;
        let Some(handle) = self.software_buffers.buffer_handle(window) else {
            return (uncovered, None);
        };
        // Moved and repainted pixels have no journal representation.
        self.pending_raster_command = Some(XAuthorityRasterCommand::Unsupported(
            XRasterUnsupportedKind::RenderOperation,
        ));
        let whole = Rect {
            x: 0,
            y: 0,
            width: after.width,
            height: after.height,
        };
        let packet = self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            Region::single(whole),
            generation,
            250,
        ));
        (uncovered, Some(packet))
    }
}

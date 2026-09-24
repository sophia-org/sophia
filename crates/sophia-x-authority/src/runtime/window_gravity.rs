// A window's win-gravity: where a child goes when its parent is resized
// (t199). Included by runtime.rs beside the other window paths, so it
// shares their imports.

/// `UnmapGravity`: the child is unmapped rather than moved.
pub const X_UNMAP_GRAVITY: u8 = 0;
/// `NorthWestGravity`, the default: the child keeps its position.
pub const X_NORTH_WEST_GRAVITY: u8 = 1;
/// `StaticGravity`: the child keeps its place on the screen.
pub const X_STATIC_GRAVITY: u8 = 10;

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
}

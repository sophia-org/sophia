/// The pointer cursor a window asks for, and what the pointer is showing.
///
/// A cursor attribute changes no pixels this authority owns and produces no
/// surface transaction: it is a statement about what the pointer should look
/// like inside a window, kept so the question can be answered. Rendering it
/// is a separate obligation and belongs to whoever draws the pointer.
///
/// `None` is a real value and not an absence of information. A window with no
/// cursor of its own shows its parent's, so the answer to what a window shows
/// is found by walking towards the root, and a tree with no cursor anywhere
/// shows none -- which is a thing XTEST can ask about and be told.
impl XAuthorityRuntime {
    /// Set or clear a window's own cursor.
    ///
    /// Zero is `None` in the attribute, meaning this window stops having one
    /// of its own and goes back to showing its parent's, so it removes rather
    /// than storing a zero. Anything else has to be a cursor this namespace
    /// can reach, checked here, because an attribute that named a freed or
    /// foreign id would answer questions about a cursor nobody owns.
    pub fn set_window_cursor(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        cursor: u32,
    ) -> Result<(), XAuthorityRuntimeError> {
        if window.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            self.resources
                .lookup(namespace, window, XResourceKind::Window)?;
        }
        if cursor == 0 {
            self.window_cursors.remove(&window);
            return Ok(());
        }
        let cursor = crate::XResourceId::new(u64::from(cursor), 1);
        self.validate_cursor_access(namespace, cursor)?;
        self.window_cursors.insert(window, cursor);
        Ok(())
    }

    /// What a window shows: its own cursor, or the nearest ancestor's.
    ///
    /// Bounded by the same depth the rest of the tree walks use. A cycle
    /// cannot be built through the window table, but a bound costs nothing
    /// and makes that a property of this function rather than of its callers.
    pub fn window_effective_cursor(
        &self,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Option<crate::XResourceId> {
        let root = crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
        let mut candidate = window;
        for _ in 0..64 {
            if let Some(cursor) = self.window_cursors.get(&candidate) {
                return Some(*cursor);
            }
            if candidate == root {
                return None;
            }
            let record = self.windows.get(candidate)?;
            if record.namespace != namespace {
                return None;
            }
            candidate = record.parent;
        }
        None
    }

    /// What the pointer is showing, from where it is.
    ///
    /// The deepest mapped window containing the point decides, which is the
    /// same rule that decides which window the pointer is in. Children are
    /// visited top of the stack first, so an overlapping sibling shows its
    /// own cursor rather than the one beneath it.
    pub fn cursor_under_point(
        &self,
        namespace: NamespaceId,
        root_x: i32,
        root_y: i32,
    ) -> Option<crate::XResourceId> {
        let root = crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
        let mut deepest = root;
        for _ in 0..64 {
            let mut descended = None;
            for child in self
                .windows
                .direct_children_bottom_to_top(namespace, deepest)
                .into_iter()
                .rev()
            {
                let Some(record) = self.windows.get(child) else {
                    continue;
                };
                if record.map_state != crate::XMapState::Viewable {
                    continue;
                }
                let Some((origin_x, origin_y)) = self.window_root_position(child) else {
                    continue;
                };
                let local_x = root_x - origin_x;
                let local_y = root_y - origin_y;
                if local_x >= 0
                    && local_y >= 0
                    && local_x < record.geometry.width
                    && local_y < record.geometry.height
                {
                    descended = Some(child);
                    break;
                }
            }
            match descended {
                Some(child) => deepest = child,
                None => break,
            }
        }
        self.window_effective_cursor(namespace, deepest)
    }

    /// Forget a cursor attribute whose window or cursor has gone.
    pub(crate) fn release_window_cursor(&mut self, window: crate::XResourceId) {
        self.window_cursors.remove(&window);
    }

    /// Forget every attribute naming a cursor that has been freed, so a later
    /// id reusing its number is never mistaken for it.
    pub(crate) fn release_cursor_attribute_uses(&mut self, cursor: crate::XResourceId) {
        self.window_cursors.retain(|_, named| *named != cursor);
    }
}

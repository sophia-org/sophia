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
        self.window_backgrounds.insert(window, background);
        Ok(())
    }

    /// The background actually used for a window, following ParentRelative up
    /// the tree. A chain that never resolves is undefined, which is what an
    /// unrooted ParentRelative means.
    fn resolved_background(&self, window: crate::XResourceId) -> crate::XWindowBackground {
        let mut candidate = window;
        for _ in 0..64 {
            match self.window_backgrounds.get(&candidate) {
                None => return crate::XWindowBackground::Undefined,
                Some(crate::XWindowBackground::ParentRelative) => {
                    let Some(parent) = self.windows.get(candidate).map(|record| record.parent)
                    else {
                        return crate::XWindowBackground::Undefined;
                    };
                    if parent == candidate {
                        return crate::XWindowBackground::Undefined;
                    }
                    candidate = parent;
                }
                Some(background) => return *background,
            }
        }
        crate::XWindowBackground::Undefined
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
        let (pixel, tile) = match self.resolved_background(window) {
            // Undefined is not black: the window is not painted, and whatever
            // was on the screen underneath it shows through.
            crate::XWindowBackground::Undefined | crate::XWindowBackground::ParentRelative => {
                return;
            }
            crate::XWindowBackground::Pixel(pixel) => (pixel, None),
            crate::XWindowBackground::Pixmap(pixmap) => (0, Some(pixmap)),
        };
        self.software_buffers
            .paint_window_background(window, size, pixel, tile);
    }
}

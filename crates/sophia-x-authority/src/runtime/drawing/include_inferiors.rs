// A graphics context's IncludeInferiors subwindow-mode, and the subtree walk
// presentation shares with it. Included by runtime.rs beside the other
// drawing paths, so it shares their imports.

/// One mapped window of a subtree, in the order the screen paints them.
#[derive(Clone, Copy, Debug)]
struct XPaintedWindow {
    /// Where the window sits and what its ancestors leave of it, in the
    /// subtree top's coordinates.
    layer: crate::XPresentLayer,
    /// 0 for the top's own children.
    depth: usize,
}

impl XAuthorityRuntime {
    /// The mapped windows under `top` in painting order: depth first and
    /// bottom to top, as `composite_inferiors` reads them back, each clipped
    /// by its ancestors and by `top_clip`.
    ///
    /// A window counts once it is mapped, not only once it is viewable, so a
    /// draw before the toplevel maps still leaves its mapped children on top
    /// for when it does.
    fn painted_subtree(
        &self,
        namespace: NamespaceId,
        top: crate::XResourceId,
        top_clip: Rect,
    ) -> Vec<XPaintedWindow> {
        let children = |parent, x, y, clip, depth| {
            self.windows
                .direct_children_bottom_to_top(namespace, parent)
                .into_iter()
                .rev()
                .map(move |child| (child, x, y, clip, depth))
        };
        let mut stack: Vec<(crate::XResourceId, i32, i32, Rect, usize)> =
            children(top, 0, 0, top_clip, 0).collect();
        let mut painted = Vec::new();
        // Bounded: the store is finite and each window is reached from its
        // own parent exactly once.
        let mut visited = 0usize;
        while let Some((window, parent_x, parent_y, parent_clip, depth)) = stack.pop() {
            visited += 1;
            if visited > 4096 {
                break;
            }
            let Some(record) = self.windows.get(window) else {
                continue;
            };
            if record.map_state == crate::XMapState::Unmapped {
                continue;
            }
            let x = parent_x.saturating_add(record.interior_geometry().x);
            let y = parent_y.saturating_add(record.interior_geometry().y);
            let bounds = Rect {
                x,
                y,
                width: record.geometry.width,
                height: record.geometry.height,
            };
            let Some(clip) = crate::software::intersect_rects(bounds, parent_clip) else {
                continue;
            };
            painted.push(XPaintedWindow {
                layer: crate::XPresentLayer { window, x, y, clip },
                depth,
            });
            stack.extend(children(window, x, y, clip, depth + 1));
        }
        painted
    }

    /// Run one draw with the graphics context's subwindow-mode applied.
    ///
    /// ClipByChildren, the default, is what every draw path already does:
    /// each window has its own buffer, so a draw on a parent never touches a
    /// child's pixels. IncludeInferiors draws through the mapped inferiors
    /// instead. The target's buffer is first given what is on screen over
    /// it -- each inferior's pixels, bottom to top -- then the draw runs as
    /// usual, and whatever it left over an inferior is copied back into that
    /// inferior. Only the drawing namespace's own windows are inferiors: on
    /// the root, which is private to the namespace, that is the namespace's
    /// toplevels, and each one the draw reached is presented again (t181).
    ///
    /// The target's own pixels under its inferiors are overwritten by the
    /// composition. They are hidden there, and on a real screen they do not
    /// exist at all: the framebuffer holds the inferior's pixels instead.
    pub(crate) fn draw_through_inferiors(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
        subwindow_mode: u8,
        draw: impl FnOnce(&mut Self) -> XAuthorityResponsePacket,
    ) -> XAuthorityResponsePacket {
        if subwindow_mode != crate::X_INCLUDE_INFERIORS {
            return draw(self);
        }
        let root = is_root(drawable);
        if !root && self.windows.get(drawable).is_none() {
            return draw(self);
        }
        let Ok((key, size, _)) = self.draw_target(namespace, drawable) else {
            return draw(self);
        };
        let whole = Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        };
        let painted = self.painted_subtree(namespace, drawable, whole);
        let layers = painted.iter().map(|window| window.layer).collect::<Vec<_>>();
        if layers.is_empty()
            || self
                .software_buffers
                .compose_inferiors(key, size, &layers)
                .is_none()
        {
            return draw(self);
        }
        self.drawing_through = Some(key);
        let mut response = draw(self);
        self.drawing_through = None;
        let sized = layers
            .iter()
            .filter_map(|layer| {
                let record = self.windows.get(layer.window)?;
                Some((
                    *layer,
                    Size {
                        width: record.geometry.width,
                        height: record.geometry.height,
                    },
                ))
            })
            .collect::<Vec<_>>();
        let changed = self.software_buffers.scatter_to_inferiors(key, &sized);
        // A window's draw presented its toplevel, inferiors included, from
        // its own buffer. The private root is never presented, so each
        // window the draw reached through it is presented on its own.
        if root {
            for (window, damage) in changed {
                let Some(generation) = self.windows.get(window).map(|record| record.generation)
                else {
                    continue;
                };
                let Some(handle) = self.software_buffers.buffer_handle(window) else {
                    continue;
                };
                // The composed raster has no journal representation.
                self.pending_raster_command = Some(XAuthorityRasterCommand::Unsupported(
                    XRasterUnsupportedKind::RenderOperation,
                ));
                let presented = self.finish_drawing_update(XDrawingUpdate::core_draw(
                    transaction,
                    namespace,
                    window,
                    handle,
                    Region::single(damage),
                    generation,
                    250,
                ));
                response.surfaces.extend(presented.surfaces);
                response.removed_surfaces.extend(presented.removed_surfaces);
                response.transactions.extend(presented.transactions);
                response.portal_commands.extend(presented.portal_commands);
                response
                    .selection_artifacts
                    .extend(presented.selection_artifacts);
            }
        }
        response
    }
}

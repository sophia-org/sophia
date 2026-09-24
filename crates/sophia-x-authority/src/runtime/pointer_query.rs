#[derive(Clone, Copy, Debug)]
pub(crate) struct XPointerQuery {
    pub child: crate::XResourceId,
    pub root_x: i16,
    pub root_y: i16,
    pub win_x: i16,
    pub win_y: i16,
    pub mask: u16,
}

impl XAuthorityRuntime {
    // Serialize anchor adjustment with input publication, not with socket I/O.
    // A stationary pointer must not move along with a reconfigured X window.
    fn change_pointer_anchor_geometry<T>(
        &mut self,
        namespace: NamespaceId,
        change: impl FnOnce(&mut Self) -> Result<T, XAuthorityRuntimeError>,
    ) -> Result<T, XAuthorityRuntimeError> {
        let shared = self.input_authority.clone();
        let mut authority = shared.lock().expect("X11 input authority lock poisoned");
        let anchor = authority
            .pointer_query_state(namespace)
            .position
            .map(|pointer| pointer.surface_window);
        let old = anchor.and_then(|window| self.window_root_position(window));
        let result = change(self)?;
        if let Some((old, new)) =
            old.zip(anchor.and_then(|window| self.window_root_position(window)))
        {
            authority.shift_query_anchor(namespace, old, new);
        }
        Ok(result)
    }
    /// Moves the pointer, as WarpPointer defines the move.
    ///
    /// The whole request is served except the events it owes: a warp must
    /// generate motion and crossing events as if the user had moved the
    /// pointer, and this authority has no path from a request to the input
    /// fan-out, which only real input drives. So the position moves and
    /// QueryPointer agrees with it, and the events are a named gap rather
    /// than a silent one. Refusing to move at all would be the larger lie,
    /// since moving the pointer is what the request is for.
    /// The topmost viewable toplevel containing a root position, on an
    /// instance where clients place their own toplevels and nothing else
    /// stacks them. Elsewhere `None`: which toplevel a point falls in is the
    /// Engine's answer, and the authority does not invent one.
    pub fn client_placed_toplevel_at(
        &self,
        namespace: NamespaceId,
        x: i32,
        y: i32,
    ) -> Option<crate::XResourceId> {
        if !self.client_places_toplevels {
            return None;
        }
        let root = crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
        self.windows
            .direct_children_bottom_to_top(namespace, root)
            .into_iter()
            .rev()
            .find(|toplevel| self.pointer_window_contains(namespace, *toplevel, (x, y)))
    }

    /// Where the pointer is, as QueryPointer reports it, in root
    /// coordinates; `None` before any motion or warp placed it.
    pub fn pointer_query_position(&self, namespace: NamespaceId) -> Option<(i16, i16)> {
        self.input_authority_mut()
            .pointer_query_state(namespace)
            .position
            .map(|pointer| (pointer.root_x, pointer.root_y))
    }

    pub fn warp_pointer(
        &mut self,
        namespace: NamespaceId,
        source: crate::XResourceId,
        destination: crate::XResourceId,
        src_x: i16,
        src_y: i16,
        src_width: u16,
        src_height: u16,
        dst_x: i16,
        dst_y: i16,
    ) -> Result<(), XAuthorityRuntimeError> {
        let root = crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
        // Either window may be absent, and an absent one is not a refusal.
        // A named one must exist, whichever of the two it is.
        for window in [source, destination] {
            if window.local.raw() != 0 && window != root {
                self.validate_window_access(namespace, window)?;
            }
        }
        let origin_of = |runtime: &Self, window: crate::XResourceId| -> Option<(i32, i32)> {
            if window == root {
                Some((0, 0))
            } else {
                runtime.window_root_position(window)
            }
        };
        let current = self
            .input_authority_mut()
            .pointer_query_state(namespace)
            .position;
        let here = current.map_or((0, 0), |pointer| {
            (i32::from(pointer.root_x), i32::from(pointer.root_y))
        });

        // A source window makes the warp conditional: it happens only if the
        // pointer is inside that window and inside the named rectangle. A
        // zero width or height means the rest of the window from the offset.
        if source.local.raw() != 0 {
            let Some((origin_x, origin_y)) = origin_of(self, source) else {
                return Ok(());
            };
            let Ok(geometry) = self.drawable_facts(namespace, source).map(|facts| facts.geometry)
            else {
                return Ok(());
            };
            let relative = (here.0 - origin_x, here.1 - origin_y);
            let left = i32::from(src_x);
            let top = i32::from(src_y);
            let right = if src_width == 0 {
                geometry.width
            } else {
                left.saturating_add(i32::from(src_width))
            };
            let bottom = if src_height == 0 {
                geometry.height
            } else {
                top.saturating_add(i32::from(src_height))
            };
            let inside = relative.0 >= left
                && relative.0 < right
                && relative.1 >= top
                && relative.1 < bottom
                && relative.0 >= 0
                && relative.1 >= 0
                && relative.0 < geometry.width
                && relative.1 < geometry.height;
            if !inside {
                return Ok(());
            }
        }

        // No destination window means the offset is from where the pointer
        // already is; a destination window means it is from that window.
        let target = if destination.local.raw() == 0 {
            (
                here.0.saturating_add(i32::from(dst_x)),
                here.1.saturating_add(i32::from(dst_y)),
            )
        } else {
            let Some((origin_x, origin_y)) = origin_of(self, destination) else {
                return Ok(());
            };
            (
                origin_x.saturating_add(i32::from(dst_x)),
                origin_y.saturating_add(i32::from(dst_y)),
            )
        };
        // The pointer cannot leave the screen, and the furthest it reaches is
        // one short of each dimension.
        let screen = self
            .drawable_facts(namespace, root)
            .map(|facts| facts.geometry)
            .map_err(|_| XAuthorityRuntimeError::UnknownResource)?;
        let clamp =
            |value: i32, limit: i32| value.clamp(0, limit.saturating_sub(1).max(0)) as i16;
        let placed = (
            clamp(target.0, screen.width),
            clamp(target.1, screen.height),
        );

        // Anchor the new position to the destination window when there is
        // one, so QueryPointer can still refine a child from it. Otherwise
        // carry the previous anchor along by the distance the pointer moved,
        // which is what keeps a relative warp consistent with where it was.
        let anchor = if destination.local.raw() != 0
            && destination != root
            && let Some(record) = self.windows.get(destination)
            && record.namespace == namespace
        {
            Some(crate::input_authority::XPointerObservation {
                surface_window: destination,
                surface: record.surface,
                root_x: placed.0,
                root_y: placed.1,
                local_x: i32::from(dst_x),
                local_y: i32::from(dst_y),
            })
        } else {
            current.map(|pointer| crate::input_authority::XPointerObservation {
                root_x: placed.0,
                root_y: placed.1,
                local_x: pointer
                    .local_x
                    .saturating_add(i32::from(placed.0) - here.0),
                local_y: pointer
                    .local_y
                    .saturating_add(i32::from(placed.1) - here.1),
                ..pointer
            })
        };
        let placed_anchor = anchor.unwrap_or(crate::input_authority::XPointerObservation {
            surface_window: crate::XResourceId::NONE,
            surface: sophia_protocol::SurfaceId::INVALID,
            root_x: placed.0,
            root_y: placed.1,
            local_x: 0,
            local_y: 0,
        });
        self.input_authority_mut()
            .warp_query_pointer(namespace, placed_anchor);
        Ok(())
    }

    pub(crate) fn query_pointer(
        &self,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Result<XPointerQuery, XAuthorityRuntimeError> {
        let root = crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
        if window != root {
            self.validate_window_access(namespace, window)?;
        }
        let state = self.input_authority_mut().pointer_query_state(namespace);
        let mut query = XPointerQuery {
            child: crate::XResourceId::NONE,
            root_x: 0,
            root_y: 0,
            win_x: 0,
            win_y: 0,
            mask: state.mask,
        };
        let Some(pointer) = state.position else {
            return Ok(query);
        };
        query.root_x = pointer.root_x;
        query.root_y = pointer.root_y;
        // Root coordinates describe Engine's output space. Within an X tree,
        // retain the local position Engine computed through visual transforms.
        let logical = self
            .windows
            .get(pointer.surface_window)
            .filter(|record| record.namespace == namespace && record.surface == pointer.surface)
            .and_then(|_| self.window_root_position(pointer.surface_window))
            .map_or(
                (i32::from(pointer.root_x), i32::from(pointer.root_y)),
                |(x, y)| {
                    (
                        x.saturating_add(pointer.local_x),
                        y.saturating_add(pointer.local_y),
                    )
                },
            );
        if window == root {
            query.win_x = pointer.root_x;
            query.win_y = pointer.root_y;
        } else if let Some((x, y)) = self.window_root_position(window) {
            let clamp = |value: i32| value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            query.win_x = clamp(logical.0.saturating_sub(x));
            query.win_y = clamp(logical.1.saturating_sub(y));
        }
        // Engine chose the surface. Only refine its X descendants; scanning
        // other top levels here would invent compositor hit-testing authority.
        if self
            .windows
            .get(pointer.surface_window)
            .is_none_or(|record| record.surface != pointer.surface)
            || !self.pointer_window_contains(namespace, pointer.surface_window, logical)
        {
            return Ok(query);
        }
        let mut deepest = pointer.surface_window;
        for _ in 0..64 {
            let child = self
                .windows
                .direct_children(namespace, deepest)
                .into_iter()
                .filter(|child| self.pointer_window_contains(namespace, *child, logical))
                .max_by_key(|child| {
                    self.windows
                        .get(*child)
                        .map(|record| (record.stack_rank, record.id))
                });
            let Some(child) = child else {
                break;
            };
            deepest = child;
        }
        let mut candidate = deepest;
        for _ in 0..64 {
            let Some(record) = self.windows.get(candidate) else {
                break;
            };
            if record.namespace != namespace || record.map_state != crate::XMapState::Viewable {
                break;
            }
            if record.parent == window {
                query.child = candidate;
                break;
            }
            if record.parent == root {
                break;
            }
            candidate = record.parent;
        }
        Ok(query)
    }

    /// The deepest window the pointer is inside, when one has been observed.
    ///
    /// Focus event generation needs it because a transition that crosses on
    /// or off the pointer's chain owes that chain its own events, and because
    /// a `PointerRoot` focus resolves through it. `None` means no pointer
    /// position has been observed at all, which the caller reads as the root.
    pub(crate) fn pointer_window(&self, namespace: NamespaceId) -> Option<crate::XResourceId> {
        let pointer = self
            .input_authority_mut()
            .pointer_query_state(namespace)
            .position?;
        let logical = self
            .window_root_position(pointer.surface_window)
            .map_or(
                (i32::from(pointer.root_x), i32::from(pointer.root_y)),
                |(x, y)| {
                    (
                        x.saturating_add(pointer.local_x),
                        y.saturating_add(pointer.local_y),
                    )
                },
            );
        if !self.pointer_window_contains(namespace, pointer.surface_window, logical) {
            return None;
        }
        // Engine chose the surface; only its own descendants are refined, for
        // the same reason `query_pointer` gives.
        let mut deepest = pointer.surface_window;
        for _ in 0..64 {
            let child = self
                .windows
                .direct_children(namespace, deepest)
                .into_iter()
                .filter(|child| self.pointer_window_contains(namespace, *child, logical))
                .max_by_key(|child| {
                    self.windows
                        .get(*child)
                        .map(|record| (record.stack_rank, record.id))
                });
            let Some(child) = child else {
                break;
            };
            deepest = child;
        }
        Some(deepest)
    }

    fn pointer_window_contains(
        &self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        point: (i32, i32),
    ) -> bool {
        let Some(record) = self.windows.get(window) else {
            return false;
        };
        if record.namespace != namespace || record.map_state != crate::XMapState::Viewable {
            return false;
        }
        let Some(origin) = self.window_root_position(window) else {
            return false;
        };
        let x = point.0.saturating_sub(origin.0);
        let y = point.1.saturating_sub(origin.1);
        if x < 0 || y < 0 || x >= record.geometry.width || y >= record.geometry.height {
            return false;
        }
        self.window_shapes.get(&window).is_none_or(|shape| {
            [&shape.bounding, &shape.input].into_iter().all(|region| {
                region.as_ref().is_none_or(|rects| {
                    sophia_protocol::geometry::region_algebra::contains_point(rects, x, y)
                })
            })
        })
    }
}

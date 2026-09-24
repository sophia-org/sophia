#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XDrawableImageDescriptor {
    pub size: Size,
    pub depth: u8,
    pub visual: u32,
    /// Pixmaps have no root-relative visibility requirement.
    pub root_position: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XDrawableImageError {
    Access(XAuthorityRuntimeError),
    BadMatch,
    AllocationFailed,
}

impl XAuthorityRuntime {
    pub(crate) fn drawable_image_descriptor(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<XDrawableImageDescriptor, XDrawableImageError> {
        if drawable.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            let size = self
                .output_topology
                .root_size()
                .map_err(|_| XDrawableImageError::BadMatch)?;
            return Ok(XDrawableImageDescriptor {
                size,
                depth: 24,
                visual: crate::X_SETUP_DEFAULT_VISUAL,
                root_position: Some((0, 0)),
            });
        }
        self.validate_drawable_access(namespace, drawable)
            .map_err(XDrawableImageError::Access)?;
        if let Ok(geometry) = self.window_geometry(namespace, drawable) {
            if self
                .window_map_state(namespace, drawable)
                .map_err(XDrawableImageError::Access)?
                != crate::XMapState::Viewable
            {
                return Err(XDrawableImageError::BadMatch);
            }
            let (depth, visual, _) = self.window_visual(drawable);
            return Ok(XDrawableImageDescriptor {
                size: Size {
                    width: geometry.width,
                    height: geometry.height,
                },
                depth,
                visual,
                root_position: Some(
                    self.window_absolute_position(namespace, drawable)
                        .map_err(XDrawableImageError::Access)?,
                ),
            });
        }
        let (size, depth) = self
            .pixmap_geometry(namespace, drawable)
            .map_err(XDrawableImageError::Access)?;
        Ok(XDrawableImageDescriptor {
            size,
            depth,
            visual: crate::X_ATOM_NONE,
            root_position: None,
        })
    }

    pub(crate) fn validate_drawable_image_region(
        &self,
        descriptor: XDrawableImageDescriptor,
        region: Rect,
    ) -> Result<(), XDrawableImageError> {
        if region.x < 0 || region.y < 0 || region.width < 0 || region.height < 0 {
            return Err(XDrawableImageError::BadMatch);
        }
        let right = region
            .x
            .checked_add(region.width)
            .ok_or(XDrawableImageError::BadMatch)?;
        let bottom = region
            .y
            .checked_add(region.height)
            .ok_or(XDrawableImageError::BadMatch)?;
        if right > descriptor.size.width || bottom > descriptor.size.height {
            return Err(XDrawableImageError::BadMatch);
        }
        if let Some((root_x, root_y)) = descriptor.root_position {
            let root_size = self
                .output_topology
                .root_size()
                .map_err(|_| XDrawableImageError::BadMatch)?;
            let root_right = root_x
                .checked_add(right)
                .ok_or(XDrawableImageError::BadMatch)?;
            let root_bottom = root_y
                .checked_add(bottom)
                .ok_or(XDrawableImageError::BadMatch)?;
            if root_x.checked_add(region.x).is_none_or(|x| x < 0)
                || root_y.checked_add(region.y).is_none_or(|y| y < 0)
                || root_right > root_size.width
                || root_bottom > root_size.height
            {
                return Err(XDrawableImageError::BadMatch);
            }
        }
        Ok(())
    }

    /// What a drawable is, for the requests that accept more than one kind.
    ///
    /// States facts and leaves admission to the caller, because the callers
    /// disagree: core drawing must refuse a drawable with no server storage,
    /// while a request that only names one may accept it.
    pub fn drawable_facts(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<XDrawableFacts, XAuthorityRuntimeError> {
        if drawable.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            let root = self
                .output_topology()
                .root_size()
                .map_err(|_| XAuthorityRuntimeError::UnknownResource)?;
            return Ok(XDrawableFacts {
                kind: XDrawableKind::Root,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: root.width,
                    height: root.height,
                },
                depth: 24,
            });
        }
        // The window error is the one a miss reports, so an unknown id keeps the
        // exact identity it had before this resolver existed.
        let window_error = match self.window_geometry(namespace, drawable) {
            Ok(geometry) => {
                return Ok(XDrawableFacts {
                    kind: XDrawableKind::Window,
                    geometry,
                    depth: self.window_visual(drawable).0,
                });
            }
            Err(error) => error,
        };
        if let Ok((size, depth)) = self.pixmap_geometry(namespace, drawable) {
            return Ok(XDrawableFacts {
                kind: XDrawableKind::Pixmap,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                },
                depth,
            });
        }
        // A pbuffer is its own X drawable: the client names the same id when it
        // asks for geometry, so this is the request that decides whether the
        // drawable it just created exists at all. Its depth comes from its
        // configuration, since there is no window to read one from.
        if let Ok((size, fbconfig)) = self.glx_pbuffer(namespace, drawable)
            && let Some(config) = crate::x_glx_fb_config(fbconfig, self.pixmap_textures_supported())
        {
            return Ok(XDrawableFacts {
                kind: XDrawableKind::GlxPbuffer,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                },
                depth: config.color_bits(),
            });
        }
        // A GLX pixmap answers for the pixels it wraps, so it reports the
        // pixmap's extent. Without this it is a drawable that cannot be made
        // current, which is the one thing it exists to be.
        if let Ok((size, depth)) = self.glx_pixmap_geometry(namespace, drawable) {
            return Ok(XDrawableFacts {
                kind: XDrawableKind::Pixmap,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                },
                depth,
            });
        }
        Err(window_error)
    }

    /// Drawables a client may name when it imports buffers it allocated itself.
    ///
    /// Wider than `validate_drawable_access` on purpose. DRI3 pixels are
    /// client-allocated: the client creates the image and asks the server to wrap
    /// its descriptors, so a drawable with no server storage is a legal target
    /// here. It is not one for core drawing, which is why that validator stays
    /// narrow and this one is named separately rather than widening it.
    pub fn validate_dri3_drawable_access(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.drawable_facts(namespace, drawable).map(|_| ())
    }

    pub fn validate_drawable_access(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        if drawable.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            return Ok(());
        }
        // The window `_NET_SUPPORTING_WM_CHECK` names exists, so requests that
        // ask about a window rather than draw into one must succeed against it:
        // a client selects events on it to learn if the manager dies, and an
        // error there reads as the manager having already gone. It is unmapped
        // and never composited, so drawing into it reaches nothing -- the same
        // as the check window a conventional manager creates.
        if drawable.local.raw() == u64::from(crate::X_SETUP_WM_CHECK_WINDOW) {
            return Ok(());
        }
        if !namespace.is_valid() {
            return Err(XAuthorityRuntimeError::InvalidNamespace);
        }
        let record = self
            .resources
            .get(drawable)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        if !matches!(record.kind, XResourceKind::Window | XResourceKind::Pixmap) {
            return Err(XAuthorityRuntimeError::WrongResourceKind);
        }
        if record.owner_namespace != namespace {
            return Err(XAuthorityRuntimeError::CrossNamespaceDenied);
        }
        Ok(())
    }

    pub(crate) fn drawable_depth(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<u8, XAuthorityRuntimeError> {
        self.validate_drawable_access(namespace, drawable)?;
        if drawable.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            return Ok(24);
        }
        if let Ok(depth) = self.pixmap_depth(namespace, drawable) {
            return Ok(depth);
        }
        // An InputOnly window has no pixels, so no context and no other
        // drawable can match it; depth zero is how the server says so.
        if self.window_is_input_only(drawable) {
            return Ok(0);
        }
        Ok(self.window_visual(drawable).0)
    }

    pub fn window_background_pixel(
        &self,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Result<u32, XAuthorityRuntimeError> {
        self.validate_window_access(namespace, window)?;
        Ok(match self.window_backgrounds.get(&window) {
            Some(crate::XWindowBackground::Pixel(pixel)) => *pixel,
            _ => 0,
        })
    }

    pub fn set_window_background_pixel(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        pixel: u32,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_window_access(namespace, window)?;
        self.window_backgrounds
            .insert(window, crate::XWindowBackground::Pixel(pixel));
        Ok(())
    }

    /// Where a core draw lands: the CPU buffer's key, its size, and the
    /// generation of the window to present afterwards, if any.
    ///
    /// The root is every namespace's parent and the Engine's to present, so
    /// a client drawing on it draws into a root private to its namespace:
    /// a buffer the size of the screen, read back by GetImage and never
    /// presented (t181).
    pub(crate) fn draw_target(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<(crate::XResourceId, Size, Option<u64>), XAuthorityRuntimeError> {
        self.validate_drawable_access(namespace, drawable)?;
        if is_root(drawable) {
            let size = self
                .output_topology()
                .root_size()
                .map_err(|_| XAuthorityRuntimeError::UnknownResource)?;
            return Ok((private_root_key(namespace), size, None));
        }
        if let Ok(size) = self.pixmap_size(namespace, drawable) {
            return Ok((drawable, size, None));
        }
        let record = self
            .windows
            .get(drawable)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        Ok((
            drawable,
            Size {
                width: record.geometry.width,
                height: record.geometry.height,
            },
            Some(record.generation),
        ))
    }

    /// The CPU buffer key a drawable's pixels live under: its own, or for the
    /// root, the namespace's private root.
    pub(crate) fn draw_key(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> crate::XResourceId {
        if is_root(drawable) {
            private_root_key(namespace)
        } else {
            drawable
        }
    }

    pub fn apply_core_draw(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        damage: Region,
    ) -> XAuthorityResponsePacket {
        self.apply_core_draw_with_gc(
            transaction,
            namespace,
            window,
            damage,
            &XGraphicsContextValues::default(),
        )
    }

    /// Paint spans, but report a single rectangle covering them.
    ///
    /// A filled polygon is many one-row spans, and sending each as its own
    /// damage rectangle makes a small triangle cost a dozen patches. Damage is
    /// a conservative over-approximation by definition, so the painting stays
    /// exact and the report is the bounding box.
    pub fn apply_span_fill(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        spans: &[Rect],
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        let Some(bounds) = bounding_rect(spans) else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let (window, size, window_generation) = match self.draw_target(namespace, window) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some(buffer) = self.software_buffers.paint_damage(window, size, spans, gc) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            );
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = buffer.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Paint {
            rects: spans.to_vec(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            Region::single(bounds),
            generation,
            250,
        ))
    }

    pub fn apply_core_draw_with_gc(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        damage: Region,
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        let (window, size, window_generation) = match self.draw_target(namespace, window) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some(buffer) = self
            .software_buffers
            .paint_damage(window, size, &damage.rects, gc)
        else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            );
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = buffer.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Paint {
            rects: damage.rects.clone(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            damage,
            generation,
            250,
        ))
    }

    /// Draw disjoint segments, each its own two-point line.
    ///
    /// Distinct from `apply_line_draw`, which draws one connected polyline:
    /// the segments here do not join, so consecutive ones share no vertex.
    pub fn apply_segment_draw(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        segments: &[(XPoint, XPoint)],
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        let (window, size, window_generation) = match self.draw_target(namespace, window) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some((update, damage)) = self
            .software_buffers
            .draw_segments(window, size, segments, gc)
        else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = update.handle();
        // Replayed as the polyline of each pair, so a density replay draws the
        // same disjoint segments rather than joining them into one path.
        self.pending_raster_command = Some(XAuthorityRasterCommand::Segments {
            points: segments
                .iter()
                .flat_map(|(from, to)| [*from, *to])
                .map(|point| XRasterPoint {
                    x: i32::from(point.x),
                    y: i32::from(point.y),
                })
                .collect(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            Region::single(damage),
            generation,
            250,
        ))
    }

    /// Stroke arcs as `miPolyArc` does. Density replay has no arc command,
    /// so it is given each arc's chords, as it was before the port: a
    /// derived store approximates the arc; the canonical drawable is exact.
    pub fn apply_arc_draw(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        arcs: &[crate::XArc],
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        let (window, size, window_generation) = match self.draw_target(namespace, window) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some((update, damage)) = self.software_buffers.draw_arcs(window, size, arcs, gc)
        else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = update.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Segments {
            points: arcs
                .iter()
                .flat_map(|arc| {
                    let chords = crate::software::geometry::arc::polyline(*arc);
                    chords
                        .windows(2)
                        .flat_map(|pair| [pair[0], pair[1]])
                        .collect::<Vec<_>>()
                })
                .map(|point| XRasterPoint {
                    x: i32::from(point.x),
                    y: i32::from(point.y),
                })
                .collect(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            Region::single(damage),
            generation,
            250,
        ))
    }

    pub fn apply_line_draw(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        points: &[XPoint],
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        let (window, size, window_generation) = match self.draw_target(namespace, window) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some(update) = self.software_buffers.draw_lines(window, size, points, gc) else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let damage = Region::single(Rect {
            x: points
                .iter()
                .map(|point| i32::from(point.x))
                .min()
                .unwrap_or(0),
            y: points
                .iter()
                .map(|point| i32::from(point.y))
                .min()
                .unwrap_or(0),
            width: points
                .iter()
                .map(|point| i32::from(point.x))
                .max()
                .unwrap_or(0)
                .saturating_sub(
                    points
                        .iter()
                        .map(|point| i32::from(point.x))
                        .min()
                        .unwrap_or(0),
                )
                .saturating_add(i32::from(gc.line_width.max(1))),
            height: points
                .iter()
                .map(|point| i32::from(point.y))
                .max()
                .unwrap_or(0)
                .saturating_sub(
                    points
                        .iter()
                        .map(|point| i32::from(point.y))
                        .min()
                        .unwrap_or(0),
                )
                .saturating_add(i32::from(gc.line_width.max(1))),
        });
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = update.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Lines {
            points: points
                .iter()
                .map(|point| XRasterPoint {
                    x: i32::from(point.x),
                    y: i32::from(point.y),
                })
                .collect(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            damage,
            generation,
            250,
        ))
    }

    pub fn apply_rectangle_draw(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        rectangles: &[Rect],
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        let (window, size, window_generation) = match self.draw_target(namespace, window) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let Some((update, damage)) = self
            .software_buffers
            .draw_rectangles(window, size, rectangles, gc)
        else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = update.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Rectangles {
            rectangles: rectangles.to_vec(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            Region::single(damage),
            generation,
            250,
        ))
    }

    pub(crate) fn apply_text_draw(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
        draws: &[XTextDraw<'_>],
        gc: &XGraphicsContextValues,
    ) -> XAuthorityResponsePacket {
        if let Err(error) = self.validate_drawable_access(namespace, drawable) {
            return XAuthorityResponsePacket::rejected(transaction, error);
        }
        if draws.iter().all(|draw| draw.text.is_empty()) {
            return XAuthorityResponsePacket::accepted(transaction);
        }
        let (drawable, size, window_generation) = match self.draw_target(namespace, drawable) {
            Ok(target) => target,
            Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
        };
        let mut damage = Region::empty();
        for draw in draws {
            if draw.text.is_empty() {
                continue;
            }
            let metrics = &draw.font.metrics;
            damage.push(Rect {
                x: draw.x,
                y: draw.baseline.saturating_sub(i32::from(metrics.font_ascent)),
                width: metrics.text_extents(draw.text).overall_width,
                height: i32::from(metrics.font_ascent.saturating_add(metrics.font_descent)),
            });
        }
        let Some(buffer) = self.software_buffers.draw_text(drawable, size, draws, gc) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            );
        };
        let Some(generation) = window_generation else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let handle = buffer.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Text {
            draws: draws
                .iter()
                .map(|draw| XOwnedTextDraw {
                    x: draw.x,
                    baseline: draw.baseline,
                    text: draw.text.to_vec(),
                    image: draw.image,
                    font: draw.font.clone(),
                })
                .collect(),
            gc: gc.clone(),
        });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            drawable,
            handle,
            damage,
            generation,
            250,
        ))
    }

    pub fn apply_clear(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        damage: Region,
    ) -> XAuthorityResponsePacket {
        self.apply_clear_with_pixel(transaction, namespace, window, damage, 0)
    }

    pub fn apply_clear_with_pixel(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        damage: Region,
        pixel: u32,
    ) -> XAuthorityResponsePacket {
        let Some(record) = self.windows.get(window) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::UnknownResource,
            );
        };
        let Some(rect) = damage.rects.first().copied() else {
            return XAuthorityResponsePacket::accepted(transaction);
        };
        let Some(buffer) = self.software_buffers.clear(
            window,
            Size {
                width: record.geometry.width,
                height: record.geometry.height,
            },
            rect,
            pixel,
        ) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            );
        };
        let handle = buffer.handle();
        self.pending_raster_command = Some(XAuthorityRasterCommand::Clear { rect, pixel });
        self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction,
            namespace,
            window,
            handle,
            damage,
            record.generation,
            250,
        ))
    }
}

/// The smallest rectangle covering every span.
fn bounding_rect(spans: &[Rect]) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for span in spans {
        if span.width <= 0 || span.height <= 0 {
            continue;
        }
        bounds = Some(match bounds {
            None => *span,
            Some(current) => {
                let left = current.x.min(span.x);
                let top = current.y.min(span.y);
                let right = (current.x + current.width).max(span.x + span.width);
                let bottom = (current.y + current.height).max(span.y + span.height);
                Rect {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                }
            }
        });
    }
    bounds
}

/// The root window's XID, which every namespace sees.
fn is_root(drawable: crate::XResourceId) -> bool {
    drawable.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT)
}

/// Private roots live above every XID and every retained-backing key: X11
/// names 32-bit resources, and retained backings count up from 2^32.
const PRIVATE_ROOT_KEY_BASE: u64 = 1 << 48;

/// The key of a namespace's private root. Created on the first draw, and
/// kept for the namespace's life, as the root's contents outlive any one
/// client.
pub(crate) fn private_root_key(namespace: NamespaceId) -> crate::XResourceId {
    crate::XResourceId::new(PRIVATE_ROOT_KEY_BASE + namespace.raw(), 1)
}

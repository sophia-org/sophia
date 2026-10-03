impl XAuthorityRuntime {
    fn execute_standard_pixmap_request(
        &mut self,
        request: XPreparedPresent,
    ) -> XAuthorityResponsePacket {
        let XPreparedPresent {
            transaction,
            namespace,
            window,
            pixmap,
            x_offset,
            y_offset,
            has_valid_region,
            has_update_region: _,
            source_damage,
            pixmap_size,
        } = request;
        let record = match self.windows.get(window) {
            Some(record) if record.namespace == namespace => record.clone(),
            _ => {
                return XAuthorityResponsePacket::rejected(
                    transaction,
                    XAuthorityRuntimeError::UnknownResource,
                );
            }
        };
        let damage = translated_present_damage(&source_damage, x_offset, y_offset);
        // Both storage paths publish the same compositor-facing owner. A child
        // is an X drawing target, not an independently managed desktop surface.
        let (target_window, _, child_x, child_y) =
            match self.window_presentation_root_and_offset(namespace, window) {
                Ok(presentation) => presentation,
                Err(error) => return XAuthorityResponsePacket::rejected(transaction, error),
            };
        let Some(presentation_record) = self.windows.get(target_window) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::UnknownResource,
            );
        };
        let target_generation = presentation_record.generation;
        let target_size = Size {
            width: presentation_record.geometry.width,
            height: presentation_record.geometry.height,
        };
        let drawing_extent = Size {
            width: record.geometry.width,
            height: record.geometry.height,
        };
        let (buffer, damage, presentation_extent, raster_extent) = if let Some(descriptor) = self
            .dri3_pixmaps
            .get(&pixmap)
            .or_else(|| {
                self.retained_pixmap_backings
                    .get(&pixmap)
                    .and_then(|record| record._dri3.as_ref())
            })
            .map(|record| record.descriptor)
        {
            (
                sophia_protocol::BufferSource::DmaBuf {
                    handle: descriptor.handle.raw(),
                },
                Region {
                    rects: damage
                        .rects
                        .into_iter()
                        .map(|rect| Rect {
                            x: rect.x.saturating_add(child_x),
                            y: rect.y.saturating_add(child_y),
                            ..rect
                        })
                        .collect(),
                },
                drawing_extent,
                pixmap_size,
            )
        } else {
            if let Some(binding) = self.shm_pixmaps.get(&pixmap).cloned().or_else(|| {
                self.retained_pixmap_backings
                    .get(&pixmap)
                    .and_then(|record| record._shm.clone())
            }) {
                let Some(stride) = usize::try_from(binding.size.width)
                    .ok()
                    .and_then(|width| width.checked_mul(4))
                else {
                    return XAuthorityResponsePacket::rejected(
                        transaction,
                        XAuthorityRuntimeError::InvalidResource,
                    );
                };
                if self
                    .software_buffers
                    .ensure_image_backing(pixmap, binding.size)
                    .is_none()
                {
                    return XAuthorityResponsePacket::rejected(
                        transaction,
                        XAuthorityRuntimeError::InvalidResource,
                    );
                }
                for rect in &source_damage {
                    let packed = usize::try_from(binding.offset).ok().and_then(|offset| {
                        let row_offset = usize::try_from(rect.x).ok()?.checked_mul(4)?;
                        let row_bytes = usize::try_from(rect.width).ok()?.checked_mul(4)?;
                        let rows = usize::try_from(rect.height).ok()?;
                        let source_y = usize::try_from(rect.y).ok()?.checked_mul(stride)?;
                        binding
                            .mapping
                            .copy_rows(
                                offset.checked_add(source_y)?,
                                stride,
                                row_offset,
                                row_bytes,
                                rows,
                            )
                            .ok()
                    });
                    if packed.as_ref().is_none_or(|bytes| {
                        self.software_buffers
                            .put_image_backing(pixmap, binding.size, *rect, bytes)
                            .is_none()
                    }) {
                        return XAuthorityResponsePacket::rejected(
                            transaction,
                            XAuthorityRuntimeError::InvalidResource,
                        );
                    }
                }
            }
            let shape = match self.effective_shape(target_window, crate::X_SHAPE_KIND_BOUNDING) {
                (true, rects) => Some(rects),
                (false, _) => None,
            };
            let Some(update) = self.software_buffers.present_window_damage(
                target_window,
                target_size,
                pixmap,
                child_x.saturating_add(i32::from(x_offset)),
                child_y.saturating_add(i32::from(y_offset)),
                &source_damage,
                shape.as_deref(),
                &crate::XPresentStacking::default(),
            ) else {
                return XAuthorityResponsePacket::rejected(
                    transaction,
                    XAuthorityRuntimeError::InvalidResource,
                );
            };
            let handle = update.handle();
            let extent = update.size();
            if std::env::var("SOPHIA_X11_PIXEL_TRACE").as_deref() == Ok("1")
                && let Some(snapshot) = self.software_buffers.presentation_snapshot(target_window)
            {
                crate::image::trace_image_pixels(
                    "present",
                    transaction,
                    target_window,
                    Rect {
                        x: 0,
                        y: 0,
                        width: snapshot.size.width,
                        height: snapshot.size.height,
                    },
                    &snapshot.bytes,
                );
            }
            self.last_cpu_buffer_updates.push(update);
            (
                sophia_protocol::BufferSource::CpuBuffer { handle },
                Region {
                    rects: damage
                        .rects
                        .into_iter()
                        .map(|rect| Rect {
                            x: rect.x.saturating_add(child_x),
                            y: rect.y.saturating_add(child_y),
                            ..rect
                        })
                        .collect(),
                },
                extent,
                extent,
            )
        };
        // Two extents, and they are not the same question. The drawing window
        // is what this present was asked to fill; the pixmap is what the client
        // actually handed over. A client that has not answered its last
        // configure presents the buffer it already has, and declaring the
        // window's size for it put a raster nobody had measured into committed
        // content -- which the compositor later compared against the buffer and
        // ended the session over.
        self.raster_store
            .invalidate_unjournaled_presentation(target_window, presentation_extent);
        let mut update = XDrawingUpdate::present_buffer(
            transaction,
            namespace,
            target_window,
            buffer,
            presentation_extent,
            raster_extent,
            damage,
            target_generation,
            250,
        );
        if target_window == window
            && x_offset == 0
            && y_offset == 0
            && child_x == 0
            && child_y == 0
            && !has_valid_region
            && matches!(buffer, sophia_protocol::BufferSource::DmaBuf { .. })
        {
            update.raster_damage = Some(Region {
                rects: source_damage,
            });
        }
        self.finish_drawing_update(update)
    }
}

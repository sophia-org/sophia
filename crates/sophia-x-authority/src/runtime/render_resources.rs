fn software_pixmap_byte_len(size: Size) -> Option<usize> {
    let width = usize::try_from(size.width).ok()?;
    let height = usize::try_from(size.height).ok()?;
    let bytes = width.checked_mul(height)?.checked_mul(4)?;
    (width != 0 && height != 0 && bytes <= crate::X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES)
        .then_some(bytes)
}

const X_AUTHORITY_PRESENT_REGION_MAX_RECTS: usize = 2_048;

/// The X depth a DRI3-imported format reports back.
///
/// The inverse of the `(depth, bits_per_pixel) -> format` mapping the import
/// path applies, kept beside it so a pixmap cannot be admitted at one depth and
/// recovered at another.
pub fn dri3_depth_of(format: u32) -> u8 {
    if format == sophia_protocol::DRM_FORMAT_ARGB8888 {
        32
    } else {
        24
    }
}

fn clipped_present_rect(size: Size, rect: Rect) -> Option<Rect> {
    let left = rect.x.max(0).min(size.width);
    let top = rect.y.max(0).min(size.height);
    let right = rect.x.saturating_add(rect.width).max(0).min(size.width);
    let bottom = rect.y.saturating_add(rect.height).max(0).min(size.height);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    })
}

fn present_source_damage(size: Size, update: Option<&Region>) -> Option<Vec<Rect>> {
    if update.is_some_and(|region| region.rects.len() > X_AUTHORITY_PRESENT_REGION_MAX_RECTS) {
        return None;
    }
    Some(match update {
        Some(region) => region
            .rects
            .iter()
            .filter_map(|rect| clipped_present_rect(size, *rect))
            .collect(),
        None => vec![Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }],
    })
}

fn translated_present_damage(source: &[Rect], x_offset: i16, y_offset: i16) -> Region {
    Region {
        rects: source
            .iter()
            .map(|rect| Rect {
                x: rect.x.saturating_add(i32::from(x_offset)),
                y: rect.y.saturating_add(i32::from(y_offset)),
                width: rect.width,
                height: rect.height,
            })
            .collect(),
    }
}

impl XAuthorityRuntime {
    pub fn create_pixmap(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        size: Size,
        depth: u8,
        generation: u64,
    ) -> Result<(), XAuthorityRuntimeError> {
        if size.width <= 0 || size.height <= 0 || crate::x11_pixmap_format(depth).is_none() {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        self.resources
            .insert(pixmap, XResourceKind::Pixmap, namespace, generation)
            .map_err(XAuthorityRuntimeError::from)?;
        self.pixmaps.insert(pixmap, XPixmapRecord { size, depth });
        Ok(())
    }

    pub fn free_pixmap(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<Option<sophia_protocol::BufferHandle>, XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, pixmap, XResourceKind::Pixmap)?;
        let released_handle = self
            .dri3_pixmaps
            .get(&pixmap)
            .map(|record| record.descriptor.handle);
        // A pixmap something still references keeps its registration: the
        // release is owed to the retained backing and is sent when the last
        // referent goes, not when the XID does.
        let retained = self.retain_freed_pixmap(namespace, pixmap, released_handle)?;
        let provider_tracked = self.pixmap_export_handles.contains_key(&pixmap);
        if !retained {
            self.retire_pixmap_export_drawable(pixmap);
        }
        let released_handle = if retained || provider_tracked {
            None
        } else {
            released_handle
        };
        if let Some(handle) = released_handle {
            self.retired_pixmap_registrations
                .entry(namespace)
                .or_default()
                .push(handle);
        }
        self.resources.remove(pixmap);
        self.pixmaps.remove(&pixmap);
        self.shm_pixmaps.remove(&pixmap);
        self.shm_mappings
            .retain(|_, mapping| mapping.strong_count() != 0);
        self.software_buffers.remove(pixmap);
        self.dri3_pixmaps.remove(&pixmap);
        Ok(released_handle)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_shm_pixmap(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        size: Size,
        depth: u8,
        generation: u64,
        segment: crate::XResourceId,
        offset: u32,
    ) -> Result<(), XAuthorityRuntimeError> {
        let byte_len =
            software_pixmap_byte_len(size).ok_or(XAuthorityRuntimeError::InvalidResource)?;
        let end = usize::try_from(offset)
            .ok()
            .and_then(|offset| offset.checked_add(byte_len))
            .ok_or(XAuthorityRuntimeError::InvalidResource)?;
        if end > crate::X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        // Whichever way the segment was named, the pixmap binds to memory.
        let mapping = self.shm_segment_mapping(namespace, segment)?;
        if end > mapping.len() {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        self.create_pixmap(namespace, pixmap, size, depth, generation)?;
        self.shm_pixmaps.insert(
            pixmap,
            XShmPixmapBinding {
                offset,
                size,
                mapping,
            },
        );
        Ok(())
    }

    pub fn validate_pixmap_access(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, pixmap, XResourceKind::Pixmap)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub fn pixmap_size(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<Size, XAuthorityRuntimeError> {
        self.pixmap_geometry(namespace, pixmap)
            .map(|(size, _)| size)
    }

    pub fn pixmap_geometry(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<(Size, u8), XAuthorityRuntimeError> {
        self.validate_pixmap_access(namespace, pixmap)?;
        self.pixmaps
            .get(&pixmap)
            .map(|record| (record.size, record.depth))
            .ok_or(XAuthorityRuntimeError::UnknownResource)
    }

    pub fn pixmap_depth(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<u8, XAuthorityRuntimeError> {
        self.pixmap_geometry(namespace, pixmap)
            .map(|(_, depth)| depth)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_dri3_pixmap(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        generation: u64,
        size_bytes: u32,
        width: u16,
        height: u16,
        stride: u16,
        depth: u8,
        bits_per_pixel: u8,
    ) -> Result<sophia_protocol::DmaBufDescriptor, XAuthorityRuntimeError> {
        let format = match (depth, bits_per_pixel) {
            (24, 32) => sophia_protocol::DRM_FORMAT_XRGB8888,
            (32, 32) => sophia_protocol::DRM_FORMAT_ARGB8888,
            _ => return Err(XAuthorityRuntimeError::InvalidResource),
        };
        let handle = self.next_dma_buf_handle.max(1);
        let descriptor = sophia_protocol::DmaBufDescriptor {
            handle: sophia_protocol::BufferHandle::from_raw(handle),
            size: Size {
                width: i32::from(width),
                height: i32::from(height),
            },
            format,
            modifier: sophia_protocol::DRM_FORMAT_MOD_INVALID,
            plane_count: 1,
            planes: [
                Some(sophia_protocol::DmaBufPlaneDescriptor {
                    offset: 0,
                    stride: u32::from(stride),
                }),
                None,
                None,
                None,
            ],
        };
        descriptor
            .validate()
            .map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
        if u64::from(stride).saturating_mul(u64::from(height)) > u64::from(size_bytes) {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        self.create_pixmap(namespace, pixmap, descriptor.size, depth, generation)?;
        self.next_dma_buf_handle = handle.saturating_add(1).max(1);
        self.dri3_pixmaps.insert(
            pixmap,
            XDri3PixmapRecord {
                descriptor,
                plane_fds: Vec::new(),
            },
        );
        Ok(descriptor)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_dri3_pixmap_from_buffers(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        generation: u64,
        num_buffers: u8,
        width: u16,
        height: u16,
        strides: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
        offsets: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
        depth: u8,
        bits_per_pixel: u8,
        modifier: u64,
    ) -> Result<sophia_protocol::DmaBufDescriptor, XAuthorityRuntimeError> {
        let format = match (depth, bits_per_pixel) {
            (24, 32) => sophia_protocol::DRM_FORMAT_XRGB8888,
            (32, 32) => sophia_protocol::DRM_FORMAT_ARGB8888,
            _ => return Err(XAuthorityRuntimeError::InvalidResource),
        };
        if num_buffers == 0 || usize::from(num_buffers) > sophia_protocol::DMA_BUF_MAX_PLANES {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let handle = self.next_dma_buf_handle.max(1);
        let mut planes = [None; sophia_protocol::DMA_BUF_MAX_PLANES];
        for index in 0..usize::from(num_buffers) {
            planes[index] = Some(sophia_protocol::DmaBufPlaneDescriptor {
                offset: offsets[index],
                stride: strides[index],
            });
        }
        let descriptor = sophia_protocol::DmaBufDescriptor {
            handle: sophia_protocol::BufferHandle::from_raw(handle),
            size: Size {
                width: i32::from(width),
                height: i32::from(height),
            },
            format,
            modifier,
            plane_count: num_buffers,
            planes,
        };
        descriptor
            .validate()
            .map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
        self.create_pixmap(namespace, pixmap, descriptor.size, depth, generation)?;
        self.next_dma_buf_handle = handle.saturating_add(1).max(1);
        self.dri3_pixmaps.insert(
            pixmap,
            XDri3PixmapRecord {
                descriptor,
                plane_fds: Vec::new(),
            },
        );
        Ok(descriptor)
    }

    /// Records the plane descriptors a DRI3 import arrived with.
    ///
    /// Separate from the import itself because the descriptors reach the
    /// authority at the socket, where the client's ancillary data is read, while
    /// the import is decided in the pure dispatch layer that never sees them.
    /// `Dri3Open` splits along the same seam.
    pub fn attach_dri3_plane_fds(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        plane_fds: Vec<std::sync::Arc<std::os::fd::OwnedFd>>,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_pixmap_access(namespace, pixmap)?;
        let record = self
            .dri3_pixmaps
            .get_mut(&pixmap)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        if plane_fds.len() != usize::from(record.descriptor.plane_count) {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        record.plane_fds = plane_fds;
        Ok(())
    }

    /// What a drawable would need backing at, when it has no buffer of its own.
    ///
    /// Deliberately wider than a pixmap. A client asking the server to own the
    /// storage names whatever it renders into, and for a GL client that is a GLX
    /// drawable -- a window alias or a pbuffer -- not a core pixmap. Refusing
    /// those as "not a pixmap" is what left the browser retrying a request it
    /// could never satisfy.
    ///
    /// `None` means no allocation is warranted: the drawable already carries a
    /// buffer, it is not one this client may name, or it holds pixels drawn by
    /// the CPU path. That last one is refused rather than backed, because a
    /// buffer allocated now would be empty and would silently replace the
    /// content the client already drew.
    pub fn dri3_pixmap_backing_request(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Option<crate::XServerFrontendPixmapAllocation> {
        self.validate_dri3_drawable_access(namespace, drawable)
            .ok()?;
        if self.dri3_pixmaps.contains_key(&drawable) {
            return None;
        }
        if self.software_buffers.has_cpu_backing(drawable) {
            return None;
        }
        let facts = self.drawable_facts(namespace, drawable).ok()?;
        // The root is the session's own output, not a client surface to hand
        // storage for.
        if matches!(facts.kind, crate::XDrawableKind::Root) {
            return None;
        }
        Some(crate::XServerFrontendPixmapAllocation {
            size: Size {
                width: facts.geometry.width,
                height: facts.geometry.height,
            },
            depth: facts.depth,
            handle: self.next_dma_buf_handle.max(1),
        })
    }

    /// Records a buffer the authority originated for a pixmap the client did
    /// not allocate.
    ///
    /// The same record an import produces, so everything downstream -- recovery,
    /// presentation, release with the pixmap -- cannot tell the two halves of
    /// DRI3 apart, which is the point.
    pub fn adopt_dri3_pixmap_backing(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        descriptor: sophia_protocol::DmaBufDescriptor,
        plane_fds: Vec<std::sync::Arc<std::os::fd::OwnedFd>>,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_dri3_drawable_access(namespace, pixmap)?;
        if plane_fds.len() != usize::from(descriptor.plane_count) {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        descriptor
            .validate()
            .map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
        // The handle must be the one issued, not one the allocator chose: a
        // buffer answering to a name another buffer already holds is the same
        // buffer as far as the renderer's registry is concerned.
        if descriptor.handle.raw() != self.next_dma_buf_handle.max(1) {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        self.next_dma_buf_handle = descriptor.handle.raw().saturating_add(1);
        self.dri3_pixmaps.insert(
            pixmap,
            XDri3PixmapRecord {
                descriptor,
                plane_fds,
            },
        );
        Ok(())
    }

    /// The next buffer handle the authority may originate.
    pub fn next_dma_buf_handle(&self) -> u64 {
        self.next_dma_buf_handle.max(1)
    }

    /// The facts and plane descriptors a DRI3 pixmap can be recovered from.
    ///
    /// Refuses a pixmap whose descriptors were never recorded rather than
    /// answering with a short list: a reply that promises `nfd` buffers and
    /// carries fewer is worse for the client than a plain error.
    pub fn dri3_pixmap_buffers(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<
        (
            sophia_protocol::DmaBufDescriptor,
            Vec<std::sync::Arc<std::os::fd::OwnedFd>>,
        ),
        XAuthorityRuntimeError,
    > {
        // A recovery can be refused three ways, and the client sees one error
        // for all of them. Name which, because "not a pixmap here" is a caller
        // mistake while "no imported buffer" is a capability Sophia does not
        // have -- and they need opposite fixes.
        if let Err(error) = self.validate_dri3_drawable_access(namespace, pixmap) {
            if crate::x11_authority_trace_enabled() {
                tracing::info!(
                    "sophia_dri3_recovery schema=1 status=refused reason=not_a_drawable pixmap={:#x} error={error:?}",
                    pixmap.local.raw(),
                );
            }
            return Err(error);
        }
        let backing = self.resolve_pixmap_export_backing(namespace, pixmap)?;
        let Some(record) = self.pixmap_export_descriptor(backing) else {
            if crate::x11_authority_trace_enabled() {
                tracing::info!(
                    "sophia_dri3_recovery schema=1 status=refused reason=never_imported pixmap={:#x} kind={:?}",
                    pixmap.local.raw(),
                    self.drawable_facts(namespace, pixmap)
                        .map(|facts| facts.kind)
                        .ok(),
                );
            }
            return Err(XAuthorityRuntimeError::UnknownResource);
        };
        if record.plane_fds.len() != usize::from(record.descriptor.plane_count) {
            if crate::x11_authority_trace_enabled() {
                tracing::info!(
                    "sophia_dri3_recovery schema=1 status=refused reason=descriptors_missing pixmap={:#x} retained={} planes={}",
                    pixmap.local.raw(),
                    record.plane_fds.len(),
                    record.descriptor.plane_count,
                );
            }
            return Err(XAuthorityRuntimeError::UnknownResource);
        }
        Ok((record.descriptor, record.plane_fds.clone()))
    }

    pub fn dri3_pixmap_descriptor(
        &self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
    ) -> Result<sophia_protocol::DmaBufDescriptor, XAuthorityRuntimeError> {
        self.validate_pixmap_access(namespace, pixmap)?;
        self.dri3_pixmaps
            .get(&pixmap)
            .map(|record| record.descriptor)
            .ok_or(XAuthorityRuntimeError::UnknownResource)
    }

    pub fn present_standard_pixmap(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        pixmap: crate::XResourceId,
        x_offset: i16,
        y_offset: i16,
        valid_region: Option<Region>,
        update_region: Option<Region>,
    ) -> XAuthorityResponsePacket {
        let record = match self.windows.get(window) {
            Some(record) if record.namespace == namespace => record.clone(),
            _ => {
                return XAuthorityResponsePacket::rejected(
                    transaction,
                    XAuthorityRuntimeError::UnknownResource,
                );
            }
        };
        if let Err(error) = self.validate_pixmap_access(namespace, pixmap) {
            return XAuthorityResponsePacket::rejected(transaction, error);
        }
        if valid_region
            .as_ref()
            .is_some_and(|region| region.rects.len() > X_AUTHORITY_PRESENT_REGION_MAX_RECTS)
        {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            );
        }
        let Some(pixmap_size) = self.pixmaps.get(&pixmap).map(|record| record.size) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::UnknownResource,
            );
        };
        let Some(source_damage) = present_source_damage(pixmap_size, update_region.as_ref()) else {
            return XAuthorityResponsePacket::rejected(
                transaction,
                XAuthorityRuntimeError::InvalidResource,
            );
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
            if let Some(binding) = self.shm_pixmaps.get(&pixmap).cloned() {
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
        self.finish_drawing_update(XDrawingUpdate::present_buffer(
            transaction,
            namespace,
            target_window,
            buffer,
            presentation_extent,
            raster_extent,
            damage,
            target_generation,
            250,
        ))
    }

    pub fn create_dri3_fence(
        &mut self,
        namespace: NamespaceId,
        fence: crate::XResourceId,
        generation: u64,
    ) -> Result<sophia_protocol::FenceHandle, XAuthorityRuntimeError> {
        self.resources
            .insert(fence, XResourceKind::Fence, namespace, generation)
            .map_err(XAuthorityRuntimeError::from)?;
        let handle = sophia_protocol::FenceHandle::from_raw(self.next_fence_handle.max(1));
        self.next_fence_handle = handle.raw().saturating_add(1).max(1);
        self.dri3_fences.insert(fence, handle);
        Ok(handle)
    }

    pub fn validate_dri3_fence_access(
        &self,
        namespace: NamespaceId,
        fence: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, fence, XResourceKind::Fence)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub fn dri3_fence_handle(
        &self,
        namespace: NamespaceId,
        fence: crate::XResourceId,
    ) -> Result<sophia_protocol::FenceHandle, XAuthorityRuntimeError> {
        self.validate_dri3_fence_access(namespace, fence)?;
        self.dri3_fences
            .get(&fence)
            .copied()
            .ok_or(XAuthorityRuntimeError::UnknownResource)
    }

    pub fn destroy_dri3_fence(
        &mut self,
        namespace: NamespaceId,
        fence: crate::XResourceId,
    ) -> Result<sophia_protocol::FenceHandle, XAuthorityRuntimeError> {
        self.validate_dri3_fence_access(namespace, fence)?;
        self.resources.remove(fence);
        self.dri3_fences
            .remove(&fence)
            .ok_or(XAuthorityRuntimeError::UnknownResource)
    }

    pub fn open_font(
        &mut self,
        namespace: NamespaceId,
        font: crate::XResourceId,
        generation: u64,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.open_font_face(namespace, font, crate::builtin_font_handle(), generation)
    }

    /// Open a font by the name a client asked for.
    ///
    /// Resolution belongs to the catalog: this only decides what a failure
    /// means to the protocol. A name nothing publishes is a client error, not
    /// a session one.
    /// Names matching a pattern, for `ListFonts`.
    pub(crate) fn list_fonts(&self, pattern: &str, max_names: usize) -> Vec<String> {
        self.font_catalog.list(pattern, max_names)
    }

    /// Names with their metrics, for `ListFontsWithInfo`.
    ///
    /// A name that will not load is dropped rather than reported with invented
    /// metrics, since a client uses these numbers to lay text out.
    pub(crate) fn list_fonts_with_info(
        &mut self,
        pattern: &str,
        max_names: usize,
    ) -> Vec<(String, Box<crate::XFontMetrics>)> {
        let names = self.font_catalog.list(pattern, max_names);
        names
            .into_iter()
            .filter_map(|name| {
                let (face, _) = self.font_catalog.open(&name).ok()?;
                Some((name, Box::new(face.metrics.clone())))
            })
            .collect()
    }

    pub(crate) fn open_named_font(
        &mut self,
        namespace: NamespaceId,
        font: crate::XResourceId,
        name: &str,
        generation: u64,
    ) -> Result<(), crate::XFontOpenFailure> {
        let (face, _) = self
            .font_catalog
            .open(name)
            .map_err(|_| crate::XFontOpenFailure::Unresolved)?;
        self.open_font_face(namespace, font, face, generation)
            .map_err(crate::XFontOpenFailure::Resource)
    }

    pub(crate) fn open_font_face(
        &mut self,
        namespace: NamespaceId,
        font: crate::XResourceId,
        face: crate::XFontHandle,
        generation: u64,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .insert(font, XResourceKind::Font, namespace, generation)
            .map_err(XAuthorityRuntimeError::from)?;
        self.fonts.insert(font, XFontRecord { face });
        Ok(())
    }

    pub fn close_font(
        &mut self,
        namespace: NamespaceId,
        font: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, font, XResourceKind::Font)?;
        self.resources.remove(font);
        self.fonts.remove(&font);
        Ok(())
    }

    pub fn validate_font_access(
        &self,
        namespace: NamespaceId,
        font: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, font, XResourceKind::Font)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub(crate) fn font_face(
        &self,
        namespace: NamespaceId,
        font: crate::XResourceId,
    ) -> Result<crate::XFontHandle, XAuthorityRuntimeError> {
        self.validate_font_access(namespace, font)?;
        self.fonts
            .get(&font)
            .map(|record| record.face.clone())
            .ok_or(XAuthorityRuntimeError::UnknownResource)
    }

    pub fn create_cursor(
        &mut self,
        namespace: NamespaceId,
        cursor: crate::XResourceId,
        generation: u64,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .insert(cursor, XResourceKind::Cursor, namespace, generation)
            .map_err(Into::into)
    }

    pub fn free_cursor(
        &mut self,
        namespace: NamespaceId,
        cursor: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, cursor, XResourceKind::Cursor)?;
        self.resources.remove(cursor);
        self.render_cursor_images.remove(&cursor);
        Ok(())
    }

    pub fn validate_cursor_access(
        &self,
        namespace: NamespaceId,
        cursor: crate::XResourceId,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, cursor, XResourceKind::Cursor)
            .map(|_| ())
            .map_err(Into::into)
    }
}

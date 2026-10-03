const X_PIXMAP_EXPORT_LIMIT: usize = 1_024;
const X_PIXMAP_PREFIX_TARGET_LIMIT: usize = 4_096;

/// Identity issued before provider allocation, independent of every wire XID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPixmapExportToken {
    pub handle: sophia_protocol::BufferHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPixmapExportPreparation {
    Ready,
    Pending(XPixmapExportToken),
    Allocate {
        token: XPixmapExportToken,
        request: crate::XServerFrontendPixmapAllocation,
    },
}

/// One pinned, finite publication obligation captured by a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPixmapPublicationTarget {
    pub handle: sophia_protocol::BufferHandle,
    pub generation: u64,
    ticket: u64,
}

#[derive(Debug)]
struct XPixmapUpdateFlight {
    revision: u64,
    generation: u64,
    damage: Vec<Rect>,
}

#[derive(Debug)]
struct XPixmapPublication {
    namespace: NamespaceId,
    backing: crate::XResourceId,
    retirement_backing: crate::XResourceId,
    request: crate::XServerFrontendPixmapAllocation,
    allocating: bool,
    provider_owned: bool,
    retired: bool,
    pins: usize,
    published_generation: Option<u64>,
    next_revision: u64,
    in_flight: Option<XPixmapUpdateFlight>,
}

impl XAuthorityRuntime {
    /// Reserves the handle and backing lifetime before the provider leaves this lock.
    pub fn prepare_pixmap_export(
        &mut self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<XPixmapExportPreparation, XAuthorityRuntimeError> {
        let backing = self.resolve_pixmap_export_backing(namespace, drawable)?;
        // Direct SHM writes have no authority damage boundary to publish against.
        if self.shm_pixmaps.contains_key(&backing)
            || self
                .retained_pixmap_backings
                .get(&backing)
                .is_some_and(|entry| entry._shm.is_some())
        {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        if let Some(handle) = self.pixmap_export_handles.get(&backing) {
            let publication = self
                .pixmap_publications
                .get(handle)
                .ok_or(XAuthorityRuntimeError::UnknownResource)?;
            return Ok(if publication.allocating {
                XPixmapExportPreparation::Pending(XPixmapExportToken { handle: *handle })
            } else {
                XPixmapExportPreparation::Ready
            });
        }
        if self.pixmap_export_descriptor(backing).is_some() {
            return Ok(XPixmapExportPreparation::Ready);
        }
        if !self.pixmap_textures_supported() && self.software_buffers.has_cpu_backing(backing) {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let reserved = self.provider_pixmap_backings.len()
            + self
                .pixmap_publications
                .values()
                .filter(|state| !state.provider_owned)
                .count();
        if reserved >= X_PIXMAP_EXPORT_LIMIT {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let (size, depth) = self.pixmap_export_size_depth(namespace, backing)?;
        if !matches!(depth, 24 | 32) || software_pixmap_byte_len(size).is_none() {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let handle = sophia_protocol::BufferHandle::from_raw(self.next_dma_buf_handle.max(1));
        self.next_dma_buf_handle = handle
            .raw()
            .checked_add(1)
            .ok_or(XAuthorityRuntimeError::InvalidResource)?;
        let retirement_backing = crate::XResourceId::new(self.next_render_backing, 1);
        self.next_render_backing = self
            .next_render_backing
            .checked_add(1)
            .ok_or(XAuthorityRuntimeError::InvalidResource)?;
        let request = crate::XServerFrontendPixmapAllocation {
            handle: handle.raw(),
            size,
            depth,
        };
        let published_generation = (!self.software_buffers.has_cpu_backing(backing)).then_some(0);
        let provider_owned = self.pixmap_textures_supported();
        if provider_owned {
            self.provider_pixmap_backings.insert(handle);
            self.software_buffers.begin_export_tracking(backing);
        }
        self.pixmap_export_handles.insert(backing, handle);
        self.pixmap_publications.insert(
            handle,
            XPixmapPublication {
                namespace,
                backing,
                retirement_backing,
                request,
                allocating: true,
                provider_owned,
                retired: false,
                pins: 0,
                published_generation,
                next_revision: 1,
                in_flight: None,
            },
        );
        Ok(XPixmapExportPreparation::Allocate {
            token: XPixmapExportToken { handle },
            request,
        })
    }

    pub fn pixmap_export_allocation_pending(&self, token: XPixmapExportToken) -> bool {
        self.pixmap_publications
            .get(&token.handle)
            .is_some_and(|state| state.allocating)
    }

    /// A rejected completion owes cleanup for the allocation that actually arrived.
    pub fn finish_pixmap_export_allocation(
        &mut self,
        token: XPixmapExportToken,
        allocation: Option<crate::XServerFrontendAllocatedPixmap>,
    ) -> Result<bool, XAuthorityRuntimeError> {
        let Some(state) = self.pixmap_publications.get(&token.handle) else {
            if self.pixmap_textures_supported() && allocation.is_some() {
                self.pending_backing_releases.push_back(token.handle);
            }
            return Err(XAuthorityRuntimeError::UnknownResource);
        };
        if !state.allocating {
            // A duplicate completion cannot release the allocation already adopted.
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let backing = state.backing;
        let request = state.request;
        let provider_owned = state.provider_owned;
        let Some(allocation) = allocation else {
            self.provider_pixmap_backings.remove(&token.handle);
            self.remove_failed_pixmap_allocation(token.handle);
            return Ok(false);
        };
        let descriptor = allocation.descriptor;
        let format = if request.depth == 32 {
            sophia_protocol::DRM_FORMAT_ARGB8888
        } else {
            sophia_protocol::DRM_FORMAT_XRGB8888
        };
        if descriptor.handle != token.handle
            || descriptor.size != request.size
            || descriptor.format != format
            || descriptor.validate().is_err()
            || allocation.plane_fds.len() != usize::from(descriptor.plane_count)
        {
            if provider_owned {
                self.pending_backing_releases.push_back(token.handle);
            }
            self.remove_failed_pixmap_allocation(token.handle);
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let record = XDri3PixmapRecord {
            descriptor,
            plane_fds: allocation.plane_fds.into_iter().map(Arc::new).collect(),
        };
        if let Some(retained) = self.retained_pixmap_backings.get_mut(&backing) {
            retained._dri3 = Some(record);
            retained.release_handle = Some(token.handle);
        } else {
            self.dri3_pixmaps.insert(backing, record);
        }
        self.pixmap_publications
            .get_mut(&token.handle)
            .expect("allocation remains reserved")
            .allocating = false;
        self.maybe_retire_pixmap_publication(token.handle);
        Ok(self.pixmap_publications.contains_key(&token.handle))
    }

    pub fn pixmap_export_buffers(
        &self,
        token: XPixmapExportToken,
    ) -> Result<(sophia_protocol::DmaBufDescriptor, Vec<Arc<OwnedFd>>), XAuthorityRuntimeError>
    {
        let state = self
            .pixmap_publications
            .get(&token.handle)
            .filter(|state| !state.allocating)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        let record = self
            .pixmap_export_descriptor(state.backing)
            .filter(|record| record.descriptor.handle == token.handle)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        Ok((record.descriptor, record.plane_fds.clone()))
    }

    /// Captures only this namespace's prefix. Future writes do not change its targets.
    pub fn capture_pixmap_publication_prefix(
        &mut self,
        namespace: NamespaceId,
    ) -> Result<Vec<XPixmapPublicationTarget>, XAuthorityRuntimeError> {
        if !namespace.is_valid() {
            return Err(XAuthorityRuntimeError::InvalidNamespace);
        }
        let pending = self
            .pixmap_publications
            .iter()
            .filter(|(_, state)| {
                state.namespace == namespace
                    && !state.retired
                    && (state.allocating
                        || state.in_flight.is_some()
                        || self.software_buffers.export_has_damage(state.backing))
            })
            .map(|(handle, state)| {
                (
                    *handle,
                    self.software_buffers.export_generation(state.backing),
                )
            })
            .collect::<Vec<_>>();
        if pending.len()
            > X_PIXMAP_PREFIX_TARGET_LIMIT.saturating_sub(self.pixmap_publication_targets.len())
            || self
                .next_pixmap_publication_target
                .checked_add(pending.len() as u64)
                .is_none()
        {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let mut targets = Vec::with_capacity(pending.len());
        for (handle, generation) in pending {
            let target = XPixmapPublicationTarget {
                handle,
                generation,
                ticket: self.next_pixmap_publication_target,
            };
            self.next_pixmap_publication_target += 1;
            self.pixmap_publication_targets
                .insert(target.ticket, target);
            self.pixmap_publications
                .get_mut(&handle)
                .expect("captured backing exists")
                .pins += 1;
            targets.push(target);
        }
        Ok(targets)
    }

    pub fn pixmap_publication_target_settled(&self, target: XPixmapPublicationTarget) -> bool {
        self.pixmap_publication_targets.get(&target.ticket) == Some(&target)
            && self
                .pixmap_publications
                .get(&target.handle)
                .is_some_and(|state| {
                    !state.allocating
                        && state
                            .published_generation
                            .is_some_and(|generation| generation >= target.generation)
                })
    }

    pub fn pixmap_publication_update_pending(&self, target: XPixmapPublicationTarget) -> bool {
        self.pixmap_publication_targets.get(&target.ticket) == Some(&target)
            && self
                .pixmap_publications
                .get(&target.handle)
                .is_some_and(|state| state.in_flight.is_some() || state.allocating)
    }

    /// Copies only the bounded dirty cover; failed work restores that cover to the source.
    pub fn take_pixmap_publication_update(
        &mut self,
        target: XPixmapPublicationTarget,
    ) -> Result<Option<crate::XServerFrontendPixmapUpdate>, XAuthorityRuntimeError> {
        if self.pixmap_publication_targets.get(&target.ticket) != Some(&target) {
            return Err(XAuthorityRuntimeError::UnknownResource);
        }
        if self.pixmap_publication_target_settled(target)
            || self.pixmap_publication_update_pending(target)
        {
            return Ok(None);
        }
        let state = self
            .pixmap_publications
            .get(&target.handle)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        let backing = state.backing;
        let request = state.request;
        let revision = state.next_revision;
        let next_revision = revision
            .checked_add(1)
            .ok_or(XAuthorityRuntimeError::InvalidResource)?;
        let (generation, patches) = self
            .software_buffers
            .take_export_patches(backing)
            .ok_or(XAuthorityRuntimeError::InvalidResource)?;
        if patches.is_empty() {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let state = self
            .pixmap_publications
            .get_mut(&target.handle)
            .expect("backing remains pinned");
        state.next_revision = next_revision;
        state.in_flight = Some(XPixmapUpdateFlight {
            revision,
            generation,
            damage: patches.iter().map(|patch| patch.rect).collect(),
        });
        Ok(Some(crate::XServerFrontendPixmapUpdate {
            handle: target.handle,
            revision,
            size: request.size,
            format: if request.depth == 32 {
                sophia_protocol::DRM_FORMAT_ARGB8888
            } else {
                sophia_protocol::DRM_FORMAT_XRGB8888
            },
            patches,
        }))
    }

    pub fn finish_pixmap_publication_update(
        &mut self,
        handle: sophia_protocol::BufferHandle,
        revision: u64,
        published: bool,
    ) {
        let Some(state) = self.pixmap_publications.get_mut(&handle) else {
            return;
        };
        if state
            .in_flight
            .as_ref()
            .is_none_or(|flight| flight.revision != revision)
        {
            return;
        }
        let flight = state.in_flight.take().expect("exact flight checked");
        if published {
            state.published_generation = Some(flight.generation);
        } else {
            self.software_buffers
                .restore_export_damage(state.backing, &flight.damage);
        }
        self.maybe_retire_pixmap_publication(handle);
    }

    pub fn release_pixmap_publication_prefix(
        &mut self,
        targets: impl IntoIterator<Item = XPixmapPublicationTarget>,
    ) {
        for target in targets {
            if self.pixmap_publication_targets.get(&target.ticket) != Some(&target) {
                continue;
            }
            self.pixmap_publication_targets.remove(&target.ticket);
            if let Some(state) = self.pixmap_publications.get_mut(&target.handle) {
                state.pins -= 1;
            }
            self.maybe_retire_pixmap_publication(target.handle);
        }
    }

    /// Capacity is released only after the provider confirms destruction outside the lock.
    pub fn finish_pixmap_backing_release(&mut self, handle: sophia_protocol::BufferHandle) {
        if !self.pixmap_publications.contains_key(&handle) {
            self.provider_pixmap_backings.remove(&handle);
        }
    }

    /// Provider storage debt is global; Engine registration retirement is namespace scoped.
    pub fn take_retired_pixmap_registrations(
        &mut self,
        namespace: NamespaceId,
    ) -> Vec<sophia_protocol::BufferHandle> {
        self.retired_pixmap_registrations
            .remove(&namespace)
            .unwrap_or_default()
    }

    /// Take the selection ownerships ended since the last drain.
    ///
    /// Drained rather than inspected so each ownership is reported once: a
    /// second reader would either re-notify watchers or race the first.
    pub fn take_retired_selection_ownerships(&mut self) -> Vec<crate::XSelectionOwnerUpdate> {
        core::mem::take(&mut self.retired_selection_ownerships)
    }

    pub(crate) fn retire_pixmap_export_drawable(&mut self, drawable: crate::XResourceId) {
        if let Some(handle) = self.pixmap_export_handles.get(&drawable).copied() {
            let Some(state) = self.pixmap_publications.get_mut(&handle) else {
                return;
            };
            state.retired = true;
            if (state.allocating || state.pins != 0 || state.in_flight.is_some())
                && !self.retained_pixmap_backings.contains_key(&drawable)
            {
                let backing = state.retirement_backing;
                let namespace = state.namespace;
                let pixmap = XPixmapRecord {
                    size: state.request.size,
                    depth: state.request.depth,
                };
                let release_handle = self
                    .dri3_pixmaps
                    .get(&drawable)
                    .map(|record| record.descriptor.handle);
                self.software_buffers.rekey_pixmap(drawable, backing);
                self.rekey_pixmap_publication(drawable, backing);
                self.retained_pixmap_backings.insert(
                    backing,
                    XRetainedPixmapBacking {
                        namespace,
                        pixmap,
                        pictures: 0,
                        glx_pixmaps: 0,
                        presents: 0,
                        release_handle,
                        _shm: self.shm_pixmaps.remove(&drawable),
                        _dri3: self.dri3_pixmaps.remove(&drawable),
                    },
                );
            }
            self.maybe_retire_pixmap_publication(handle);
        }
    }

    fn remove_failed_pixmap_allocation(&mut self, handle: sophia_protocol::BufferHandle) {
        if let Some(state) = self.pixmap_publications.remove(&handle) {
            self.pixmap_export_handles.remove(&state.backing);
            self.software_buffers.end_export_tracking(state.backing);
            self.pixmap_publication_targets
                .retain(|_, target| target.handle != handle);
            self.maybe_drop_retained_pixmap(state.backing);
        }
    }

    fn pixmap_export_holds_backing(&self, backing: crate::XResourceId) -> bool {
        self.pixmap_export_handles
            .get(&backing)
            .and_then(|handle| self.pixmap_publications.get(handle))
            .is_some_and(|state| state.allocating || state.pins != 0 || state.in_flight.is_some())
    }

    fn rekey_pixmap_publication(&mut self, from: crate::XResourceId, to: crate::XResourceId) {
        if let Some(handle) = self.pixmap_export_handles.remove(&from) {
            self.pixmap_export_handles.insert(to, handle);
            if let Some(state) = self.pixmap_publications.get_mut(&handle) {
                state.backing = to;
            }
        }
    }

    fn maybe_retire_pixmap_publication(&mut self, handle: sophia_protocol::BufferHandle) {
        let Some(state) = self.pixmap_publications.get(&handle) else {
            return;
        };
        if state.allocating || state.pins != 0 || state.in_flight.is_some() {
            return;
        }
        let backing = state.backing;
        let no_referents = self
            .retained_pixmap_backings
            .get(&backing)
            .is_some_and(|retained| retained.referents() == 0);
        if !state.retired && !no_referents {
            return;
        }
        let namespace = state.namespace;
        let provider_owned = state.provider_owned;
        self.pixmap_publications.remove(&handle);
        self.pixmap_export_handles.remove(&backing);
        self.software_buffers.end_export_tracking(backing);
        if provider_owned {
            self.pending_backing_releases.push_back(handle);
        }
        self.retired_pixmap_registrations
            .entry(namespace)
            .or_default()
            .push(handle);
        if let Some(retained) = self.retained_pixmap_backings.get_mut(&backing) {
            retained.release_handle = None;
        } else {
            self.dri3_pixmaps.remove(&backing);
        }
        self.maybe_drop_retained_pixmap(backing);
    }

    fn resolve_pixmap_export_backing(
        &self,
        namespace: NamespaceId,
        drawable: crate::XResourceId,
    ) -> Result<crate::XResourceId, XAuthorityRuntimeError> {
        if let Some(record) = self.glx_drawables.get(&drawable) {
            if record.owner != namespace {
                return Err(XAuthorityRuntimeError::UnknownResource);
            }
            return Ok(match record.backing {
                XGlxDrawableBacking::Window(window) => window,
                XGlxDrawableBacking::Pbuffer(_) => drawable,
                XGlxDrawableBacking::Pixmap { pixmap, .. } => pixmap,
            });
        }
        self.validate_dri3_drawable_access(namespace, drawable)?;
        Ok(drawable)
    }

    fn pixmap_export_size_depth(
        &self,
        namespace: NamespaceId,
        backing: crate::XResourceId,
    ) -> Result<(Size, u8), XAuthorityRuntimeError> {
        if let Some(retained) = self.retained_pixmap_backings.get(&backing) {
            if retained.namespace != namespace {
                return Err(XAuthorityRuntimeError::UnknownResource);
            }
            return Ok((retained.pixmap.size, retained.pixmap.depth));
        }
        let facts = self.drawable_facts(namespace, backing)?;
        if facts.kind == crate::XDrawableKind::Root {
            return Err(XAuthorityRuntimeError::WrongResourceKind);
        }
        Ok((
            Size {
                width: facts.geometry.width,
                height: facts.geometry.height,
            },
            facts.depth,
        ))
    }

    fn pixmap_export_descriptor(&self, backing: crate::XResourceId) -> Option<&XDri3PixmapRecord> {
        self.dri3_pixmaps.get(&backing).or_else(|| {
            self.retained_pixmap_backings
                .get(&backing)
                .and_then(|retained| retained._dri3.as_ref())
        })
    }
}

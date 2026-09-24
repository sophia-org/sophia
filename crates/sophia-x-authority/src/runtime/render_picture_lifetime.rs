/// A pixmap whose XID was freed while something still references it.
///
/// The private storage key is never registered as a wire resource, so every
/// referent keeps one backing regardless of what later takes the XID.
///
/// The counts are separate but the backing is ONE. Two independent lifetimes
/// would drop it while the other kind still held it, so nothing is released
/// until both reach zero. Graphics contexts are a third kind of referent,
/// counted from the contexts themselves rather than kept here: a context
/// holds a clip mask, tile or stipple until a later request replaces it or
/// frees the context, and every such request re-checks the backing.
#[derive(Debug)]
struct XRetainedPixmapBacking {
    namespace: NamespaceId,
    pixmap: XPixmapRecord,
    pictures: usize,
    glx_pixmaps: usize,
    /// The renderer registration this backing owes a release for, once no
    /// referent remains.
    release_handle: Option<sophia_protocol::BufferHandle>,
    // Keep the underlying allocations alive even after the public pixmap and
    // its renderer registration disappear. RENDER currently uses CPU pixels;
    // retaining FDs does not add GPU sampling to that software path.
    _shm: Option<XShmPixmapBinding>,
    _dri3: Option<XDri3PixmapRecord>,
}

impl XRetainedPixmapBacking {
    const fn referents(&self) -> usize {
        self.pictures + self.glx_pixmaps
    }
}

impl XAuthorityRuntime {
    /// Retains a freed pixmap's backing while any referent survives.
    ///
    /// Answers whether it retained, because a pixmap still referenced must not
    /// have its renderer registration released yet.
    fn retain_freed_pixmap(
        &mut self,
        namespace: NamespaceId,
        pixmap: crate::XResourceId,
        release_handle: Option<sophia_protocol::BufferHandle>,
    ) -> Result<bool, XAuthorityRuntimeError> {
        let pictures = self
            .render_pictures
            .values()
            .filter(|record| !record.drawable_is_window && record.drawable == pixmap)
            .count();
        let glx_pixmaps = self
            .glx_drawables
            .values()
            .filter(|record| {
                matches!(record.backing, XGlxDrawableBacking::Pixmap { pixmap: backing, .. } if backing == pixmap)
            })
            .count();
        let held_by_graphics_context = self.graphics_contexts.holds_pixmap(pixmap);
        if pictures + glx_pixmaps == 0
            && !held_by_graphics_context
            && !self.pixmap_export_holds_backing(pixmap)
        {
            return Ok(false);
        }
        let metadata = *self
            .pixmaps
            .get(&pixmap)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        // X11 can name only 32-bit resources. Allocate above that range, and
        // never recycle a private key while an old picture can still hold it.
        let backing = loop {
            let key = crate::XResourceId::new(self.next_render_backing, 1);
            self.next_render_backing = self
                .next_render_backing
                .checked_add(1)
                .ok_or(XAuthorityRuntimeError::InvalidResource)?;
            if !self.resource_id_in_use(key) && !self.retained_pixmap_backings.contains_key(&key) {
                break key;
            }
        };
        self.software_buffers.rekey_pixmap(pixmap, backing);
        self.rekey_pixmap_publication(pixmap, backing);
        if pictures + glx_pixmaps == 0
            && let Some(handle) = self.pixmap_export_handles.get(&backing)
            && let Some(state) = self.pixmap_publications.get_mut(handle)
        {
            state.retired = true;
        }
        self.retained_pixmap_backings.insert(
            backing,
            XRetainedPixmapBacking {
                namespace,
                pixmap: metadata,
                pictures,
                glx_pixmaps,
                release_handle,
                _shm: self.shm_pixmaps.remove(&pixmap),
                _dri3: self.dri3_pixmaps.remove(&pixmap),
            },
        );
        for record in self.render_pictures.values_mut() {
            if !record.drawable_is_window && record.drawable == pixmap {
                record.drawable = backing;
            }
        }
        self.graphics_contexts.rekey_held_pixmap(pixmap, backing);
        for record in self.glx_drawables.values_mut() {
            if let XGlxDrawableBacking::Pixmap {
                pixmap: current,
                texture,
            } = record.backing
                && current == pixmap
            {
                record.backing = XGlxDrawableBacking::Pixmap {
                    pixmap: backing,
                    texture,
                };
            }
        }
        Ok(true)
    }

    /// Drops one referent and, when the last goes, the backing with it.
    ///
    /// The release is queued rather than performed: the renderer call must
    /// leave the runtime lock, and an owed release is never discarded.
    fn release_retained_referent(&mut self, backing: crate::XResourceId, picture: bool) {
        let Some(retained) = self.retained_pixmap_backings.get_mut(&backing) else {
            return;
        };
        if picture {
            retained.pictures = retained.pictures.saturating_sub(1);
        } else {
            retained.glx_pixmaps = retained.glx_pixmaps.saturating_sub(1);
        }
        if retained.referents() == 0
            && let Some(handle) = self.pixmap_export_handles.get(&backing)
            && let Some(state) = self.pixmap_publications.get_mut(handle)
        {
            state.retired = true;
        }
        self.maybe_drop_retained_pixmap(backing);
    }

    pub(super) fn maybe_drop_retained_pixmap(&mut self, backing: crate::XResourceId) {
        let Some(retained) = self.retained_pixmap_backings.get(&backing) else {
            return;
        };
        if retained.referents() != 0
            || self.graphics_contexts.holds_pixmap(backing)
            || self.pixmap_export_holds_backing(backing)
        {
            return;
        }
        if let Some(handle) = self.pixmap_export_handles.get(&backing).copied() {
            self.maybe_retire_pixmap_publication(handle);
            return;
        }
        let retained = self
            .retained_pixmap_backings
            .remove(&backing)
            .expect("retained backing checked");
        self.software_buffers.remove(backing);
        self.shm_mappings
            .retain(|_, mapping| mapping.strong_count() != 0);
        if let Some(handle) = retained.release_handle {
            self.retired_pixmap_registrations
                .entry(retained.namespace)
                .or_default()
                .push(handle);
        }
    }

    /// How many freed pixmaps live on because something still holds them.
    pub fn retained_pixmap_count(&self) -> usize {
        self.retained_pixmap_backings.len()
    }

    /// Takes the renderer registrations whose backings have been dropped.
    ///
    /// Draining leaves the runtime with no record of them, so a caller that
    /// cannot complete a release must hand it back through
    /// [`Self::restore_pending_backing_releases`] rather than drop it.
    pub fn take_pending_backing_releases(&mut self) -> Vec<sophia_protocol::BufferHandle> {
        self.pending_backing_releases.drain(..).collect()
    }

    /// Returns releases a caller could not complete, ahead of any queued since.
    pub fn restore_pending_backing_releases(
        &mut self,
        handles: impl IntoIterator<Item = sophia_protocol::BufferHandle>,
    ) {
        for handle in handles.into_iter().collect::<Vec<_>>().into_iter().rev() {
            self.pending_backing_releases.push_front(handle);
        }
    }

    fn render_release_picture(&mut self, picture: crate::XResourceId) {
        self.resources.remove(picture);
        let Some(record) = self.render_pictures.remove(&picture) else {
            return;
        };
        self.release_retained_referent(record.drawable, true);
    }
}

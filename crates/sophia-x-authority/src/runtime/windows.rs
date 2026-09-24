/// What a ReparentWindow did (t184), for the dispatcher to report.
#[derive(Clone, Debug)]
pub struct XWindowReparent {
    pub old_parent: crate::XResourceId,
    pub was_mapped: bool,
    pub override_redirect: bool,
    /// The surfaces the Engine applies, in the order the protocol performs
    /// the steps: the unmap, the placement, the remap.
    pub surfaces: Vec<AuthoritySurface>,
}

impl XAuthorityRuntime {
     pub fn resource_count(&self) -> usize {
         self.resources.len()
     }

     /// Where a window's origin sits in root coordinates.
     pub fn window_root_position(&self, window: crate::XResourceId) -> Option<(i32, i32)> {
         self.windows.root_position(window).ok()
     }

     pub fn resource_id_in_use(&self, resource: crate::XResourceId) -> bool {
         matches!(
             u32::try_from(resource.local.raw()),
             Ok(crate::X_SETUP_DEFAULT_ROOT | crate::X_SETUP_DEFAULT_COLORMAP)
         ) || self.resources.get(resource).is_some()
             || self.graphics_contexts.contains(resource)
             // GLX contexts and drawables are ids a client chose too. Creating one
             // already refuses a collision with the tables above; omitting them here
             // let the reverse happen, so a pixmap could shadow a live pbuffer.
             || self.glx_contexts.contains_key(&resource)
             || self.glx_drawables.contains_key(&resource)
     }
 
     pub fn window_count(&self) -> usize {
         self.windows.len()
     }
 
     pub fn shm_segment_count(&self) -> usize {
         self.shm_segments.len()
     }
 
     pub fn attach_shm_segment(
         &mut self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
         shmid: u32,
         read_only: bool,
         generation: u64,
     ) -> Result<(), XAuthorityRuntimeError> {
         self.shm_segments
             .attach(
                 namespace,
                 segment,
                 crate::XShmBacking::Sysv(shmid),
                 read_only,
                 generation,
             )
             .map_err(Into::into)
     }

     /// Records a segment the client named with a descriptor.
     ///
     /// The mapping is already made -- validating the descriptor is the
     /// caller's business, because only it knows whether the descriptor came
     /// from the client or from us.
     pub fn attach_shm_descriptor_segment(
         &mut self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
         mapping: std::sync::Arc<sophia_sysv_shm::ClientMapping>,
         read_only: bool,
         generation: u64,
     ) -> Result<(), XAuthorityRuntimeError> {
         self.shm_segments
             .attach(
                 namespace,
                 segment,
                 crate::XShmBacking::Descriptor,
                 read_only,
                 generation,
             )
             .map_err(XAuthorityRuntimeError::from)?;
         self.shm_descriptor_mappings.insert(segment, mapping);
         Ok(())
     }

     /// Allocates a segment the server owns, for `CreateSegment`.
     ///
     /// The descriptor is kept until the socket layer takes it for the reply,
     /// because only that layer can put one on the wire.
     pub fn create_shm_descriptor_segment(
         &mut self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
         size: u32,
         read_only: bool,
         generation: u64,
     ) -> Result<(), XAuthorityRuntimeError> {
         let len = usize::try_from(size).map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
         let (mapping, descriptor) = sophia_sysv_shm::DescriptorMapping::create_sealed(len)
             .map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
         self.attach_shm_descriptor_segment(
             namespace,
             segment,
             std::sync::Arc::new(sophia_sysv_shm::ClientMapping::Descriptor(mapping)),
             read_only,
             generation,
         )?;
         self.shm_reply_descriptors.insert(segment, descriptor);
         Ok(())
     }

     /// Takes the descriptor a `CreateSegment` reply owes the client.
     pub fn take_shm_reply_descriptor(
         &mut self,
         segment: crate::XResourceId,
     ) -> Option<std::os::fd::OwnedFd> {
         self.shm_reply_descriptors.remove(&segment)
     }

     /// The mapping behind a segment, whichever way it was named.
     ///
     /// This is what lets the image paths stop knowing about SysV ids: a
     /// segment answers with memory, not with the name it was given.
     pub fn shm_segment_mapping(
         &mut self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
     ) -> Result<std::sync::Arc<sophia_sysv_shm::ClientMapping>, XAuthorityRuntimeError> {
         let record = self.shm_segments.lookup(namespace, segment)?;
         match record.backing {
             crate::XShmBacking::Descriptor => self
                 .shm_descriptor_mappings
                 .get(&segment)
                 .cloned()
                 .ok_or(XAuthorityRuntimeError::UnknownResource),
             crate::XShmBacking::Sysv(shmid) => {
                 if let Some(mapping) = self.shm_mappings.get(&shmid).and_then(Weak::upgrade) {
                     return Ok(mapping);
                 }
                 let mapping = sophia_sysv_shm::ClientMapping::attach_sysv(shmid)
                     .map(std::sync::Arc::new)
                     .map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
                 self.shm_mappings.insert(shmid, Arc::downgrade(&mapping));
                 Ok(mapping)
             }
         }
     }
 
     pub fn detach_shm_segment(
         &mut self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
     ) -> Result<(), XAuthorityRuntimeError> {
         self.shm_segments.detach(namespace, segment)?;
         // Dropping the mapping is what unmaps it. A pixmap still bound to the
         // same memory holds its own reference, so this detaches the segment
         // without pulling the memory out from under it.
         self.shm_descriptor_mappings.remove(&segment);
         Ok(())
     }
 
     pub fn validate_shm_segment_access(
         &self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
     ) -> Result<(), XAuthorityRuntimeError> {
         self.shm_segments
             .lookup(namespace, segment)
             .map(|_| ())
             .map_err(Into::into)
     }
 
     /// Whether a segment refuses writes, which `GetImage` has to ask before
     /// it reads pixels into one.
     pub fn shm_segment_is_read_only(
         &self,
         namespace: NamespaceId,
         segment: crate::XResourceId,
     ) -> Result<bool, XAuthorityRuntimeError> {
         self.shm_segments
             .lookup(namespace, segment)
             .map(|record| record.read_only)
             .map_err(Into::into)
     }
 
     /// The surface a window is presented through, if it is one this
     /// namespace may see.
     ///
     /// `None` for the root, which is no client's window and has no surface:
     /// it is the output, not something presented on it. A caller that needs
     /// to inject at the bare root asks for that explicitly rather than
     /// getting a surface that does not exist.
     pub(crate) fn window_surface(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Option<sophia_protocol::SurfaceId> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)
             .ok()?;
         self.windows.get(window).map(|record| record.surface)
     }

     pub fn validate_window_access(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<(), XAuthorityRuntimeError> {
         if self.is_clipboard_proxy(namespace, window) {
             return Ok(());
         }
         // The window `_NET_SUPPORTING_WM_CHECK` names is authority-owned, like a
         // clipboard proxy above. A client must be able to read its properties to
         // confirm a manager is live, and must never be able to hold or destroy
         // it, so it answers here rather than from the client resource table.
         if window.local.raw() == u64::from(crate::X_SETUP_WM_CHECK_WINDOW) {
             return Ok(());
         }
         self.resources
             .lookup(namespace, window, XResourceKind::Window)
             .map(|_| ())
             .map_err(Into::into)
     }
 
     pub fn window_geometry(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<Rect, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.windows
             .get(window)
             .map(|record| record.geometry)
             .ok_or(XAuthorityRuntimeError::UnknownResource)
     }

     pub fn window_override_redirect(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<bool, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.windows
             .get(window)
             .map(|record| record.override_redirect)
             .ok_or(XAuthorityRuntimeError::UnknownResource)
     }

     pub fn set_window_override_redirect(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         override_redirect: bool,
     ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.windows
             .set_override_redirect(window, override_redirect)
             .map_err(Into::into)
     }

     pub fn set_window_constraints(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         constraints: sophia_protocol::SurfaceConstraints,
     ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.windows
             .set_constraints(window, constraints)
             .map_err(Into::into)
     }

    pub fn set_window_transient_for(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        owner: Option<crate::XResourceId>,
    ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, window, XResourceKind::Window)?;
        let transient_for = owner.is_some();
        let owner_surface = owner.and_then(|owner| {
            self.resources
                .lookup(namespace, owner, XResourceKind::Window)
                .ok()?;
            let (root, _, _) = self.windows.presentation_root_and_offset(owner).ok()?;
            self.windows.get(root).map(|record| record.surface)
        });
        self.windows
            .set_transient_for(window, transient_for, owner_surface)
            .map_err(Into::into)
    }

    pub fn set_window_type_facts(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        facts: crate::XWindowTypeFacts,
    ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, window, XResourceKind::Window)?;
        self.windows
            .set_window_type_facts(window, facts)
            .map_err(Into::into)
    }

    pub fn set_window_parent(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         parent: crate::XResourceId,
     ) -> Result<(), XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         if parent.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             self.resources
                 .lookup(namespace, parent, XResourceKind::Window)?;
         }
        self.change_pointer_anchor_geometry(namespace, |runtime| {
            runtime.windows.set_parent(window, parent).map_err(Into::into)
        })?;
        // A new parent that is not viewable takes a whole subtree with it,
        // without anything in it being unmapped.
        self.revert_focus_if_unviewable(namespace);
        Ok(())
    }

    /// ReparentWindow as the protocol orders it (t184): a mapped window is
    /// unmapped first, given its new parent and position, and mapped again.
    /// The outcome names what the dispatcher reports and the surfaces the
    /// Engine applies, in that order.
    pub fn reparent_window(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        parent: crate::XResourceId,
        x: i16,
        y: i16,
        generation: u64,
    ) -> Result<XWindowReparent, XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, window, XResourceKind::Window)?;
        let old_parent = self
            .windows
            .get(window)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?
            .parent;
        let was_mapped = !matches!(
            self.window_map_state(namespace, window)?,
            crate::XMapState::Unmapped
        );
        let mut surfaces = Vec::new();
        if was_mapped && let Some(surface) = self.unmap_window(namespace, window)? {
            surfaces.push(surface);
        }
        self.set_window_parent(namespace, window, parent)?;
        surfaces.push(self.configure_window_geometry_observed(
            namespace,
            window,
            XWindowGeometryUpdate {
                x: Some(x),
                y: Some(y),
                generation,
                ..XWindowGeometryUpdate::default()
            },
        )?);
        if was_mapped {
            let record_generation = self.windows.get(window).map_or(0, |record| record.generation);
            if let Some(surface) = self.windows.apply(XWindowLifecycleEvent::Mapped {
                id: window,
                generation: record_generation,
            })? {
                surfaces.push(surface);
            }
        }
        Ok(XWindowReparent {
            old_parent,
            was_mapped,
            override_redirect: self.window_override_redirect(namespace, window).unwrap_or(false),
            surfaces,
        })
    }

    pub fn restack_window(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        sibling: Option<crate::XResourceId>,
        stack_mode: Option<u8>,
    ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, window, XResourceKind::Window)?;
        if let Some(sibling) = sibling {
            self.resources
                .lookup(namespace, sibling, XResourceKind::Window)?;
        }
        self.windows
            .restack(window, sibling, stack_mode)
            .map_err(Into::into)
    }

    pub fn window_presentation_root_and_offset(
        &self,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Result<(crate::XResourceId, sophia_protocol::SurfaceId, i32, i32), XAuthorityRuntimeError>
    {
        self.resources
            .lookup(namespace, window, XResourceKind::Window)?;
        let (root, x, y) = self.windows.presentation_root_and_offset(window)?;
        let record = self
            .windows
            .get(root)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        Ok((root, record.surface, x, y))
    }

    pub(crate) fn window_absolute_position(
        &self,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Result<(i32, i32), XAuthorityRuntimeError> {
        self.resources
            .lookup(namespace, window, XResourceKind::Window)?;
        let (root, offset_x, offset_y) = self.windows.presentation_root_and_offset(window)?;
        let root_geometry = self
            .windows
            .get(root)
            .map(|record| record.geometry)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        Ok((
            root_geometry.x.saturating_add(offset_x),
            root_geometry.y.saturating_add(offset_y),
        ))
    }

     pub fn window_parent_and_children(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<(crate::XResourceId, Vec<crate::XResourceId>), XAuthorityRuntimeError> {
         if window.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             return Ok((
                 crate::XResourceId::NONE,
                 // QueryTree lists children bottom to top in stacking order,
                 // which a CirculateWindow or a restack makes differ from
                 // the order they were created in.
                 self.windows.direct_children_bottom_to_top(namespace, window),
             ));
         }
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         let parent = self
             .windows
             .get(window)
             .ok_or(XAuthorityRuntimeError::UnknownResource)?
             .parent;
         Ok((
             parent,
             self.windows.direct_children_bottom_to_top(namespace, window),
         ))
     }
 
     pub fn set_window_visual(
         &mut self,
         window: crate::XResourceId,
         depth: u8,
         visual: u32,
         colormap: crate::XResourceId,
     ) {
         self.window_visuals
             .insert(window, (depth, visual, colormap));
     }
 
     pub fn window_visual(&self, window: crate::XResourceId) -> (u8, u32, crate::XResourceId) {
         self.window_visuals.get(&window).copied().unwrap_or((
             24,
             crate::X_SETUP_DEFAULT_VISUAL,
             crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_COLORMAP), 1),
         ))
     }

     /// Record whether a window was created InputOnly.
     pub fn set_window_input_only(&mut self, window: crate::XResourceId, input_only: bool) {
         if input_only {
             self.input_only_windows.insert(window);
         } else {
             self.input_only_windows.remove(&window);
         }
     }

     /// Whether a window was created InputOnly: it has no pixels, so it can
     /// be neither drawn into nor read back, and no context can match it.
     pub fn window_is_input_only(&self, window: crate::XResourceId) -> bool {
         self.input_only_windows.contains(&window)
     }

     pub fn window_map_state(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<crate::XMapState, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.windows
             .get(window)
             .map(|record| record.map_state)
             .ok_or(XAuthorityRuntimeError::UnknownResource)
     }

     pub fn window_policy_map_pending(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<bool, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.windows
             .get(window)
             .map(|record| record.policy_map_pending)
             .ok_or(XAuthorityRuntimeError::UnknownResource)
     }

     /// Whether an X client may directly mutate this window's geometry.
     /// Once a policy-managed toplevel is mapped, geometry belongs to the
     /// Engine/WM control path; children and override-redirect windows retain
     /// normal client geometry authority.
     pub fn client_controls_window_geometry(
         &self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<bool, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         let record = self
             .windows
             .get(window)
             .ok_or(XAuthorityRuntimeError::UnknownResource)?;
         Ok(record.map_state != crate::XMapState::Viewable
             || record.presentation_role()
                 != sophia_protocol::SurfacePresentationRole::PolicyManaged)
     }
 
     pub fn configure_window_geometry(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         update: XWindowGeometryUpdate,
     ) -> Result<(), XAuthorityRuntimeError> {
         self.configure_window_geometry_observed(namespace, window, update)
             .map(drop)
     }

     pub(crate) fn configure_window_geometry_observed(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         update: XWindowGeometryUpdate,
     ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         self.change_pointer_anchor_geometry(namespace, |runtime| runtime.windows.apply(XWindowLifecycleEvent::Configured {
             id: window,
             x: update.x,
             y: update.y,
             width: update.width,
             height: update.height,
             generation: update.generation,
         })?
         .ok_or(XAuthorityRuntimeError::UnknownResource))
     }
 
     /// Ends an X11 window's lifetime and returns the Sophia surface that the
     /// Engine must remove from its committed snapshot.
     /// Destroy a window and everything below it, descendants first.
     ///
     /// The protocol destroys the whole subtree, and a subtree outliving its
     /// parent is both a lifecycle defect and the reason no descendant
     /// notification could be emitted: nothing was being destroyed to report.
     /// Returned in destruction order so the caller's notifications follow it.
     pub fn destroy_window_subtree(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<Vec<XDestroyedWindow>, XAuthorityRuntimeError> {
         // The root is a valid window with no parent, so destroying it is a
         // request that succeeds and destroys nothing. Refusing it would be a
         // Window error for a window that plainly exists, and would tell the
         // client its own windows had gone down with it.
         if window.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             return Ok(Vec::new());
         }
         // Resolve the whole subtree before destroying any of it. Walking and
         // destroying together would read a tree the destruction is mutating.
         let mut order = Vec::new();
         let mut pending = vec![window];
         while let Some(next) = pending.pop() {
             order.push(next);
             pending.extend(self.windows.direct_children(namespace, next));
         }
         // Deepest first: a parent is destroyed only after its children.
         order.reverse();

         let mut destroyed = Vec::with_capacity(order.len());
         for id in order {
             // Read the map state before destroying: X11 unmaps a mapped
             // window as part of destroying it, and the caller owes its
             // watchers that unmap. Afterwards there is nothing left to ask.
             let was_mapped = self
                 .window_map_state(namespace, id)
                 .is_ok_and(|state| state != crate::XMapState::Unmapped);
             destroyed.push(XDestroyedWindow {
                 window: id,
                 surface: self.destroy_window(namespace, id)?,
                 was_mapped,
                 is_subtree_root: id == window,
             });
         }
         Ok(destroyed)
     }

     pub fn destroy_window(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<sophia_protocol::SurfaceId, XAuthorityRuntimeError> {
         // Validate before publication invalidation; an invalid request has no
         // native effect. Every subtree/teardown destruction reaches this source.
         self.resources.lookup(namespace, window, XResourceKind::Window)?;
         #[cfg(unix)]
         if let Some(source) = self.private_focus_source.clone() {
             return source.destroy_window(self, namespace, window);
         }
         self.destroy_window_effect(namespace, window)
     }

     #[cfg(unix)]
     pub(crate) fn destroy_window_private_effect(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         _permit: &crate::x11_socket::XPrivateWindowDestructionPermit,
     ) -> Result<sophia_protocol::SurfaceId, XAuthorityRuntimeError> {
         self.destroy_window_effect(namespace, window)
     }

     fn destroy_window_effect(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
     ) -> Result<sophia_protocol::SurfaceId, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         let surface = self
             .windows
             .get(window)
             .ok_or(XAuthorityRuntimeError::UnknownResource)?
             .surface;
         let cleared = self.selections.clear_window_owner(
             window,
             &self.windows,
             crate::XSelectionChangeKind::SelectionWindowDestroyed,
         );
         self.retired_selection_ownerships.extend(cleared);
         self.retire_pixmap_export_drawable(window);
         self.windows
             .apply(XWindowLifecycleEvent::Destroyed { id: window })?;
         self.resources.remove(window);
         self.window_allocation.hints.remove(&surface);
         self.window_allocation.preferences.remove(&surface);
         self.input_authority_mut().forget_query_window(namespace, window);
         self.software_buffers.remove(window);
         self.raster_store.remove(window);
         self.render_drop_pictures_of_drawable(window);
         self.forget_window_shapes(window);
         self.window_backgrounds.remove(&window);
         self.window_visuals.remove(&window);
         self.release_window_cursor(window);
         self.input_only_windows.remove(&window);
         self.glx_drawables.retain(|_, record| match record.backing {
             XGlxDrawableBacking::Window(underlying) => underlying != window,
             // Neither borrows a window, so neither is disturbed by one going.
             XGlxDrawableBacking::Pbuffer(_) | XGlxDrawableBacking::Pixmap { .. } => true,
         });
         // Gone rather than merely unviewable, so the focus cannot stay. It
         // used to reset to the root here and ignore revert_to entirely.
         if self.input_focus(namespace).0 == window {
             self.revert_focus_if_unviewable(namespace);
         }
         Ok(surface)
     }
 
     /// Reclaims every supported resource created from a disconnected client's
     /// XID range. Existing-resource references are intentionally not used as
     /// ownership evidence: classic shared-X clients may refer to one another's
     /// resources, while allocation is constrained by the setup range.
     pub fn release_client_resource_range(
         &mut self,
         namespace: NamespaceId,
         range: crate::XWireClientResourceRange,
     ) -> Result<XAuthorityClientResourceRelease, XAuthorityRuntimeError> {
         self.release_client_resource_range_with_save_set(namespace, range, &[])
     }

     /// The save-set walks before the destroy loop: a saved window lives
     /// inside the departing client's subtree, which that loop destroys
     /// whole. Each saved window still alive and outside the range is given
     /// to its nearest ancestor outside the range (the root when none) and
     /// re-mapped if it was mapped, as the protocol has a departing window
     /// manager's clients survive it.
     pub fn release_client_resource_range_with_save_set(
         &mut self,
         namespace: NamespaceId,
         range: crate::XWireClientResourceRange,
         save_set: &[crate::XResourceId],
     ) -> Result<XAuthorityClientResourceRelease, XAuthorityRuntimeError> {
         if !namespace.is_valid() {
             return Err(XAuthorityRuntimeError::InvalidNamespace);
         }
 
         let mut release = XAuthorityClientResourceRelease::default();
         self.apply_save_set(namespace, range, save_set, &mut release)?;
         self.glx_contexts.retain(|id, (owner, _, _)| {
             let owned = *owner == namespace
                 && u32::try_from(id.local.raw()).is_ok_and(|raw| range.owns_new_resource(raw));
             if owned {
                 release.released_glx_contexts = release.released_glx_contexts.saturating_add(1);
             }
             !owned
         });
         // A GLX pixmap going here may be the last referent of a retained
         // backing, and a disconnect must settle that account like any other
         // release. Collected first because the accounting needs `&mut self`.
         let mut dropped_pixmap_backings = Vec::new();
         self.glx_drawables.retain(|id, record| {
             let owned = record.owner == namespace
                 && u32::try_from(id.local.raw()).is_ok_and(|raw| range.owns_new_resource(raw));
             if owned {
                 release.released_glx_windows = release.released_glx_windows.saturating_add(1);
                 if let XGlxDrawableBacking::Pixmap { pixmap: backing, .. } = record.backing {
                     dropped_pixmap_backings.push(backing);
                 }
             }
             !owned
         });
         for backing in dropped_pixmap_backings {
             self.release_retained_referent(backing, false);
         }
         for gc in self
             .graphics_contexts
             .ids_for_namespace_in_client_range(namespace, range)
         {
             self.free_graphics_context(namespace, gc)?;
             release.released_graphics_contexts =
                 release.released_graphics_contexts.saturating_add(1);
         }
         for segment in self
             .shm_segments
             .ids_for_namespace_in_client_range(namespace, range)
         {
             self.detach_shm_segment(namespace, segment)?;
             release.released_shm_segments = release.released_shm_segments.saturating_add(1);
         }
 
         // A departing client's selections end because the client went away,
         // not because someone destroyed a window. Ending them here, before
         // the windows are destroyed, is what makes that the reported cause:
         // the destroy below then finds nothing left to clear.
         let closing_windows: Vec<_> = self
             .resources
             .records_for_namespace_in_client_range(namespace, range)
             .into_iter()
             .filter(|record| record.kind == XResourceKind::Window)
             .map(|record| record.id)
             .collect();
         for window in closing_windows {
             let cleared = self.selections.clear_window_owner(
                 window,
                 &self.windows,
                 crate::XSelectionChangeKind::SelectionClientClosed,
             );
             // Kept with this release, not put on the shared queue: it is
             // drained by whichever request next reaches it, which could route
             // this teardown out of order or before the release completes.
             release.retired_selection_ownerships.extend(cleared);
         }

         // Resource records come back in XID allocation order, which is
         // ordinarily parent-before-child: the exact reverse of what a window
         // vanishing owes its watchers. Order the client's windows deepest-first
         // before destroying any of them, so the notifications this drives
         // report descendants before their ancestors.
         let mut records = self
             .resources
             .records_for_namespace_in_client_range(namespace, range);
         records.sort_by_key(|record| {
             core::cmp::Reverse(match record.kind {
                 XResourceKind::Window => self.windows.depth(record.id),
                 _ => 0,
             })
         });

         for record in records {
             match record.kind {
                 XResourceKind::Window => {
                     // The subtree, not just the named window. A window
                     // contains whatever was reparented into it, including
                     // another client's, and those inferiors cannot outlive
                     // the container that held them: destroying only the named
                     // window would leave a peer's child parented to something
                     // that no longer exists.
                     //
                     // Already destroyed if a deeper record took it as part of
                     // its own subtree, which the deepest-first order makes
                     // ordinary rather than exceptional.
                     if self
                         .resources
                         .lookup(namespace, record.id, XResourceKind::Window)
                         .is_err()
                     {
                         continue;
                     }
                     for destroyed in self.destroy_window_subtree(namespace, record.id)? {
                         release.destroyed_windows.push(destroyed.window);
                         release.removed_surfaces.push(destroyed.surface);
                     }
                 }
                 XResourceKind::Pixmap => {
                     if let Some(handle) = self.free_pixmap(namespace, record.id)? {
                         release.released_dma_bufs.push(handle);
                     }
                     release.released_pixmaps = release.released_pixmaps.saturating_add(1);
                 }
                 XResourceKind::Font => {
                     self.close_font(namespace, record.id)?;
                     release.released_fonts = release.released_fonts.saturating_add(1);
                 }
                 XResourceKind::Cursor => {
                     self.free_cursor(namespace, record.id)?;
                     release.released_cursors = release.released_cursors.saturating_add(1);
                 }
                 XResourceKind::Colormap => {
                     self.free_colormap(namespace, record.id).map_err(|error| match error {
                         crate::XColormapError::Access(error) => error.into(),
                         crate::XColormapError::DuplicateId
                         | crate::XColormapError::UnknownVisual => {
                             XAuthorityRuntimeError::UnknownResource
                         }
                     })?;
                     release.released_colormaps = release.released_colormaps.saturating_add(1);
                 }
                 // The reduced frontend does not persist client atoms or GCs in
                 // the resource table. Remove future records instead of leaking.
                 XResourceKind::Atom | XResourceKind::Property | XResourceKind::GraphicsContext => {
                     self.resources.remove(record.id);
                 }
                 XResourceKind::Fence => {
                     self.resources.remove(record.id);
                     if let Some(handle) = self.dri3_fences.remove(&record.id) {
                         release.released_fences.push(handle);
                     }
                 }
                 XResourceKind::Region => {
                     self.resources.remove(record.id);
                     self.xfixes_regions.remove(&record.id);
                 }
                 // A window may already have swept its pictures. Pixmap-backed
                 // pictures instead release their retained backing one at a time.
                 XResourceKind::Picture => {
                     self.render_release_picture(record.id);
                 }
                 XResourceKind::GlyphSet => {
                     self.resources.remove(record.id);
                     self.render_glyphsets.remove(&record.id);
                 }
                 XResourceKind::SyncCounter => {
                     self.resources.remove(record.id);
                     self.sync_counters.remove(&record.id);
                 }
             }
         }
         // Destroying this client's windows can end selections held by peer
         // windows further down the tree, which are outside the range being
         // released and are reported as destroyed rather than closed. Those
         // reach the shared queue through the ordinary destroy path, so they
         // are taken into the release here, still under the lock that produced
         // them: left behind, they would be drained by whichever request
         // arrived next and routed after the teardown that caused them.
         release
             .retired_selection_ownerships
             .extend(core::mem::take(&mut self.retired_selection_ownerships));
         Ok(release)
     }
 
    /// Applies Engine-owned outer geometry without advancing client drawing
    /// generation.
    pub fn configure_window_from_engine(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        geometry: Rect,
    ) -> Result<Rect, XAuthorityRuntimeError> {
        if geometry.is_empty()
            || geometry.width > i32::from(u16::MAX)
            || geometry.height > i32::from(u16::MAX)
            || geometry.x < i32::from(i16::MIN)
            || geometry.x > i32::from(i16::MAX)
            || geometry.y < i32::from(i16::MIN)
            || geometry.y > i32::from(i16::MAX)
        {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let generation = self
            .windows
            .get(window)
             .ok_or(XAuthorityRuntimeError::UnknownResource)?
             .generation;
         self.configure_window_geometry(
             namespace,
            window,
            XWindowGeometryUpdate {
                x: Some(i16::try_from(geometry.x).expect("validated above")),
                y: Some(i16::try_from(geometry.y).expect("validated above")),
                width: Some(u16::try_from(geometry.width).expect("validated above")),
                height: Some(u16::try_from(geometry.height).expect("validated above")),
                generation,
            },
        )?;
        Ok(geometry)
    }

     pub fn admit_window_from_engine(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         geometry: Rect,
     ) -> Result<Rect, XAuthorityRuntimeError> {
         if geometry.is_empty()
             || geometry.width > i32::from(u16::MAX)
             || geometry.height > i32::from(u16::MAX)
             || geometry.x < i32::from(i16::MIN)
             || geometry.x > i32::from(i16::MAX)
             || geometry.y < i32::from(i16::MIN)
             || geometry.y > i32::from(i16::MAX)
         {
             return Err(XAuthorityRuntimeError::InvalidResource);
         }
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         let record = self
             .windows
             .get(window)
             .ok_or(XAuthorityRuntimeError::UnknownResource)?;
         if !record.policy_map_pending || record.map_state != crate::XMapState::Unmapped {
             return Err(XAuthorityRuntimeError::InvalidResource);
         }
         let generation = record.generation;
         self.configure_window_geometry(
             namespace,
             window,
             XWindowGeometryUpdate {
                 x: Some(i16::try_from(geometry.x).expect("validated above")),
                 y: Some(i16::try_from(geometry.y).expect("validated above")),
                 width: Some(u16::try_from(geometry.width).expect("validated above")),
                 height: Some(u16::try_from(geometry.height).expect("validated above")),
                 generation,
             },
         )?;
         self.windows.apply(XWindowLifecycleEvent::Mapped {
             id: window,
             generation,
         })?;
         Ok(geometry)
     }

    pub fn unmap_window(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Result<Option<AuthoritySurface>, XAuthorityRuntimeError> {
         self.resources
             .lookup(namespace, window, XResourceKind::Window)?;
         let record = self
             .windows
             .get(window)
             .ok_or(XAuthorityRuntimeError::UnknownResource)?;
         let generation = record.generation;
         let surface = self.windows.apply(XWindowLifecycleEvent::Unmapped {
             id: window,
             generation,
         })?;
         // The ordinary way a focus window stops being viewable.
         self.revert_focus_if_unviewable(namespace);
         Ok(surface)
     }
 
     /// Destroy every child of `parent`, each with its own subtree, bottom to
     /// top. The named window itself survives.
     ///
     /// Returned in destruction order so the caller's notifications follow it:
     /// within each child, descendants precede the child, and children follow
     /// stacking order.
     pub fn destroy_direct_subwindows(
         &mut self,
         namespace: NamespaceId,
         parent: crate::XResourceId,
     ) -> Result<Vec<XDestroyedWindow>, XAuthorityRuntimeError> {
         if parent.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             self.resources
                 .lookup(namespace, parent, XResourceKind::Window)?;
         }
         let mut destroyed = Vec::new();
         for child in self
             .windows
             .direct_children_bottom_to_top(namespace, parent)
         {
             destroyed.extend(self.destroy_window_subtree(namespace, child)?);
         }
         Ok(destroyed)
     }

     /// UnmapSubwindows: every mapped direct child, top to bottom in stacking
     /// order, as the protocol orders it. Children already unmapped are
     /// skipped, since an event for them would report a transition that
     /// never happened.
     pub fn unmap_direct_subwindows(
         &mut self,
         namespace: NamespaceId,
         parent: crate::XResourceId,
     ) -> Result<Vec<(crate::XResourceId, AuthoritySurface)>, XAuthorityRuntimeError> {
         if parent.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             self.resources
                 .lookup(namespace, parent, XResourceKind::Window)?;
         }
         let mut unmapped = Vec::new();
         let mut children = self.windows.direct_children_bottom_to_top(namespace, parent);
         children.reverse();
         for window in children {
             if let Some(surface) = self.unmap_window(namespace, window)? {
                 unmapped.push((window, surface));
             }
         }
         Ok(unmapped)
     }

     /// The child CirculateWindow would move, if any: for RaiseLowest the
     /// lowest mapped child occluded by a mapped sibling above it; for
     /// LowerHighest the highest mapped child occluding a mapped sibling
     /// below it. Occlusion is the siblings' rectangles meeting.
     pub fn circulate_candidate(
         &self,
         namespace: NamespaceId,
         parent: crate::XResourceId,
         direction: u8,
     ) -> Result<Option<crate::XResourceId>, XAuthorityRuntimeError> {
         if parent.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             self.resources
                 .lookup(namespace, parent, XResourceKind::Window)?;
         }
         let mapped = self
             .windows
             .direct_children_bottom_to_top(namespace, parent)
             .into_iter()
             .filter(|window| {
                 !matches!(self.window_map_state(namespace, *window), Ok(crate::XMapState::Unmapped))
             })
             .filter_map(|window| self.window_geometry(namespace, window).ok().map(|geometry| (window, geometry)))
             .collect::<Vec<_>>();
         let meets = |a: &sophia_protocol::Rect, b: &sophia_protocol::Rect| {
             a.x < b.x.saturating_add(b.width)
                 && b.x < a.x.saturating_add(a.width)
                 && a.y < b.y.saturating_add(b.height)
                 && b.y < a.y.saturating_add(a.height)
         };
         let candidate = if direction == 0 {
             mapped.iter().enumerate().find(|(index, (_, geometry))| {
                 mapped[index + 1..].iter().any(|(_, above)| meets(geometry, above))
             })
         } else {
             mapped.iter().enumerate().rev().find(|(index, (_, geometry))| {
                 mapped[..*index].iter().any(|(_, below)| meets(geometry, below))
             })
         };
         Ok(candidate.map(|(_, (window, _))| *window))
     }

     /// CirculateWindow, once the candidate is known: to the top for
     /// RaiseLowest, to the bottom for LowerHighest.
     pub fn circulate_window(
         &mut self,
         namespace: NamespaceId,
         window: crate::XResourceId,
         direction: u8,
     ) -> Result<AuthoritySurface, XAuthorityRuntimeError> {
         self.restack_window(namespace, window, None, Some(if direction == 0 { 0 } else { 1 }))
     }

     /// A departing client whose close-down mode retains its range: the
     /// save-set is honoured and its selections end with the connection, as
     /// the reference server ends them, and nothing else is destroyed. The
     /// range is freed later by KillClient, through the ordinary release.
     /// The save-set walk: each saved window still alive and outside the
     /// range is given to its nearest ancestor outside the range (the root
     /// when none) and re-mapped if it was mapped.
     fn apply_save_set(
         &mut self,
         namespace: NamespaceId,
         range: crate::XWireClientResourceRange,
         save_set: &[crate::XResourceId],
         release: &mut XAuthorityClientResourceRelease,
     ) -> Result<(), XAuthorityRuntimeError> {
         for window in save_set {
             let window = *window;
             let owned = u32::try_from(window.local.raw()).is_ok_and(|raw| range.owns_new_resource(raw));
             if owned || self.resources.lookup(namespace, window, XResourceKind::Window).is_err() {
                 continue;
             }
             let Some(record) = self.windows.get(window) else {
                 continue;
             };
             let old_parent = record.parent;
             let mut new_parent = old_parent;
             while new_parent.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT)
                 && u32::try_from(new_parent.local.raw()).is_ok_and(|raw| range.owns_new_resource(raw))
             {
                 new_parent = match self.windows.get(new_parent) {
                     Some(ancestor) => ancestor.parent,
                     None => crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
                 };
             }
             if new_parent == old_parent {
                 continue;
             }
             let was_mapped = !matches!(
                 self.window_map_state(namespace, window),
                 Ok(crate::XMapState::Unmapped)
             );
             if was_mapped {
                 self.unmap_window(namespace, window)?;
             }
             self.set_window_parent(namespace, window, new_parent)?;
             let generation = self.windows.get(window).map_or(0, |record| record.generation);
             let surface = if was_mapped {
                 self.windows.apply(XWindowLifecycleEvent::Mapped {
                     id: window,
                     generation,
                 })?
             } else {
                 None
             };
             let geometry = self.window_geometry(namespace, window).unwrap_or_default();
             release.save_set_reparents.push(crate::XSaveSetReparent {
                 window,
                 old_parent,
                 new_parent,
                 was_mapped,
                 x: i16::try_from(geometry.x).unwrap_or(i16::MAX),
                 y: i16::try_from(geometry.y).unwrap_or(i16::MAX),
                 override_redirect: self.window_override_redirect(namespace, window).unwrap_or(false),
                 surface,
             });
         }
         Ok(())
     }

     pub fn retain_client_resource_range(
         &mut self,
         namespace: NamespaceId,
         range: crate::XWireClientResourceRange,
         save_set: &[crate::XResourceId],
     ) -> Result<XAuthorityClientResourceRelease, XAuthorityRuntimeError> {
         if !namespace.is_valid() {
             return Err(XAuthorityRuntimeError::InvalidNamespace);
         }
         let mut release = XAuthorityClientResourceRelease::default();
         self.apply_save_set(namespace, range, save_set, &mut release)?;
         let closing_windows: Vec<_> = self
             .resources
             .records_for_namespace_in_client_range(namespace, range)
             .into_iter()
             .filter(|record| record.kind == XResourceKind::Window)
             .map(|record| record.id)
             .collect();
         for window in closing_windows {
             let cleared = self.selections.clear_window_owner(
                 window,
                 &self.windows,
                 crate::XSelectionChangeKind::SelectionClientClosed,
             );
             release.retired_selection_ownerships.extend(cleared);
         }
         Ok(release)
     }

     pub fn map_direct_subwindows(
         &mut self,
         namespace: NamespaceId,
         parent: crate::XResourceId,
         generation: u64,
     ) -> Result<Vec<AuthoritySurface>, XAuthorityRuntimeError> {
         if parent.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
             self.resources
                 .lookup(namespace, parent, XResourceKind::Window)?;
         }
         let mut surfaces = Vec::new();
         for window in self.windows.direct_children(namespace, parent) {
             let role = self
                 .windows
                 .get(window)
                 .ok_or(XAuthorityRuntimeError::UnknownResource)?
                 .presentation_role();
             let event = if role
                 == sophia_protocol::SurfacePresentationRole::ClientPositioned
                 || !self.defer_policy_maps
             {
                 XWindowLifecycleEvent::Mapped {
                     id: window,
                     generation,
                 }
             } else {
                 XWindowLifecycleEvent::PolicyPending {
                     id: window,
                     generation,
                 }
             };
             if let Some(surface) = self.windows.apply(event)? {
                 surfaces.push(surface);
             }
         }
         Ok(surfaces)
     }
 
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XWindowGeometryUpdate {
    pub x: Option<i16>,
    pub y: Option<i16>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub generation: u64,
}

/// One window removed by a destroy, and what its removal owes its watchers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XDestroyedWindow {
    pub window: crate::XResourceId,
    pub surface: sophia_protocol::SurfaceId,
    /// Whether it was mapped when destroyed. A mapped window is unmapped as
    /// part of being destroyed, and that unmap is reported before the destroy.
    pub was_mapped: bool,
    /// Whether this is the window the request named rather than one of its
    /// inferiors. Only the named window is unmapped as part of being
    /// destroyed; inferiors are destroyed and owe nobody an unmap.
    pub is_subtree_root: bool,
}

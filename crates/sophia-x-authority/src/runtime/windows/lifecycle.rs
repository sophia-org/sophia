// The window lifecycle the policy and the departing client drive: admission
// from the Engine, unmapping, the subwindow requests, circulation, the
// save-set and retention. Included into `runtime/windows.rs`; split out to
// keep that file within the layout ledger's bound (t026).

impl XAuthorityRuntime {
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
     /// when none), and mapped if it is unmapped -- whether or not it was
     /// reparented, as the protocol's save-set processing has it: "If the
     /// save-set window is unmapped, a MapWindow request is performed on
     /// it (even if it was not an inferior of a window created by the
     /// client)".
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
             let reparented = new_parent != old_parent;
             let was_mapped = !matches!(
                 self.window_map_state(namespace, window),
                 Ok(crate::XMapState::Unmapped)
             );
             if !reparented && was_mapped {
                 continue;
             }
             if reparented {
                 if was_mapped {
                     self.unmap_window(namespace, window)?;
                 }
                 self.set_window_parent(namespace, window, new_parent)?;
             }
             let generation = self.windows.get(window).map_or(0, |record| record.generation);
             let surface = self.windows.apply(XWindowLifecycleEvent::Mapped {
                 id: window,
                 generation,
             })?;
             let geometry = self.window_geometry(namespace, window).unwrap_or_default();
             release.save_set_reparents.push(crate::XSaveSetReparent {
                 window,
                 old_parent,
                 new_parent,
                 was_mapped,
                 input_only: self.window_is_input_only(window),
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
                 // As MapWindow: a subwindow made viewable with no remembered
                 // contents is painted with its background, and its viewable
                 // inferiors with theirs.
                 if self.window_map_state(namespace, window) == Ok(crate::XMapState::Viewable) {
                     for target in self.viewable_subtree(window) {
                         self.paint_window_background(target);
                     }
                 }
             }
         }
         Ok(surfaces)
     }
 
}

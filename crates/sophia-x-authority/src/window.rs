use std::collections::BTreeMap;

use sophia_protocol::{
    AuthorityKind, AuthoritySurface, LayoutNodeKind, NamespaceId, Rect, SurfaceConstraints,
    SurfaceId, SurfacePlacementPreference, SurfacePresentationRole,
};

use crate::{XAuthorityAccessError, XMapState, XResourceId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XWindowRecord {
    pub id: XResourceId,
    pub parent: XResourceId,
    pub surface: SurfaceId,
    pub namespace: NamespaceId,
    pub override_redirect: bool,
    /// The client published `WM_TRANSIENT_FOR`, even when its owner is the
    /// root window or cannot be reduced to an Engine surface.
    pub transient_for: bool,
    pub window_type_kind: LayoutNodeKind,
    pub window_type_placement: SurfacePlacementPreference,
    pub window_type_client_positioned: bool,
    pub presentation_owner: Option<SurfaceId>,
    /// Engine admission is pending for a redirected policy-managed root child.
    /// This is not an X11 map state: the window remains `Unmapped` until the
    /// Engine applies the configure/map decision.
    pub policy_map_pending: bool,
    pub map_state: XMapState,
    pub geometry: Rect,
    pub constraints: SurfaceConstraints,
    pub generation: u64,
    pub stack_rank: u32,
}

impl XWindowRecord {
    pub fn presentation_role(&self) -> SurfacePresentationRole {
        let is_root_child = self.parent.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT);
        if self.override_redirect || self.window_type_client_positioned || !is_root_child {
            SurfacePresentationRole::ClientPositioned
        } else {
            SurfacePresentationRole::PolicyManaged
        }
    }

    pub fn kind(&self) -> LayoutNodeKind {
        if self.transient_for && self.window_type_kind == LayoutNodeKind::Toplevel {
            LayoutNodeKind::Dialog
        } else {
            self.window_type_kind
        }
    }

    pub fn placement_preference(&self) -> SurfacePlacementPreference {
        if self.transient_for {
            SurfacePlacementPreference::Floating
        } else {
            self.window_type_placement
        }
    }

    pub fn authority_surface(&self) -> AuthoritySurface {
        AuthoritySurface {
            authority: AuthorityKind::SophiaX,
            local_id: self.id.local,
            surface: self.surface,
            namespace: Some(self.namespace),
            presentation: self.presentation_role(),
            kind: self.kind(),
            placement_preference: self.placement_preference(),
            presentation_owner: self.presentation_owner,
            stack_rank: self.stack_rank,
            mapped: self.map_state == XMapState::Viewable,
            geometry: self.geometry,
            constraints: self.constraints,
            generation: self.generation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XWindowLifecycleEvent {
    Created {
        id: XResourceId,
        surface: SurfaceId,
        namespace: NamespaceId,
        geometry: Rect,
        constraints: SurfaceConstraints,
        generation: u64,
    },
    Mapped {
        id: XResourceId,
        generation: u64,
    },
    PolicyPending {
        id: XResourceId,
        generation: u64,
    },
    Unmapped {
        id: XResourceId,
        generation: u64,
    },
    Configured {
        id: XResourceId,
        x: Option<i16>,
        y: Option<i16>,
        width: Option<u16>,
        height: Option<u16>,
        generation: u64,
    },
    Destroyed {
        id: XResourceId,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XWindowTable {
    windows: BTreeMap<XResourceId, XWindowRecord>,
}

impl XWindowTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(
        &mut self,
        event: XWindowLifecycleEvent,
    ) -> Result<Option<AuthoritySurface>, XAuthorityAccessError> {
        match event {
            XWindowLifecycleEvent::Created {
                id,
                surface,
                namespace,
                geometry,
                constraints,
                generation,
            } => {
                if !id.is_valid() {
                    return Err(XAuthorityAccessError::InvalidResource);
                }
                if !surface.is_valid() {
                    return Err(XAuthorityAccessError::InvalidResource);
                }
                if !namespace.is_valid() {
                    return Err(XAuthorityAccessError::InvalidNamespace);
                }

                let record = XWindowRecord {
                    id,
                    parent: XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
                    surface,
                    namespace,
                    override_redirect: false,
                    transient_for: false,
                    window_type_kind: LayoutNodeKind::Toplevel,
                    window_type_placement: SurfacePlacementPreference::Default,
                    window_type_client_positioned: false,
                    presentation_owner: None,
                    policy_map_pending: false,
                    map_state: XMapState::Unmapped,
                    geometry,
                    constraints,
                    generation,
                    stack_rank: self.windows.len().try_into().unwrap_or(u32::MAX),
                };
                let authority_surface = record.authority_surface();
                self.windows.insert(id, record);
                Ok(Some(authority_surface))
            }
            // Mapping changes the X11 lifecycle state but does not create a
            // compositor transaction.  In particular, its X11 request
            // sequence must not overwrite the generation used as the
            // predecessor of the next pixel transaction.
            XWindowLifecycleEvent::Mapped { id, generation: _ } => {
                let parent = self
                    .windows
                    .get(&id)
                    .ok_or(XAuthorityAccessError::UnknownResource)?
                    .parent;
                let parent_viewable = parent.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT)
                    || self
                        .windows
                        .get(&parent)
                        .is_some_and(|parent| parent.map_state == XMapState::Viewable);
                let record = self.windows.get_mut(&id).expect("window checked above");
                if record.map_state != XMapState::Unmapped {
                    return Ok(None);
                }
                record.policy_map_pending = false;
                record.map_state = if parent_viewable {
                    XMapState::Viewable
                } else {
                    XMapState::Unviewable
                };
                let surface = record.authority_surface();
                if parent_viewable {
                    self.promote_unviewable_descendants(id);
                }
                Ok(Some(surface))
            }
            XWindowLifecycleEvent::PolicyPending { id, generation: _ } => {
                let record = self
                    .windows
                    .get_mut(&id)
                    .ok_or(XAuthorityAccessError::UnknownResource)?;
                if record.map_state != XMapState::Unmapped || record.policy_map_pending {
                    return Ok(None);
                }
                record.policy_map_pending = true;
                Ok(Some(record.authority_surface()))
            }
            XWindowLifecycleEvent::Unmapped { id, generation: _ } => {
                let record = self
                    .windows
                    .get_mut(&id)
                    .ok_or(XAuthorityAccessError::UnknownResource)?;
                if record.map_state == XMapState::Unmapped && !record.policy_map_pending {
                    return Ok(None);
                }
                record.policy_map_pending = false;
                record.map_state = XMapState::Unmapped;
                let surface = record.authority_surface();
                self.demote_viewable_descendants(id);
                Ok(Some(surface))
            }
            XWindowLifecycleEvent::Configured {
                id,
                x,
                y,
                width,
                height,
                generation: _,
            } => {
                let record = self
                    .windows
                    .get_mut(&id)
                    .ok_or(XAuthorityAccessError::UnknownResource)?;
                if let Some(x) = x {
                    record.geometry.x = i32::from(x);
                }
                if let Some(y) = y {
                    record.geometry.y = i32::from(y);
                }
                if let Some(width) = width {
                    record.geometry.width = i32::from(width);
                }
                if let Some(height) = height {
                    record.geometry.height = i32::from(height);
                }
                Ok(Some(record.authority_surface()))
            }
            XWindowLifecycleEvent::Destroyed { id } => {
                self.windows.remove(&id);
                let root = XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
                for record in self.windows.values_mut() {
                    if record.parent == id {
                        record.parent = root;
                    }
                }
                Ok(None)
            }
        }
    }

    pub fn get(&self, id: XResourceId) -> Option<&XWindowRecord> {
        self.windows.get(&id)
    }

    pub(crate) fn presentation_for_surface(&self, surface: SurfaceId) -> Option<&XWindowRecord> {
        self.windows
            .values()
            .find(|record| record.surface == surface)
    }

    pub fn set_override_redirect(
        &mut self,
        id: XResourceId,
        override_redirect: bool,
    ) -> Result<AuthoritySurface, XAuthorityAccessError> {
        let record = self
            .windows
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        record.override_redirect = override_redirect;
        Ok(record.authority_surface())
    }

    pub fn set_constraints(
        &mut self,
        id: XResourceId,
        constraints: SurfaceConstraints,
    ) -> Result<AuthoritySurface, XAuthorityAccessError> {
        let record = self
            .windows
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        record.constraints = constraints;
        Ok(record.authority_surface())
    }

    pub fn set_transient_for(
        &mut self,
        id: XResourceId,
        transient_for: bool,
        owner: Option<SurfaceId>,
    ) -> Result<AuthoritySurface, XAuthorityAccessError> {
        let record = self
            .windows
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        record.transient_for = transient_for;
        record.presentation_owner = transient_for
            .then_some(owner)
            .flatten()
            .filter(|owner| *owner != record.surface);
        Ok(record.authority_surface())
    }

    pub fn set_window_type_facts(
        &mut self,
        id: XResourceId,
        facts: crate::XWindowTypeFacts,
    ) -> Result<AuthoritySurface, XAuthorityAccessError> {
        let record = self
            .windows
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        record.window_type_kind = facts.kind;
        record.window_type_placement = facts.placement_preference;
        record.window_type_client_positioned = facts.client_positioned;
        Ok(record.authority_surface())
    }

    /// Distance from the root: a root child is one, its own child two.
    ///
    /// Ordering a client's windows by descending depth destroys them
    /// deepest-first, so a parent is never reported gone before its children.
    pub fn depth(&self, id: XResourceId) -> usize {
        let mut depth: usize = 0;
        let mut current = id;
        // The root holds no record, so the walk ordinarily ends there. The
        // bound is belt-and-braces: a malformed cycle must not spin forever.
        while let Some(record) = self.windows.get(&current) {
            depth = depth.saturating_add(1);
            if depth > self.windows.len() {
                break;
            }
            current = record.parent;
        }
        depth
    }

    pub fn set_parent(
        &mut self,
        id: XResourceId,
        parent: XResourceId,
    ) -> Result<(), XAuthorityAccessError> {
        if id == parent {
            return Err(XAuthorityAccessError::InvalidResource);
        }
        let mut ancestor = parent;
        while let Some(record) = self.windows.get(&ancestor) {
            if record.parent == id {
                return Err(XAuthorityAccessError::InvalidResource);
            }
            ancestor = record.parent;
        }
        self.windows
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?
            .parent = parent;
        self.recompute_subtree_viewability(id);
        Ok(())
    }

    pub fn restack(
        &mut self,
        id: XResourceId,
        sibling: Option<XResourceId>,
        mode: Option<u8>,
    ) -> Result<AuthoritySurface, XAuthorityAccessError> {
        let record = self
            .windows
            .get(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        let parent = record.parent;
        if sibling.is_some_and(|sibling| {
            sibling == id
                || self
                    .windows
                    .get(&sibling)
                    .is_none_or(|record| record.parent != parent)
        }) {
            return Err(XAuthorityAccessError::InvalidResource);
        }
        let (own_rank, own_geometry, own_mapped) = (
            record.stack_rank,
            record.geometry,
            record.map_state != XMapState::Unmapped,
        );
        let mut siblings = self
            .windows
            .values()
            .filter(|record| record.parent == parent && record.id != id)
            .map(|record| (record.stack_rank, record.id))
            .collect::<Vec<_>>();
        siblings.sort_unstable();
        let sibling_index = sibling.and_then(|sibling| {
            siblings
                .iter()
                .position(|(_, candidate)| *candidate == sibling)
        });
        // Whether `other` occludes this window or is occluded by it: both
        // mapped and their extents meet, with the occluder above.
        let meets = |a: &Rect, b: &Rect| {
            a.x < b.x.saturating_add(b.width)
                && b.x < a.x.saturating_add(a.width)
                && a.y < b.y.saturating_add(b.height)
                && b.y < a.y.saturating_add(a.height)
        };
        let overlaps = |other: XResourceId| {
            own_mapped
                && self.windows.get(&other).is_some_and(|record| {
                    record.map_state != XMapState::Unmapped
                        && meets(&own_geometry, &record.geometry)
                })
        };
        let rank_of = |other: XResourceId| {
            self.windows
                .get(&other)
                .map_or(0, |record| record.stack_rank)
        };
        let occluded_by = |other: XResourceId| rank_of(other) > own_rank && overlaps(other);
        let occludes = |other: XResourceId| rank_of(other) < own_rank && overlaps(other);
        let any_above_occludes = || siblings.iter().any(|(_, other)| occluded_by(*other));
        let occludes_any_below = || siblings.iter().any(|(_, other)| occludes(*other));
        // The protocol's stack modes, as the reference decides them: Above
        // and Below place the window against the sibling or at an end;
        // TopIf, BottomIf and Opposite move it only when the named sibling
        // (or, with none named, any sibling) occludes it or is occluded by
        // it, and otherwise leave the order as it is.
        let top = siblings.len();
        let index = match (mode, sibling, sibling_index) {
            (Some(0), _, Some(index)) => index.saturating_add(1),
            (Some(0), _, None) => top,
            (Some(1), _, Some(index)) => index,
            (Some(1), _, None) => 0,
            (Some(2), Some(sibling), _) => {
                if occluded_by(sibling) {
                    top
                } else {
                    return Ok(record.authority_surface());
                }
            }
            (Some(2), None, _) => {
                if any_above_occludes() {
                    top
                } else {
                    return Ok(record.authority_surface());
                }
            }
            (Some(3), Some(sibling), _) => {
                if occludes(sibling) {
                    0
                } else {
                    return Ok(record.authority_surface());
                }
            }
            (Some(3), None, _) => {
                if occludes_any_below() {
                    0
                } else {
                    return Ok(record.authority_surface());
                }
            }
            (Some(4), Some(sibling), _) => {
                if occluded_by(sibling) {
                    top
                } else if occludes(sibling) {
                    0
                } else {
                    return Ok(record.authority_surface());
                }
            }
            (Some(4), None, _) => {
                if any_above_occludes() {
                    top
                } else if occludes_any_below() {
                    0
                } else {
                    return Ok(record.authority_surface());
                }
            }
            _ => top,
        };
        siblings.insert(index.min(siblings.len()), (0, id));
        for (rank, (_, window)) in siblings.into_iter().enumerate() {
            if let Some(record) = self.windows.get_mut(&window) {
                record.stack_rank = u32::try_from(rank).unwrap_or(u32::MAX);
            }
        }
        Ok(self
            .windows
            .get(&id)
            .expect("restacked window remains present")
            .authority_surface())
    }

    fn promote_unviewable_descendants(&mut self, parent: XResourceId) {
        let children = self.direct_children_any_namespace(parent);
        for child in children {
            let promoted = self.windows.get_mut(&child).is_some_and(|record| {
                if record.map_state == XMapState::Unviewable {
                    record.map_state = XMapState::Viewable;
                    true
                } else {
                    false
                }
            });
            if promoted
                || self
                    .windows
                    .get(&child)
                    .is_some_and(|record| record.map_state == XMapState::Viewable)
            {
                self.promote_unviewable_descendants(child);
            }
        }
    }

    fn demote_viewable_descendants(&mut self, parent: XResourceId) {
        let children = self.direct_children_any_namespace(parent);
        for child in children {
            let demoted = self.windows.get_mut(&child).is_some_and(|record| {
                if record.map_state == XMapState::Viewable {
                    record.map_state = XMapState::Unviewable;
                    true
                } else {
                    false
                }
            });
            if demoted {
                self.demote_viewable_descendants(child);
            }
        }
    }

    fn recompute_subtree_viewability(&mut self, id: XResourceId) {
        let Some(record) = self.windows.get(&id) else {
            return;
        };
        if record.map_state == XMapState::Unmapped {
            self.demote_viewable_descendants(id);
            return;
        }
        let parent = record.parent;
        let parent_viewable = parent.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT)
            || self
                .windows
                .get(&parent)
                .is_some_and(|parent| parent.map_state == XMapState::Viewable);
        if let Some(record) = self.windows.get_mut(&id) {
            record.map_state = if parent_viewable {
                XMapState::Viewable
            } else {
                XMapState::Unviewable
            };
        }
        if parent_viewable {
            self.promote_unviewable_descendants(id);
        } else {
            self.demote_viewable_descendants(id);
        }
    }

    pub(crate) fn direct_children_any_namespace(&self, parent: XResourceId) -> Vec<XResourceId> {
        self.windows
            .values()
            .filter(|record| record.parent == parent)
            .map(|record| record.id)
            .collect()
    }

    /// Direct children bottom to top.
    ///
    /// `direct_children` returns them in id order, which is the order the store
    /// happens to hold them in. Stacking order is a different fact, maintained
    /// as `stack_rank` by `restack`, and the protocol asks for it wherever an
    /// operation walks siblings.
    pub fn direct_children_bottom_to_top(
        &self,
        namespace: NamespaceId,
        parent: XResourceId,
    ) -> Vec<XResourceId> {
        let mut children = self
            .windows
            .values()
            .filter(|record| record.namespace == namespace && record.parent == parent)
            .map(|record| (record.stack_rank, record.id))
            .collect::<Vec<_>>();
        children.sort_unstable();
        children.into_iter().map(|(_, id)| id).collect()
    }

    pub fn direct_children(&self, namespace: NamespaceId, parent: XResourceId) -> Vec<XResourceId> {
        self.windows
            .values()
            .filter(|record| record.namespace == namespace && record.parent == parent)
            .map(|record| record.id)
            .collect()
    }

    /// Where a window's origin sits in root coordinates.
    ///
    /// This is what a client is asking for when it translates a point to the
    /// root, and it is how a toolkit decides where on screen to put a menu:
    /// it takes the position of the widget's window and offsets the popup
    /// from there. The walk includes the toplevel's own geometry, which is
    /// the whole difference between this and `presentation_root_and_offset`
    /// -- that one measures a descendant's offset *within* its toplevel and
    /// therefore stops before adding it.
    /// The visibility of each viewable window among `candidates`, as the
    /// protocol reports it: Unobscured (0), PartiallyObscured (1) or
    /// FullyObscured (2), from the viewable siblings stacked above it and
    /// above each of its ancestors, each clipped to its own ancestors.
    /// Inferiors never obscure their ancestors, an InputOnly window obscures
    /// nothing and is never reported, and the root is never reported. Only
    /// the candidates are computed: a namespace may hold hundreds of
    /// viewable windows while a handful selected VisibilityChange, and the
    /// stacking index is built once per call.
    pub fn visibility_states(
        &self,
        namespace: NamespaceId,
        input_only: &std::collections::BTreeSet<XResourceId>,
        candidates: &[XResourceId],
    ) -> Vec<(XResourceId, u8)> {
        let root = u64::from(crate::X_SETUP_DEFAULT_ROOT);
        let is_root = |id: XResourceId| id.local.raw() == root;
        // Viewable, pixel-bearing children of each parent, bottom to top.
        let mut children: BTreeMap<XResourceId, Vec<(u32, XResourceId)>> = BTreeMap::new();
        for record in self.windows.values() {
            if record.namespace == namespace
                && record.map_state == XMapState::Viewable
                && !is_root(record.id)
                && !input_only.contains(&record.id)
            {
                children
                    .entry(record.parent)
                    .or_default()
                    .push((record.stack_rank, record.id));
            }
        }
        for stack in children.values_mut() {
            stack.sort_unstable();
        }
        let root_rect = |id: XResourceId| -> Option<Rect> {
            let record = self.windows.get(&id)?;
            let (x, y) = self.root_position(id).ok()?;
            Some(Rect {
                x,
                y,
                width: record.geometry.width,
                height: record.geometry.height,
            })
        };
        let chain = |id: XResourceId| {
            let mut chain = vec![id];
            let mut current = id;
            while let Some(record) = self.windows.get(&current) {
                if is_root(record.parent) || chain.len() > 64 {
                    break;
                }
                chain.push(record.parent);
                current = record.parent;
            }
            chain
        };
        let clip_to_ancestors = |ancestors: &[XResourceId], rect: Rect| -> Option<Rect> {
            ancestors.iter().try_fold(rect, |rect, ancestor| {
                visibility_clip(rect, root_rect(*ancestor)?)
            })
        };
        let mut states = Vec::new();
        for candidate in candidates {
            let Some(record) = self.windows.get(candidate) else {
                continue;
            };
            if record.namespace != namespace
                || record.map_state != XMapState::Viewable
                || is_root(record.id)
                || input_only.contains(&record.id)
            {
                continue;
            }
            let chain = chain(record.id);
            let Some(own) =
                root_rect(record.id).and_then(|rect| clip_to_ancestors(&chain[1..], rect))
            else {
                states.push((record.id, 2));
                continue;
            };
            let own_area = visibility_area(own);
            if own_area == 0 {
                states.push((record.id, 0));
                continue;
            }
            let mut pieces = vec![own];
            'levels: for (depth, element) in chain.iter().enumerate() {
                let Some(element_record) = self.windows.get(element) else {
                    continue;
                };
                let ancestors = &chain[depth + 1..];
                let Some(stack) = children.get(&element_record.parent) else {
                    continue;
                };
                let above = stack
                    .iter()
                    .filter(|(rank, id)| *rank > element_record.stack_rank && *id != *element);
                for (_, sibling) in above {
                    let Some(rect) =
                        root_rect(*sibling).and_then(|rect| clip_to_ancestors(ancestors, rect))
                    else {
                        continue;
                    };
                    pieces = visibility_subtract(pieces, rect);
                    if pieces.is_empty() {
                        break 'levels;
                    }
                }
            }
            let visible = pieces
                .iter()
                .map(|rect| visibility_area(*rect))
                .sum::<i64>();
            states.push((
                record.id,
                if visible == 0 {
                    2
                } else if visible == own_area {
                    0
                } else {
                    1
                },
            ));
        }
        states
    }

    pub fn root_position(&self, id: XResourceId) -> Result<(i32, i32), XAuthorityAccessError> {
        if id.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            return Ok((0, 0));
        }
        let mut current = id;
        let mut x = 0i32;
        let mut y = 0i32;
        loop {
            let record = self
                .windows
                .get(&current)
                .ok_or(XAuthorityAccessError::UnknownResource)?;
            x = x.saturating_add(record.geometry.x);
            y = y.saturating_add(record.geometry.y);
            if record.parent.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
                return Ok((x, y));
            }
            current = record.parent;
        }
    }

    pub fn presentation_root_and_offset(
        &self,
        id: XResourceId,
    ) -> Result<(XResourceId, i32, i32), XAuthorityAccessError> {
        let mut current = id;
        let mut x = 0i32;
        let mut y = 0i32;
        loop {
            let record = self
                .windows
                .get(&current)
                .ok_or(XAuthorityAccessError::UnknownResource)?;
            if record.parent.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) {
                return Ok((current, x, y));
            }
            x = x.saturating_add(record.geometry.x);
            y = y.saturating_add(record.geometry.y);
            current = record.parent;
        }
    }

    pub fn advance_generation(
        &mut self,
        id: XResourceId,
        expected: u64,
    ) -> Result<u64, XAuthorityAccessError> {
        let record = self
            .windows
            .get_mut(&id)
            .ok_or(XAuthorityAccessError::UnknownResource)?;
        if record.generation != expected {
            return Err(XAuthorityAccessError::StaleGeneration);
        }
        let next = expected
            .checked_add(1)
            .ok_or(XAuthorityAccessError::InvalidResource)?;
        record.generation = next;
        Ok(next)
    }

    pub fn ids_for_namespace(&self, namespace: NamespaceId) -> Vec<XResourceId> {
        self.windows
            .values()
            .filter(|record| record.namespace == namespace)
            .map(|record| record.id)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

fn visibility_area(rect: Rect) -> i64 {
    i64::from(rect.width.max(0)) * i64::from(rect.height.max(0))
}

fn visibility_clip(rect: Rect, by: Rect) -> Option<Rect> {
    let left = rect.x.max(by.x);
    let top = rect.y.max(by.y);
    let right = rect
        .x
        .saturating_add(rect.width)
        .min(by.x.saturating_add(by.width));
    let bottom = rect
        .y
        .saturating_add(rect.height)
        .min(by.y.saturating_add(by.height));
    (right > left && bottom > top).then(|| Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// The pieces of `pieces` outside `cut`: each piece that meets the cut is
/// replaced by the up to four strips around it, so the pieces stay disjoint
/// and their areas add up to the visible area.
fn visibility_subtract(pieces: Vec<Rect>, cut: Rect) -> Vec<Rect> {
    let mut out = Vec::with_capacity(pieces.len() + 3);
    for piece in pieces {
        let Some(hit) = visibility_clip(piece, cut) else {
            out.push(piece);
            continue;
        };
        let piece_right = piece.x.saturating_add(piece.width);
        let piece_bottom = piece.y.saturating_add(piece.height);
        let hit_right = hit.x.saturating_add(hit.width);
        let hit_bottom = hit.y.saturating_add(hit.height);
        if hit.y > piece.y {
            out.push(Rect {
                x: piece.x,
                y: piece.y,
                width: piece.width,
                height: hit.y - piece.y,
            });
        }
        if hit_bottom < piece_bottom {
            out.push(Rect {
                x: piece.x,
                y: hit_bottom,
                width: piece.width,
                height: piece_bottom - hit_bottom,
            });
        }
        if hit.x > piece.x {
            out.push(Rect {
                x: piece.x,
                y: hit.y,
                width: hit.x - piece.x,
                height: hit.height,
            });
        }
        if hit_right < piece_right {
            out.push(Rect {
                x: hit_right,
                y: hit.y,
                width: piece_right - hit_right,
                height: hit.height,
            });
        }
    }
    out
}

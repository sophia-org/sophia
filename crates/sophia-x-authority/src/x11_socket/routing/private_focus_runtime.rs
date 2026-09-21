/// Bound once from actual connection setup. The registry retains only a Weak
/// runtime reference, so keeping this producer with the runtime creates no
/// strong reference cycle. Its methods cannot publish caller-supplied results.
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct XPrivateFocusRuntimeSource {
    routing: XServerFrontendRouteRegistry,
}

#[cfg(unix)]
impl core::fmt::Debug for XPrivateFocusRuntimeSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("XPrivateFocusRuntimeSource")
            .finish_non_exhaustive()
    }
}

/// Only the bound source can enter the runtime's observed destruction body.
#[cfg(unix)]
pub(crate) struct XPrivateWindowDestructionPermit(());

#[cfg(unix)]
impl XPrivateFocusRuntimeSource {
    pub(crate) fn same_origin(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.routing.clients, &other.routing.clients)
            && Arc::ptr_eq(
                &self.routing.private_applied,
                &other.routing.private_applied,
            )
    }

    pub(crate) fn destroy_window(
        &self,
        runtime: &mut XAuthorityRuntime,
        namespace: NamespaceId,
        window: XResourceId,
    ) -> Result<SurfaceId, crate::XAuthorityRuntimeError> {
        let permit = XPrivateWindowDestructionPermit(());
        let Some(owner) = self.routing.private_applied.get().filter(|owner| {
            owner.namespace == namespace && runtime.input_focus(namespace).0 == window
        }) else {
            return runtime.destroy_window_private_effect(namespace, window, &permit);
        };
        let unavailable = crate::XAuthorityRuntimeError::FocusAuthorityUnavailable;
        // Caller already owns the outer runtime. Common remains held through
        // the real mutation, but neither clients nor publication is held into
        // the body's X-authority access. There is no output/socket operation.
        owner
            .controller
            .under_common(|_| {
                if !owner.ready.load(Ordering::Acquire) {
                    return Err(unavailable);
                }
                let (generation, connection) = {
                    let clients = self.routing.clients.lock().map_err(|_| unavailable)?;
                    let mut state = owner.publication.lock().map_err(|_| unavailable)?;
                    let generation = owner
                        .next_focus_claim
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| {
                            next.checked_add(1)
                        })
                        .map_err(|_| unavailable)?;
                    let connection = state.focus.and_then(|route| {
                        clients
                            .get(&route.client)
                            .map(|entry| entry.connection_state.clone())
                    });
                    state.focus_generation = generation;
                    state.begin_focus_change().map_err(|_| unavailable)?;
                    if let Some(connection) = connection.as_ref().and_then(|slot| slot.get()) {
                        if connection.namespace != namespace {
                            return Err(unavailable);
                        }
                        connection
                            .applied_focus_generation
                            .store(generation, Ordering::Release);
                    }
                    (generation, connection)
                };
                // The effect lives here, not behind a caller-supplied success bit.
                // Error/unwind after invalidation leaves publication unavailable.
                let surface = runtime.destroy_window_private_effect(namespace, window, &permit)?;
                // Destroying the focus window reverts the focus, and where it
                // goes is the revert_to the client supplied rather than the
                // root. This used to require the root and refuse anything
                // else, which is what made destroy ignore revert_to. What
                // still has to hold is that the focus left the window that no
                // longer exists.
                let (reverted, reverted_revert_to) = runtime.input_focus(namespace);
                if reverted == window {
                    return Err(unavailable);
                }
                if let Some(connection) = connection.as_ref().and_then(|slot| slot.get()) {
                    connection
                        .focused_projection
                        .store(reverted.local.raw(), Ordering::Release);
                    // Old output/dependent continuations can no longer answer for
                    // this projection, even when raw window values later recur.
                    connection
                        .applied_focus_generation
                        .store(0, Ordering::Release);
                }
                let mut state = owner.publication.lock().map_err(|_| unavailable)?;
                if state.focus_generation != generation {
                    return Err(unavailable);
                }
                state.focus = None;
                state.focus_window = reverted;
                state.focus_revert_to = reverted_revert_to;
                state.published = true;
                // This publishes no key target. DestroyNotify, selection/hierarchy
                // retirement and native/recipient debt remain their own owners'
                // obligations; this is not a receipt for any of those effects.
                Ok(surface)
            })
            .map_err(|_| unavailable)?
    }
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    /// Publish the focus an instance starts with, if it is the one the
    /// applied owner was built assuming.
    ///
    /// A fresh publication describes focus on the root, reverting to parent,
    /// and is unpublished: nothing has been applied yet, and publication is
    /// the statement that what the view routes by has been fully applied and
    /// its output owed to nobody. For an instance whose runtime has exactly
    /// that focus, the statement is already true, and leaving it unsaid makes
    /// every pointer route refuse until some client happens to change focus.
    /// So it is said here, once, by the service that prepared the runner,
    /// holding the runtime to compare against.
    ///
    /// Nothing is published if the runtime's focus is anything else: a focus
    /// retained from an earlier invocation names a window this owner has no
    /// route for, and the next focus change will publish it properly. And
    /// nothing is published if a focus change has already begun, because
    /// that change owns the publication from here on.
    ///
    /// Reports whether it published.
    pub(crate) fn publish_prepared_focus(
        &self,
        runtime: &XAuthorityRuntime,
    ) -> Result<bool, PrivateAppliedRegistryRefusal> {
        let owner = self
            .private_applied
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::NoPrivateOwner)?;
        let mut state = owner
            .publication
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::PublicationUnavailable)?;
        if state.published || state.revision != 0 || state.focus.is_some() {
            return Ok(false);
        }
        if runtime.input_focus(owner.namespace) != (state.focus_window, state.focus_revert_to) {
            return Ok(false);
        }
        state.published = true;
        Ok(true)
    }

    /// Observe where an instance's pointer starts: over the root, at the
    /// centre of the screen, which is where the reference server puts it
    /// before anything has moved it.
    ///
    /// A key needs the pointer's position, for the coordinates it carries
    /// and for the path that decides which window under the focus receives
    /// it, and the executor takes that position only from an observation.
    /// A native source provides one the first time the pointer crosses a
    /// surface; a headless instance, or one whose clients inject before any
    /// pointer has crossed anything, has none, and could not deliver a key
    /// at all. This is that first observation, made once, only when the
    /// namespace has been prepared and nothing has observed the pointer yet.
    /// It invents no surface: the observation is over the root, and every
    /// path that reads it treats the root as the bare screen.
    ///
    /// Taken under common, as every observation is, and reports whether it
    /// observed.
    pub(crate) fn publish_prepared_pointer(
        &self,
        runtime: &XAuthorityRuntime,
    ) -> Result<bool, PrivateAppliedRegistryRefusal> {
        let owner = self
            .private_applied
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::NoPrivateOwner)?;
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let geometry = runtime
            .drawable_facts(owner.namespace, root)
            .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?
            .geometry;
        let centre_x = clamp_input_coordinate(f64::from(geometry.width / 2));
        let centre_y = clamp_input_coordinate(f64::from(geometry.height / 2));
        owner
            .controller
            .under_common(|_| {
                let mut authority = self
                    .input_authority
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?;
                if !authority.has_ordered_namespace(owner.namespace)
                    || authority
                        .pointer_query_state(owner.namespace)
                        .position
                        .is_some()
                {
                    return Ok(false);
                }
                authority.observe_query_input(
                    owner.namespace,
                    root,
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Motion,
                        surface: crate::ROOT_POINTER_SURFACE,
                        root_x: centre_x,
                        root_y: centre_y,
                        event_x: centre_x,
                        event_y: centre_y,
                        state: 0,
                        time_msec: 0,
                    }),
                );
                Ok(true)
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?
    }
}

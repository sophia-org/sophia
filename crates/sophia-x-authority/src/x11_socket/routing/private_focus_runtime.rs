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
                let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
                if runtime.input_focus(namespace) != (root, 1) {
                    return Err(unavailable);
                }
                if let Some(connection) = connection.as_ref().and_then(|slot| slot.get()) {
                    connection
                        .focused_projection
                        .store(root.local.raw(), Ordering::Release);
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
                state.focus_window = root;
                state.focus_revert_to = 1;
                state.published = true;
                // This publishes no key target. DestroyNotify, selection/hierarchy
                // retirement and native/recipient debt remain their own owners'
                // obligations; this is not a receipt for any of those effects.
                Ok(surface)
            })
            .map_err(|_| unavailable)?
    }
}

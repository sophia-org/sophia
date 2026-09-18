// Who is watching which selection, and what happens when one stops listening.
//
// Split out of the route registry because that file reached the length the
// layout audit refuses. These belong together: one key, one namespace rule,
// and the retirement paths that clear them.

/// Who is watching which selection, and for which of its three causes.
///
/// Keyed by subscribing client, the window it named, and the selection atom:
/// one client may watch several selections, and the same selection through
/// different windows, so none of the three alone identifies a subscription.
/// The value carries the namespace the subscription belongs to, because atoms
/// are global and the atom alone does not say whose selection it is.
#[cfg(unix)]
type XFixesSelectionSubscriptions =
    Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId, u32), (NamespaceId, u32)>>>;

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn select_xfixes_selection_input(
        &self,
        client: XServerFrontendClientId,
        namespace: NamespaceId,
        window: XResourceId,
        selection: u32,
        mask: u32,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut subscriptions = self
            .xfixes_selection_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        // A mask of zero is how a client stops watching, so it removes the
        // subscription rather than recording an interest in nothing.
        if mask == 0 {
            subscriptions.remove(&(client, window, selection));
        } else {
            subscriptions.insert((client, window, selection), (namespace, mask));
        }
        Ok(())
    }

    /// Who is owed an event for `selection`, and through which window.
    ///
    /// Filtered by subtype: a subscriber hears only about the causes it asked
    /// for, and one that selected none of them is not a recipient at all.
    fn xfixes_selection_subscribers(
        &self,
        namespace: NamespaceId,
        selection: u32,
        subtype: u8,
    ) -> Result<Vec<(XServerFrontendClientId, XResourceId)>, XServerFrontendRouteError> {
        let wanted = 1u32 << subtype;
        Ok(self
            .xfixes_selection_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, window, candidate), (owner_namespace, mask))| {
                // Atoms are global, so the selection name alone does not say
                // whose selection changed. Delivering across namespaces would
                // disclose another namespace's owner window to a confined
                // client that only ever named an atom.
                (*candidate == selection
                    && *owner_namespace == namespace
                    && mask & wanted != 0)
                    .then_some((*client, *window))
            })
            .collect())
    }

    /// Retire every selection subscription naming `window`.
    ///
    /// A destroyed window cannot receive anything, and its id may be handed to
    /// the next client that asks for one.
    /// Retire every selection subscription belonging to `client`.
    ///
    /// A client id may be reissued, and an inherited subscription would
    /// deliver another client's selections to whoever takes the id next.
    /// End a watcher that has stopped draining its queue.
    ///
    /// Dropping its event instead would leave an admitted client believing it
    /// is still subscribed while the server quietly stopped telling it things.
    /// A client that cannot keep up has failed as an endpoint, which is what
    /// the input path already concludes for the same failure.
    ///
    /// BY THE IDENTITY THAT STALLED, NOT BY THE NUMBER. This runs after the
    /// route that failed has returned, and in between the stalled connection
    /// can have ended, given its number back and been succeeded. Both effects
    /// here compare the captured identity under their own acquisition, so a
    /// successor's entry and row are left alone; nothing here looks the number
    /// up afresh to decide whom to act on.
    fn disconnect_saturated_recipient(
        &self,
        stalled: XServerFrontendStalledRecipient,
    ) -> Result<(), XServerFrontendRouteError> {
        self.input_recovery.disconnect_exact(
            stalled.client,
            &stalled.occupant,
            XAuthorityInputDeliveryOutcome::ClientDisconnected,
            None,
        )?;
        self.remove_row_of(stalled.client, &stalled.occupant)?;
        Ok(())
    }

    fn remove_xfixes_selection_client(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.xfixes_selection_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .retain(|(candidate, _, _), _| *candidate != client);
        Ok(())
    }

    fn remove_xfixes_selection_window(
        &self,
        window: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.xfixes_selection_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .retain(|(_, candidate, _), _| *candidate != window);
        Ok(())
    }
}

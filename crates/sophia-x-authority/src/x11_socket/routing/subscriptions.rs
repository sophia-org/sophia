#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn select_core_events(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
        mask: u32,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut subscriptions = self
            .core_event_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let key = (client, window);
        if mask == 0 {
            subscriptions.remove(&key);
        } else {
            subscriptions.insert(key, mask);
        }
        Ok(())
    }

    /// The core event mask a client selected on a window, 0 for none.
    fn core_event_mask(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<u32, XServerFrontendRouteError> {
        Ok(self
            .core_event_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&(client, window))
            .copied()
            .unwrap_or(0))
    }

    fn remove_core_event_window(
        &self,
        window: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.core_event_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .retain(|(_, candidate), _| *candidate != window);
        Ok(())
    }

    fn property_change_subscribers(
        &self,
        window: XResourceId,
    ) -> Result<Vec<XServerFrontendClientId>, XServerFrontendRouteError> {
        const PROPERTY_CHANGE_MASK: u32 = 1 << 22;
        Ok(self
            .core_event_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, candidate), mask)| {
                (*candidate == window && *mask & PROPERTY_CHANGE_MASK != 0).then_some(*client)
            })
            .collect())
    }

    /// The keys down in the client's namespace, as QueryKeymap reports them:
    /// what a KeymapNotify to it carries.
    fn pressed_keys_of_client(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<[u8; 32], XServerFrontendRouteError> {
        // A client not registered into a namespace has no keyboard here:
        // nothing is down.
        let Some(namespace) = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&client)
            .and_then(|senders| senders.namespace)
        else {
            return Ok([0; 32]);
        };
        Ok(self
            .input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .pressed_keys(namespace))
    }

    fn core_event_subscribers(
        &self,
        window: XResourceId,
        required_mask: u32,
    ) -> Result<Vec<XServerFrontendClientId>, XServerFrontendRouteError> {
        Ok(self
            .core_event_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, candidate), mask)| {
                (*candidate == window && *mask & required_mask != 0).then_some(*client)
            })
            .collect())
    }

    /// The root's XID is shared, but its colormap belongs to one namespace.
    /// Drop the subscriptions lock before inspecting connection admissions.
    fn colormap_subscribers(
        &self,
        namespace: NamespaceId,
        window: XResourceId,
    ) -> Result<Vec<XServerFrontendClientId>, XServerFrontendRouteError> {
        let mut subscribers = self.core_event_subscribers(window, 1 << 23)?;
        let clients = self.clients.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        subscribers.retain(|client| clients.get(client).is_some_and(|senders| {
            senders.namespace == Some(namespace)
                || senders.connection_state.get().is_some_and(|state| state.namespace == namespace)
        }));
        Ok(subscribers)
    }
}

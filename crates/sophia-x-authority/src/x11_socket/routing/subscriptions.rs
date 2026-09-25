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
}

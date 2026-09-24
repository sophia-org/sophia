#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct XPresentMscDelivery {
    recipient: XServerFrontendClientId,
    event: XClientEvent,
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    /// Immediate answers belong to the request's serialized output. Publishing
    /// them asynchronously here can stamp the preceding request's sequence.
    fn prepare_present_msc_notify(
        &self,
        window: XResourceId,
        serial: u32,
        target_msc: u64,
    ) -> Result<Vec<XPresentMscDelivery>, XServerFrontendRouteError> {
        let clock = *self
            .present_clock
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let (ust, msc) = clock.unwrap_or((0, 0));
        if target_msc <= msc {
            return self.present_msc_deliveries(window, serial, ust, msc);
        }
        self.pending_msc_notifies
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .push((window, serial, target_msc));
        Ok(Vec::new())
    }

    fn present_msc_deliveries(
        &self,
        window: XResourceId,
        serial: u32,
        ust: u64,
        msc: u64,
    ) -> Result<Vec<XPresentMscDelivery>, XServerFrontendRouteError> {
        Ok(self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter(|(_, subscription)| {
                subscription.window == window && subscription.mask & (1 << 1) != 0
            })
            .map(|((recipient, _), subscription)| XPresentMscDelivery {
                recipient: *recipient,
                event: XClientEvent::PresentCompleteNotify {
                    sequence: 0,
                    event_id: subscription.event_id,
                    window,
                    serial,
                    ust,
                    msc,
                    kind: 1,
                    mode: 0,
                },
            })
            .collect())
    }

    fn route_present_msc_notify(
        &self,
        window: XResourceId,
        serial: u32,
        ust: u64,
        msc: u64,
    ) -> Result<(), XServerFrontendRouteError> {
        for delivery in self.present_msc_deliveries(window, serial, ust, msc)? {
            self.route_protocol_contained(delivery.recipient, delivery.event)?;
        }
        Ok(())
    }
}

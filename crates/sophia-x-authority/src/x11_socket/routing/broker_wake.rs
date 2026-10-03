// Queue notifications are separate from the routing decisions they wake.
#[cfg(unix)]
impl XServerFrontendRouteBroker {
    pub fn notifying_route_lease_release_sender(
        &self,
    ) -> sophia_wake::SignalSender<XAuthorityRouteLeaseRelease> {
        sophia_wake::SignalSender::new(
            self.route_lease_release_sender.clone(),
            self.service_wake.clone(),
        )
    }

    /// Enable prepared Present wire admission before this broker is handed
    /// to the frontend. The caller must drain present_clock_router admissions
    /// on every owner wake, including when no native outputs exist.
    pub fn with_present_clock_admission(mut self, owner_wake: sophia_wake::Notifier) -> Self {
        self.registry.present_clock_owner = Some(owner_wake);
        self
    }

    pub fn set_owner_wake(&self, wake: sophia_wake::Notifier) {
        if let Some(client) = &self.registry.explicit_pointer_grabs {
            client.set_owner_wake(wake.clone());
        }
        self.registry.input_recovery.owner_wake.set(wake.clone());
        self.registry.owner_wake.set(wake);
    }
}

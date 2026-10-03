// The hint may stay true after cancellation, but never false while timed
// service is owed. Producers publish under runtime before the obligation or
// its wake is exposed. Only the service clears it, under runtime -> feedback.
impl XAuthorityRuntime {
    pub(crate) fn present_service_demand(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.present_service_demand)
    }

    fn require_present_service(&self) {
        self.present_service_demand.store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn prepared_present_service_pending(&self) -> bool {
        let pending = !self.prepared_presents.is_empty() || !self.prepared_msc_notifies.is_empty();
        debug_assert!(pending || self.prepared_present_schedules.values().all(|s| s.queue.is_empty()),
            "a queued clock obligation must have a prepared owner");
        // Keep the queue check in release builds too: never sleep over debt.
        pending || self.prepared_present_schedules.values().any(|s| !s.queue.is_empty())
    }

    pub(crate) fn record_present_service_lock(&mut self, deadline: bool) {
        let counter = if deadline { &mut self.present_timing_statistics.deadline_runtime_locks }
            else { &mut self.present_timing_statistics.service_runtime_locks };
        *counter = counter.saturating_add(1);
    }
}

// Binding failures are request-local. Terminal feedback stays in the same
// bounded preparations and is cancelled by the same window/client lifecycle.
impl XAuthorityRuntime {
    /// Try the supplied observation, then retry a clock refusal once on Fake.
    /// A second refusal becomes terminal feedback, never an owner-loop error.
    /// `previous=None` retains the window's last accepted old-source anchor.
    pub fn resolve_present_clock_admission(
        &mut self, request: TransactionId, sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>, now_usec: impl FnOnce() -> u64,
    ) -> bool {
        let result = self.bind_present_clock_admission(request, sample, previous);
        let error = match result {
            Ok(result) => return result.is_some(),
            Err(error) => error,
        };
        self.record_present_admission_error(error);
        // The successful hot path does not sample a second host clock.
        let fake = crate::XPresentClockSample::background(now_usec());
        if matches!(error, XPreparedPresentScheduleError::Clock(_)) {
            self.present_timing_statistics.admission_fake_retries =
                self.present_timing_statistics.admission_fake_retries.saturating_add(1);
            match self.bind_present_clock_admission(request, fake, None) {
                Ok(result) => return result.is_some(),
                Err(error) => self.record_present_admission_error(error),
            }
        }
        self.settle_failed_present_admission(request, fake)
    }

    fn record_present_admission_error(&mut self, error: XPreparedPresentScheduleError) {
        self.present_timing_statistics.admission_errors =
            self.present_timing_statistics.admission_errors.saturating_add(1);
        if let XPreparedPresentScheduleError::Clock(error) = error {
            self.present_timing_statistics.reject_clock(error);
        }
        // The combined preparation cap and once-only binding under runtime
        // make queue capacity/duplicate refusals unreachable. Release builds
        // still settle a request if that invariant is broken.
        debug_assert!(!matches!(error, XPreparedPresentScheduleError::Queue(_)),
            "Present admission violated queue capacity or identity");
    }

    fn last_prepared_present_sample(&self, request: TransactionId, window: crate::XResourceId)
        -> Option<(u64, u64)>
    {
        let state = self.prepared_present_schedules.get(&window)?;
        state.queue.get(request).and_then(|queued|
            queued.binding.window_sample(state.queue.observation(request)?).ok())
    }

    fn fallback_prepared_present_sample(&self, request: TransactionId, window: crate::XResourceId)
        -> Option<(u64, u64)>
    {
        self.prepared_present_schedules.get(&window).and_then(|state|
            state.clock.binding().ok()?.window_sample(state.clock.sample().ok()?).ok())
            .or_else(|| self.prepared_presents.get(&request)?.anchor)
    }

    fn settle_failed_present_admission(&mut self, request: TransactionId,
        fake: crate::XPresentClockSample) -> bool
    {
        if let Some(notify) = self.prepared_msc_notifies.get(&request) {
            if notify.terminal.is_some() { return false; }
            let window = notify.window;
            let sample = self.last_prepared_present_sample(request, window)
                .or_else(|| self.fallback_prepared_present_sample(request, window))
                .unwrap_or((fake.ust, fake.msc));
            self.unschedule_prepared_present(window, request);
            let notify = self.prepared_msc_notifies.get_mut(&request).expect("request held");
            notify.timing = None;
            // Preserve the accepted window timeline when one exists. Raw
            // Fake is only for a clockless refusal, with no display claim.
            notify.terminal = Some(sample);
        } else if let Some(pixmap) = self.prepared_presents.get(&request) {
            if pixmap.terminal_fake_owed || pixmap.terminal.is_some() { return false; }
            let sample = self.last_prepared_present_sample(request, pixmap.request.window)
                .or_else(|| self.fallback_prepared_present_sample(request, pixmap.request.window))
                .unwrap_or((fake.ust, fake.msc));
            self.signal_failed_admission_idle(request);
            self.terminalize_prepared_pixmap(request, sample);
        } else {
            return false;
        }
        self.present_timing_statistics.admission_settled =
            self.present_timing_statistics.admission_settled.saturating_add(1);
        true
    }

    fn signal_failed_admission_idle(&mut self, request: TransactionId) {
        if self.signal_prepared_present_idle(request).is_err() {
            self.present_timing_statistics.idle_signal_failures =
                self.present_timing_statistics.idle_signal_failures.saturating_add(1);
        }
    }

    fn terminalize_prepared_pixmap(&mut self, request: TransactionId, sample: (u64, u64)) {
        let pixmap = self.prepared_presents.get(&request).expect("request held");
        let window = pixmap.request.window;
        if pixmap.backing_live { self.scrap_prepared_present(request); }
        self.unschedule_prepared_present(window, request);
        let pixmap = self.prepared_presents.get_mut(&request).expect("request held");
        pixmap.timing = None;
        pixmap.terminal = Some(sample);
    }

    /// Equal-target scrap and source loss already changed the queue. A failed
    /// idle signal must not leave that older pixmap pinned or strand later
    /// scraps. Settle only the failing pixmap at its own accepted sample.
    fn retire_scrapped_prepared_present(&mut self, request: TransactionId) {
        let Some(pixmap) = self.prepared_presents.get(&request) else { return; };
        if pixmap.terminal_fake_owed || pixmap.terminal.is_some() { return; }
        let window = pixmap.request.window;
        if self.signal_prepared_present_idle(request).is_err() {
            self.present_timing_statistics.idle_signal_failures =
                self.present_timing_statistics.idle_signal_failures.saturating_add(1);
            let accepted = self.last_prepared_present_sample(request, window);
            if accepted.is_none() {
                self.present_timing_statistics.scrap_sample_fallbacks =
                    self.present_timing_statistics.scrap_sample_fallbacks.saturating_add(1);
            }
            if let Some(sample) = accepted.or_else(|| self.fallback_prepared_present_sample(request, window)) {
                self.terminalize_prepared_pixmap(request, sample);
                self.present_timing_statistics.admission_settled =
                    self.present_timing_statistics.admission_settled.saturating_add(1);
            } else {
                // No saved pair exists. Release custody now, retaining the
                // truthful Idle event and a terminal Skip obligation. Service
                // supplies its current Fake pair without another clock read;
                // this error completion makes no physical-display claim.
                if self.prepared_presents[&request].backing_live { self.scrap_prepared_present(request); }
                self.unschedule_prepared_present(window, request);
                let pixmap = self.prepared_presents.get_mut(&request).expect("request held");
                pixmap.timing = None;
                pixmap.terminal_fake_owed = true;
            }
            // Assert after containing the broken invariant. Release builds
            // continue servicing other requests; debug builds expose the bug.
            debug_assert!(accepted.is_some(), "a queued scrap lost its accepted clock sample");
        } else {
            self.scrap_prepared_present(request);
        }
    }

    pub(crate) fn settle_prepared_present_fake_terminals(&mut self, now_usec: u64) {
        for pixmap in self.prepared_presents.values_mut().filter(|p| p.terminal_fake_owed) {
            let fake = crate::XPresentClockSample::background(now_usec);
            pixmap.terminal = Some((fake.ust, fake.msc));
            pixmap.terminal_fake_owed = false;
            self.present_timing_statistics.admission_settled =
                self.present_timing_statistics.admission_settled.saturating_add(1);
        }
    }
}

// Fault controls only; assertions and socket-level regressions live in tests/support.
#[cfg(test)]
impl XAuthorityRuntime {
    pub(crate) fn refuse_prepared_admission_for_test(&mut self, request: TransactionId,
        fake: crate::XPresentClockSample) -> bool
    {
        self.settle_failed_present_admission(request, fake)
    }

    pub(crate) fn scrap_with_missing_sample_for_test(&mut self, request: TransactionId,
        retain_window: bool, retain_anchor: bool)
    {
        let window = self.prepared_presents[&request].request.window;
        if retain_window {
            self.prepared_present_schedules.get_mut(&window).unwrap().queue.take(request);
        } else {
            self.prepared_present_schedules.remove(&window);
        }
        if !retain_anchor { self.prepared_presents.get_mut(&request).unwrap().anchor = None; }
        self.publish_present_clock_interest(window);
        self.retire_scrapped_prepared_present(request);
    }
}

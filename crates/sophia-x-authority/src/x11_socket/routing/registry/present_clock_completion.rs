const X_PRESENT_COMPLETION_OBSERVATIONS: usize = 4;

#[derive(Clone, Copy, Debug)]
struct XPresentCompletionState {
    binding: crate::XPresentClockBinding,
    // Immutable fallback if every retained observation is newer than
    // the retirement event. No future query may timestamp an old retirement.
    executed: crate::XPresentClockSample,
    // Newest first, fixed storage. Repeated observations must not evict the
    // pre-event sample when clock service precedes retirement in the pass.
    history: [crate::XPresentClockSample; X_PRESENT_COMPLETION_OBSERVATIONS],
    lost: bool,
    rejected: u8,
}

impl XPresentCompletionState {
    fn new(binding: crate::XPresentClockBinding, executed: crate::XPresentClockSample) -> Self {
        Self { binding, executed, history: [executed; X_PRESENT_COMPLETION_OBSERVATIONS], lost: false, rejected: 0 }
    }

    fn latest(&self) -> crate::XPresentClockSample {
        self.history[0]
    }

    fn remember(&mut self, sample: crate::XPresentClockSample) {
        if self.latest() != sample {
            self.history.rotate_right(1);
            self.history[0] = sample;
        }
    }

    fn at_or_before(&self, ust: u64) -> crate::XPresentClockSample {
        self.history.iter().find(|sample| sample.ust <= ust).copied().unwrap_or(self.executed)
    }
}

impl XServerFrontendRouteRegistry {
    /// The caller supplies retirement permission, in retirement delivery
    /// order. Use its chosen-head sample when available; a foreign retirement
    /// uses a retained bound-source observation no newer than that event, or
    /// the execution observation. Loss takes precedence over a late matching
    /// callback and uses the last accepted sample. Never
    /// wait for a future tick, reorder by the client's opaque serial, or clamp
    /// counters from independent frozen sources to a fictitious common MSC.
    ///
    /// Delivery is serialized through event enqueue. Sorting events across
    /// physical sources is the adapter's job: this registry cannot know about
    /// an earlier event still unread on another card fd.
    fn route_bound_present_complete(
        &self,
        transaction: TransactionId,
        sample: crate::XPresentClockSample,
        mode: XPresentCompletionMode,
        comparison: Option<crate::XPresentLayoutComparison>,
    ) -> Result<crate::XPresentCompleteRouteOutcome, XServerFrontendRouteError> {
        self.route_retired_present_complete(transaction, (sample.ust, None), [sample.into()].into_iter(), mode, comparison)
            .map(|(outcome, _)| outcome)
    }

    fn route_retired_present_complete(
        &self,
        transaction: TransactionId,
        retirement: (u64, Option<u64>),
        samples: impl Iterator<Item = crate::XPresentRetirementClock> + Clone,
        mode: XPresentCompletionMode,
        comparison: Option<crate::XPresentLayoutComparison>,
    ) -> Result<(crate::XPresentCompleteRouteOutcome, Option<(u64, u64)>), XServerFrontendRouteError> {
        let _delivery = self.pending_presentations.completion_delivery.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let (selected, reported) = {
            let pending = self.pending_presentations.entries.lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let no_delivery = crate::XPresentCompleteRouteOutcome {
                routed: false, mode, layout_comparison: None,
            };
            let Some(p) = pending.get(&transaction) else { return Ok((no_delivery, None)); };
            if p.phases.completed() { return Ok((no_delivery, None)); }
            if let Some(clock) = &p.clock {
                let selected = if matches!(clock.binding.source, crate::XPresentClockSource::Unclocked { .. }) {
                    // Permission and UST come from real retirement. The
                    // counter stays at its execution plateau, even if the
                    // source was lost meanwhile; no clock is revived.
                    crate::XPresentClockSample { ust: retirement.0, ..clock.executed }
                } else if clock.lost {
                    clock.latest()
                } else if let Some(sample) = samples.clone().find(|evidence| evidence.sample.source == clock.binding.source) {
                    sample.sample
                } else {
                    // No MSC is manufactured for an out-fence or a sibling
                    // whose event has not arrived. Permission never waits.
                    clock.at_or_before(retirement.0)
                };
                let reported = clock.binding.window_sample(selected)
                    .map_err(|_| XServerFrontendRouteError::PresentClockMismatch { transaction })?;
                (XPresentCompletionClock::Bound(selected), reported)
            } else {
                let msc = retirement.1.ok_or(XServerFrontendRouteError::PresentClockMismatch { transaction })?;
                (XPresentCompletionClock::Legacy { ust: retirement.0, msc }, (retirement.0, msc))
            }
        };
        self.route_present_complete_on_clock(transaction, selected, mode, comparison)
            .map(|outcome| { let clock = outcome.routed.then_some(reported); (outcome, clock) })
    }

    /// Observations/loss only update the fallback and demand lifetime. They
    /// never create Complete or Idle, including after an invalid sample.
    fn observe_completion_clock(
        &self,
        source: crate::XPresentClockSource,
        sample: Option<crate::XPresentClockSample>,
        mut runtime: Option<&mut XAuthorityRuntime>,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut pending = self.pending_presentations.entries.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        for p in pending.values_mut() {
            if p.phases.completed() { continue; }
            let Some(clock) = p.clock.as_mut().filter(|c| c.binding.source == source && !c.lost) else { continue; };
            match sample {
                Some(sample) if sample.source == source
                    && sample.ust >= clock.latest().ust
                    && sample.msc.wrapping_sub(clock.latest().msc) < (1 << 63) => {
                    clock.remember(sample);
                    clock.rejected = 0;
                }
                Some(_) => {
                    let _ = self.pending_presentations.rejected_clock_samples.fetch_update(
                        Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_add(1)));
                    clock.rejected = clock.rejected.saturating_add(1).min(crate::runtime::X_PRESENT_CLOCK_REJECTION_LIMIT);
                    clock.lost = clock.rejected == crate::runtime::X_PRESENT_CLOCK_REJECTION_LIMIT;
                }
                None => { clock.lost = true; }
            }
            if let Some(runtime) = runtime.as_deref_mut() {
                runtime.observe_executed_present_clock(p.window, clock.binding, clock.latest());
            }
        }
        Ok(())
    }
}

impl XServerFrontendRouteRegistry {
    /// Native-only adapter. No caller holds runtime. Publish a real event's
    /// progress before Complete can remove the final obligation. Lock order
    /// remains runtime -> feedback -> index; historical/lost events are inert.
    fn retain_retirement_clock(&self, transaction: TransactionId,
        samples: impl Iterator<Item = crate::XPresentRetirementClock> + Clone) -> Result<(), XServerFrontendRouteError> {
        if samples.clone().next().is_none() { return Ok(()); }
        let stats = &self.pending_presentations;
        let needs_runtime = {
            let mut pending = stats.entries.lock().map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some(p) = pending.get_mut(&transaction).filter(|p| !p.phases.completed()) else { return Ok(()); };
            let Some(clock) = p.clock.as_mut().filter(|clock| !clock.lost) else { return Ok(()); };
            let Some(sample) = samples.clone().find(|evidence| evidence.sample.source == clock.binding.source) else { return Ok(()); };
            if sample.historical {
                let _ = stats.completion_historical_samples.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_add(1)));
                return Ok(());
            }
            let sample = sample.sample;
            let needed = if let Some(index) = self.present_clock_interests.get() {
                index.lock().map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .get(&p.window).is_some_and(|interest| interest.current == Some(clock.binding)
                        || interest.queued.contains(&sample.source))
            } else { true };
            if !needed { clock.remember_retirement(sample); }
            needed
        };
        if !needs_runtime { return Ok(()); }
        let runtime = self.runtime.get().and_then(std::sync::Weak::upgrade);
        let mut runtime = runtime.as_ref().map(|runtime| runtime.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)).transpose()?;
        if runtime.is_some() {
            let _ = stats.completion_runtime_locks.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_add(1)));
        }
        let mut pending = stats.entries.lock().map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let Some(p) = pending.get_mut(&transaction).filter(|p| !p.phases.completed()) else { return Ok(()); };
        let Some(clock) = p.clock.as_mut().filter(|clock| !clock.lost) else { return Ok(()); };
        let Some(sample) = samples.clone().find(|evidence| evidence.sample.source == clock.binding.source) else { return Ok(()); };
        if sample.historical { return Ok(()); }
        let sample = sample.sample;
        clock.remember_retirement(sample);
        if let Some(runtime) = runtime.as_deref_mut() {
            let (ready, historical) = runtime.observe_retirement_present_clock(p.window, clock.binding, sample);
            if historical {
                let _ = stats.completion_historical_samples.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_add(1)));
            }
            if ready { self.service_wake.notify(); }
        }
        Ok(())
    }
}

impl XPresentCompletionState {
    fn remember_retirement(&mut self, sample: crate::XPresentClockSample) {
        if !self.lost && sample.ust > self.latest().ust
            && sample.msc.wrapping_sub(self.latest().msc) < (1 << 63) {
            self.remember(sample);
            self.rejected = 0;
        }
    }
}

#[cfg(test)]
impl XServerFrontendRouteRegistry {
    pub(crate) fn completion_clock_snapshot(&self, transaction: TransactionId)
        -> ([crate::XPresentClockSample; X_PRESENT_COMPLETION_OBSERVATIONS], u8, bool) {
        let pending = self.pending_presentations.entries.lock().unwrap();
        let clock = pending.get(&transaction).unwrap().clock.unwrap();
        (clock.history, clock.rejected, clock.lost)
    }
}

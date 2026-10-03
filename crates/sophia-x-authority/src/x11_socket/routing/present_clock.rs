/// Clock observations only. This handle cannot execute a request or publish
/// scene content. Physical source identity and its reset incarnation are
/// supplied by the backend; X window identities stay within the frontend.
#[derive(Clone)]
pub struct XServerFrontendPresentClockRouter {
    registry: XServerFrontendRouteRegistry,
}

impl XServerFrontendPresentClockRouter {
    /// Pending source choices only. Session selects from its current sampling
    /// table, outside runtime, then binds the still-live request once.
    pub fn admissions(&self) -> Result<Vec<crate::XPresentClockAdmission>, X11SetupSocketError> {
        let Some(runtime) = self.registry.runtime.get().and_then(std::sync::Weak::upgrade) else {
            return Ok(Vec::new());
        };
        Ok(runtime.lock().map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))?
            .present_clock_admissions())
    }

    pub fn bind_admission(&self, request: TransactionId, sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>)
        -> Result<bool, X11SetupSocketError>
    {
        self.bind_admission_with_time(request, sample, previous, || {
            let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
            (time.tv_sec as u64).saturating_mul(1_000_000).saturating_add(time.tv_nsec as u64 / 1_000)
        })
    }

    fn bind_admission_with_time(&self, request: TransactionId, sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>, now_usec: impl FnOnce() -> u64)
        -> Result<bool, X11SetupSocketError>
    {
        let runtime = self.registry.runtime.get().and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| X11SetupSocketError::new("Present clock authority unavailable"))?;
        let mut runtime = runtime.lock().map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))?;
        let bound = if matches!(sample.source, crate::XPresentClockSource::Unclocked { .. }) {
            // The bounded NotifyMSC wait starts when binding actually occurs,
            // not at an earlier Session query that may have waited on runtime.
            let now = now_usec();
            runtime.resolve_present_clock_admission(request,
                crate::XPresentClockSample { ust: now, msc: 0, ..sample }, previous, || now)
        } else {
            runtime.resolve_present_clock_admission(request, sample, previous, now_usec)
        };
        // The private service delivers immediate scrap Idle and arms the
        // deadline under the same runtime->feedback ordering as execution.
        if bound { self.registry.service_wake.notify(); }
        Ok(bound)
    }

    /// Request-local fallback counters retained for existing diagnostic callers.
    pub fn admission_counts(&self) -> Result<(u64, u64, u64, u64, u64), X11SetupSocketError> {
        let stats = self.wire_timing_statistics()?;
        Ok((stats.admission_errors, stats.admission_fake_retries, stats.admission_settled,
            stats.idle_signal_failures, stats.scrap_sample_fallbacks))
    }

    /// Cumulative timing and admission measurements, sampled only on diagnostic cadence.
    pub fn wire_timing_statistics(&self) -> Result<crate::XPresentTimingStatistics, X11SetupSocketError> {
        let Some(runtime) = self.registry.runtime.get().and_then(std::sync::Weak::upgrade) else {
            return Ok(crate::XPresentTimingStatistics::default());
        };
        Ok(runtime.lock().map_err(|_| X11SetupSocketError::new("Present wire statistics runtime poisoned"))?
            .present_timing_statistics())
    }

    /// Queued requests need progress. Executed requests retain their binding
    /// for loss and completion mapping, but no longer need periodic queries:
    /// the retirement supplies its own timestamp/permission.
    pub fn query_demands(&self) -> Result<Vec<(crate::XPresentClockSource, u64)>, X11SetupSocketError> {
        // The first client setup binds runtime. An empty Session can service
        // clocks before that happens; it has no queued timing obligations.
        let Some(runtime) = self.registry.runtime.get().and_then(std::sync::Weak::upgrade) else {
            return Ok(Vec::new());
        };
        Ok(runtime.lock().map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))?
            .prepared_present_clock_demands())
    }

    pub fn completed_count(&self) -> u64 {
        self.registry.pending_presentations.completed.load(Ordering::Relaxed)
    }

    /// Source deliveries from the Session bridge and the runtime locks they
    /// acquire. Excludes demand/loss calls and the frontend's already-locked
    /// fake-clock turn. Use interval deltas, not lifetime ratios.
    pub fn observation_counts(&self) -> (u64, u64) {
        (self.registry.pending_presentations.source_observations.load(Ordering::Relaxed),
         self.registry.pending_presentations.observation_runtime_locks.load(Ordering::Relaxed))
    }

    /// Native Complete anchoring only; separate from source delivery locks.
    pub fn completion_observation_counts(&self) -> (u64, u64) {
        (self.registry.pending_presentations.completion_runtime_locks.load(Ordering::Relaxed),
         self.registry.pending_presentations.completion_historical_samples.load(Ordering::Relaxed))
    }

    /// Includes old heads and fake clocks still owed by queued requests or
    /// executed requests whose Complete has not arrived. Idle alone does not
    /// release a clock obligation; Complete alone does release the clock but
    /// leaves the independent buffer obligation in the feedback registry.
    /// A window moving changes only the clock chosen by its next request.
    pub fn bound_sources(&self) -> Result<Vec<crate::XPresentClockSource>, X11SetupSocketError> {
        let mut sources = BTreeSet::new();
        if let Some(runtime) = self.registry.runtime.get().and_then(std::sync::Weak::upgrade) {
            sources.extend(runtime.lock().map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))?
                .prepared_present_clock_bindings());
        }
        sources.extend(self.completion_bindings()?.into_iter().map(|(_, binding)| binding.source));
        Ok(sources.into_iter().collect())
    }

    /// Opaque request identities only; window and pixmap names stay in X.
    pub fn completion_bindings(&self) -> Result<Vec<(TransactionId, crate::XPresentClockBinding)>, X11SetupSocketError> {
        Ok(self.registry.pending_presentations.entries.lock()
            .map_err(|_| X11SetupSocketError::new("Present clock feedback registry poisoned"))?
            .iter().filter(|(_, p)| !p.phases.completed())
            .filter_map(|(transaction, p)| p.clock.as_ref().filter(|clock| !clock.lost).map(|clock| (*transaction, clock.binding)))
            .collect())
    }

    pub fn observe_source(&self, sample: crate::XPresentClockSample) -> Result<(), X11SetupSocketError> {
        let stats = &self.registry.pending_presentations;
        let _ = stats.source_observations.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |n| Some(n.saturating_add(1)));
        if let Some(index) = self.registry.present_clock_interests.get() {
            let needs_runtime = {
                let pending = stats.entries.lock()
                    .map_err(|_| X11SetupSocketError::new("Present clock feedback registry poisoned"))?;
                let interests = index.lock().map_err(|_| X11SetupSocketError::new("Present clock interest index poisoned"))?;
                interests.values().any(|interest| interest.queued.contains(&sample.source))
                    || pending.values().any(|p| !p.phases.completed() && p.clock.as_ref().is_some_and(|clock|
                        !clock.lost && clock.binding.source == sample.source
                        && interests.get(&p.window).is_some_and(|interest| interest.current == Some(clock.binding))))
            };
            if !needs_runtime {
                // Never nest index -> feedback: execution publishes index
                // updates while it owns feedback. A concurrent admission
                // after this read starts from its supplied sample; its next
                // observation sees the published interest.
                self.registry.observe_completion_clock(sample.source, Some(sample), None)
                    .map_err(|_| X11SetupSocketError::new("Present completion clock routing failed"))?;
                return Ok(());
            }
        }
        // Keep runtime -> feedback lock order, including the persisted window
        // anchor. A new request must not rebind between these two updates.
        let runtime = self.registry.runtime.get().and_then(std::sync::Weak::upgrade);
        let mut runtime = runtime.as_ref().map(|runtime| runtime.lock()
            .map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))).transpose()?;
        if runtime.is_some() {
            let _ = stats.observation_runtime_locks.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
                |n| Some(n.saturating_add(1)));
        }
        let ready = if let Some(runtime) = runtime.as_deref_mut() {
            runtime.observe_prepared_source_clock(sample).map_err(X11SetupSocketError::new)?
        } else { false };
        self.registry.observe_completion_clock(sample.source, Some(sample), runtime.as_deref_mut())
            .map_err(|_| X11SetupSocketError::new("Present completion clock routing failed"))?;
        if ready { self.registry.service_wake.notify(); }
        Ok(())
    }

    /// Explicitly ends obligations on a disabled/lost counter. Queued
    /// pixmaps settle Skip; no new counter incarnation can revive them.
    pub fn lose_source(&self, source: crate::XPresentClockSource) -> Result<(), X11SetupSocketError> {
        let runtime = self.registry.runtime.get().and_then(std::sync::Weak::upgrade);
        let mut runtime = runtime.as_ref().map(|runtime| runtime.lock()
            .map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))).transpose()?;
        if let Some(runtime) = runtime.as_deref_mut() {
            runtime.lose_prepared_present_clock(source).map_err(X11SetupSocketError::new)?;
        }
        self.registry.observe_completion_clock(source, None, runtime.as_deref_mut())
            .map_err(|_| X11SetupSocketError::new("Present lost completion clock routing failed"))?;
        self.registry.service_wake.notify();
        Ok(())

    }
    pub fn demands(&self) -> Result<Vec<SurfaceId>, X11SetupSocketError> {
        let runtime = self.registry.runtime.get().and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| X11SetupSocketError::new("Present clock authority unavailable"))?;
        Ok(runtime.lock().map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))?
            .prepared_present_clock_surfaces())
    }

    pub fn observe(
        &self, surface: SurfaceId, sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>,
    ) -> Result<(), X11SetupSocketError> {
        let runtime = self.registry.runtime.get().and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| X11SetupSocketError::new("Present clock authority unavailable"))?;
        let mut runtime = runtime.lock().map_err(|_| X11SetupSocketError::new("Present clock runtime poisoned"))?;
        let ready = runtime.observe_prepared_surface_clock(surface, sample, previous).map_err(X11SetupSocketError::new)?;
        for sample in previous.into_iter().chain(std::iter::once(sample)) {
            self.registry.observe_completion_clock(sample.source, Some(sample), Some(&mut runtime))
                .map_err(|_| X11SetupSocketError::new("Present completion clock routing failed"))?;
        }
        if ready { self.registry.service_wake.notify(); }
        Ok(())
    }
}

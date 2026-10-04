/// Window clocks survive an empty queue, but never window destruction. No
/// host clock or hardware prediction enters this state: its owner supplies
/// observations, and a fake-clock obligation returns an absolute deadline.
#[derive(Debug)]
struct XPreparedWindowSchedule {
    clock: crate::XPresentWindowClock,
    queue: crate::XPresentWindowSchedule,
    rejections: BTreeMap<crate::XPresentClockSource, u8>,
}

impl Default for XPreparedWindowSchedule {
    fn default() -> Self {
        Self {
            clock: Default::default(),
            queue: crate::XPresentWindowSchedule::new(
                std::num::NonZeroUsize::new(X_PREPARED_PRESENT_CAPACITY).expect("nonzero bound")),
            rejections: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPreparedPresentScheduleError {
    MissingPreparation,
    Clock(crate::XPresentTimingError),
    Queue(crate::XPresentScheduleError),
}

/// Cumulative bounded counters, independent of window lifetime. Invalid
/// observations are refused per window, never returned as a server failure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XPresentTimingStatistics {
    pub damage: XPresentDamageStatistics,
    pub clock_stale_samples: u64,
    pub clock_wrong_sources: u64,
    pub clock_counter_exhausted: u64,
    pub clock_invalid_samples: u64,
    pub clock_sources_lost: u64,
    /// Exact descriptor checks. A blocked request backs off to 32 ms, which
    /// can add that much signal-detection latency without a trigger wake.
    pub fence_queries: u64,
    /// Runtime acquisitions by timed service and its deadline calculation.
    pub service_runtime_locks: u64,
    pub deadline_runtime_locks: u64,
    pub fence_blocked: u64,
    pub admission_errors: u64,
    pub admission_fake_retries: u64,
    pub admission_settled: u64,
    pub idle_signal_failures: u64,
    pub scrap_sample_fallbacks: u64,
    pub wire_prepared: u64,
    pub wire_published: u64,
    /// Notification attempts, not owner passes or eventfd wakeups.
    pub wire_owner_notifications: u64,
    pub wire_bound: u64,
    pub wire_hardware_bound: u64,
    pub unclocked_bound: u64,
    pub unclocked_notify_settled: u64,
    pub wire_executions: u64,
    /// Preparation-to-execution delay, including target/fence/publication waits.
    /// No per-frame record; cumulative duration and maximum only.
    pub wire_execution_wait_usec: u64,
    pub wire_execution_wait_max_usec: u64,
}

pub(crate) const X_PRESENT_CLOCK_REJECTION_LIMIT: u8 = 3;

/// Read-side interest only, published under runtime before an admission or
/// rebind becomes visible. The small index mutex provides release/acquire
/// ordering; nested order is runtime -> feedback -> index. Readers release
/// feedback and index before waiting on runtime.
/// A dependency published after the read uses its admission sample, then the
/// next observation. No predicted clock value is stored here.
pub(crate) type XPresentClockInterests = Arc<Mutex<BTreeMap<crate::XResourceId, XPresentClockInterest>>>;

#[derive(Debug)]
pub(crate) struct XPresentClockInterest {
    pub(crate) current: Option<crate::XPresentClockBinding>,
    pub(crate) queued: BTreeSet<crate::XPresentClockSource>,
}

impl XPreparedWindowSchedule {
    fn observe_clock(&mut self, sample: crate::XPresentClockSample, previous: Option<crate::XPresentClockSample>)
        -> Result<(), (crate::XPresentClockSource, crate::XPresentTimingError)>
    {
        let source = self.clock.binding().map_or(sample.source, |binding| binding.source);
        let mut clock = self.clock.clone();
        let previous = previous.filter(|sample| !self.rejects_source(sample.source));
        clock.observe(sample, previous).map_err(|error| (source, error))?;
        self.queue.observe_pair(previous, sample)?;
        self.clock = clock;
        Ok(())
    }
    fn rejects_source(&self, source: crate::XPresentClockSource) -> bool {
        self.rejections.get(&source).is_some_and(|n| *n == X_PRESENT_CLOCK_REJECTION_LIMIT)
    }

    fn rejects_observation(&mut self, source: crate::XPresentClockSource,
        error: crate::XPresentTimingError, statistics: &mut XPresentTimingStatistics)
        -> Vec<crate::XPresentScheduledRequest>
    {
        statistics.reject_clock(error);
        let count = self.rejections.entry(source).or_default();
        *count = count.saturating_add(1).min(X_PRESENT_CLOCK_REJECTION_LIMIT);
        if *count < X_PRESENT_CLOCK_REJECTION_LIMIT { return Vec::new(); }
        statistics.clock_sources_lost = statistics.clock_sources_lost.saturating_add(1);
        self.queue.lose_source(source)
    }
}

impl XPresentTimingStatistics {
    fn record_wire_execution_wait(&mut self, elapsed: std::time::Duration) {
        let usec = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.wire_executions = self.wire_executions.saturating_add(1);
        self.wire_execution_wait_usec = self.wire_execution_wait_usec.saturating_add(usec);
        self.wire_execution_wait_max_usec = self.wire_execution_wait_max_usec.max(usec);
    }

    fn reject_clock(&mut self, error: crate::XPresentTimingError) {
        use crate::XPresentTimingError::*;
        let count = match error {
            StaleSample => &mut self.clock_stale_samples,
            WrongSource => &mut self.clock_wrong_sources,
            CounterExhausted => &mut self.clock_counter_exhausted,
            MissingClock | InvalidRemainder => &mut self.clock_invalid_samples,
        };
        *count = count.saturating_add(1);
    }
}

impl XAuthorityRuntime {
    pub(crate) fn present_clock_interests(&self) -> XPresentClockInterests {
        self.present_clock_interests.clone()
    }

    fn publish_present_clock_interest(&self, window: crate::XResourceId) {
        let mut index = self.present_clock_interests.lock().expect("Present clock interest index poisoned");
        if let Some(state) = self.prepared_present_schedules.get(&window) {
            index.insert(window, XPresentClockInterest {
                current: state.clock.binding().ok().filter(|binding| !state.rejects_source(binding.source)),
                queued: state.queue.clock_sources().collect(),
            });
        } else {
            index.remove(&window);
        }
    }

    pub fn present_timing_statistics(&self) -> XPresentTimingStatistics {
        self.present_timing_statistics
    }
    /// Timing follows preparation under the same runtime lock. Return the
    /// equal-target older preparations to the caller for immediate Idle
    /// delivery. Their pixmap/fence references are already released; each
    /// keeps a bounded completion record until its target. This lock hold
    /// also prevents DestroyFence from racing the exact private signal.
    pub fn schedule_prepared_present(
        &mut self,
        preparation: TransactionId,
        timing: crate::XPresentMscTiming,
        sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>,
    ) -> Result<Vec<TransactionId>, XPreparedPresentScheduleError> {
        let request = &self.prepared_presents.get(&preparation)
            .ok_or(XPreparedPresentScheduleError::MissingPreparation)?.request;
        let window = request.window;
        let replaces_contents = !request.has_update_region;
        let superseded = self.schedule_present_clock_request(window, preparation,
            crate::XPresentScheduledKind::Pixmap, replaces_contents, timing, sample, previous)?;
        let anchor = self.last_prepared_present_sample(preparation, window);
        let prepared = self.prepared_presents.get_mut(&preparation).expect("preparation held");
        prepared.timing = None;
        prepared.anchor = anchor;
        let superseded = superseded.into_iter().map(|p| p.request).collect::<Vec<_>>();
        for &preparation in &superseded {
            self.retire_scrapped_prepared_present(preparation);
        }
        Ok(superseded)
    }

    fn schedule_present_clock_request(
        &mut self, window: crate::XResourceId, preparation: TransactionId,
        kind: crate::XPresentScheduledKind, replaces_contents: bool,
        timing: crate::XPresentMscTiming, sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>,
    ) -> Result<Vec<crate::XPresentScheduledRequest>, XPreparedPresentScheduleError> {
        let state = self.prepared_present_schedules.entry(window).or_default();
        if state.rejects_source(sample.source) {
            return Err(XPreparedPresentScheduleError::Clock(crate::XPresentTimingError::WrongSource));
        }
        let mut clock = state.clock.clone();
        let previous = previous.filter(|sample| !state.rejects_source(sample.source));
        clock.observe(sample, previous).map_err(XPreparedPresentScheduleError::Clock)?;
        let target = clock.target(timing).map_err(XPreparedPresentScheduleError::Clock)?;
        state.queue.observe(sample).map_err(XPreparedPresentScheduleError::Clock)?;
        let superseded = state.queue.insert(crate::XPresentScheduledRequest {
            request: preparation, kind, target, replaces_contents,
            binding: clock.binding().map_err(XPreparedPresentScheduleError::Clock)?,
        }).map_err(XPreparedPresentScheduleError::Queue)?;
        state.clock = clock;
        if matches!(sample.source, crate::XPresentClockSource::Unclocked { .. }) {
            let count = &mut self.present_timing_statistics.unclocked_bound;
            *count = count.saturating_add(1);
        }
        state.rejections.retain(|source, _| *source == sample.source
            || state.queue.clock_sources().any(|bound| bound == *source));
        self.publish_present_clock_interest(window);
        Ok(superseded)
    }

    /// Hardware progress is supplied by a real query/event. The frontend
    /// service must be notified after publishing this observation.
    pub fn observe_prepared_present_clock(
        &mut self,
        window: crate::XResourceId,
        sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>,
    ) -> Result<(), crate::XPresentTimingError> {
        if let Some(state) = self.prepared_present_schedules.get_mut(&window) {
            state.observe_clock(sample, previous).map_err(|(_, error)| error)?;
        }
        self.publish_present_clock_interest(window);
        Ok(())
    }

    pub fn advance_prepared_present_fake_clocks(&mut self, now_usec: u64) -> Result<(), &'static str> {
        self.observe_prepared_source_clock(crate::XPresentClockSample::background(now_usec))?;
        // The ordinary clocked desktop does no additional queue scan.
        if self.present_timing_statistics.unclocked_bound == 0 { return Ok(()); }
        // Only queued obligations need a timer. An executed unclocked
        // pixmap completes from actual retirement and creates no idle work.
        let unclocked = self.prepared_present_schedules.values()
            .flat_map(|state| state.queue.clock_sources())
            .filter(|source| matches!(source, crate::XPresentClockSource::Unclocked { .. }))
            .collect::<BTreeSet<_>>();
        for source in unclocked {
            self.observe_prepared_source_clock(crate::XPresentClockSample { source, ust: now_usec, msc: 0 })?;
        }
        Ok(())
    }

    pub(crate) fn record_unclocked_notify_settled(&mut self, request: TransactionId) {
        if self.prepared_msc_notifies.get(&request).and_then(|notify|
            self.prepared_present_schedules.get(&notify.window))
            .and_then(|state| state.queue.get(request))
            .is_some_and(|request| matches!(request.binding.source, crate::XPresentClockSource::Unclocked { .. }))
        {
            let count = &mut self.present_timing_statistics.unclocked_notify_settled;
            *count = count.saturating_add(1);
        }
    }

    /// Observing an old source never changes a window's currently selected
    /// clock. Its still-queued requests keep their original source/offset.
    pub(crate) fn observe_prepared_source_clock(&mut self, sample: crate::XPresentClockSample)
        -> Result<bool, &'static str>
    {
        let mut changed = false;
        let mut idle = Vec::new();
        let mut lost_windows = Vec::new();
        for (&window, state) in &mut self.prepared_present_schedules {
            if state.queue.is_empty() { continue; }
            if state.rejects_source(sample.source) { continue; }
            if state.clock.sample().is_ok_and(|current| current.source != sample.source)
                && !state.queue.clock_sources().any(|source| source == sample.source) { continue; }
            let before = state.queue.ready().count();
            let mut clock = state.clock.clone();
            let observation = (|| {
                if clock.binding()?.source == sample.source { clock.observe(sample, None)?; }
                state.queue.observe(sample)
            })();
            if let Err(error) = observation {
                idle.extend(state.rejects_observation(sample.source, error, &mut self.present_timing_statistics));
                changed |= state.rejects_source(sample.source) || before != state.queue.ready().count();
                if state.rejects_source(sample.source) { lost_windows.push(window); }
                continue;
            }
            state.rejections.remove(&sample.source);
            state.clock = clock;
            changed |= before != state.queue.ready().count();
        }
        for window in lost_windows { self.publish_present_clock_interest(window); }
        self.scrap_clock_lost_preparations(idle)?;
        Ok(changed)
    }

    pub fn prepared_present_deadline_usec(&self) -> Option<u64> {
        self.prepared_present_schedules.values()
            .filter_map(|s| s.queue.background_deadline()).min()
    }

    pub(crate) fn prepared_present_service_deadline_usec(&self, now_usec: u64, execution_available: bool) -> Option<u64> {
        let clock = self.prepared_present_schedules.values()
            .filter_map(|s| s.queue.future_background_deadline()).min();
        let fence = execution_available.then(|| self.ready_prepared_presents()).into_iter().flatten().filter_map(|id| {
            let p = self.prepared_presents.get(&id)?;
            Some(if p.fences.wait.is_some() { p.fence_retry.next_usec.max(now_usec) } else { now_usec })
        }).min();
        clock.into_iter().chain(fence).min()
    }

    /// Each window retains target order; windows with older ready requests
    /// are visited first. Peeking does not consume a fence-blocked request.
    pub fn ready_prepared_presents(&self) -> Vec<TransactionId> {
        let mut windows = self.prepared_present_schedules.values()
            .map(|s| s.queue.ready().filter(|p| p.kind == crate::XPresentScheduledKind::Pixmap).map(|p| p.request).collect::<Vec<_>>())
            .filter(|requests| !requests.is_empty()).collect::<Vec<_>>();
        windows.sort_by_key(|requests| requests[0].raw());
        windows.into_iter().flatten().collect()
    }

    pub(crate) fn prepared_present_is_ready(&self, preparation: TransactionId) -> bool {
        let Some(request) = self.prepared_presents.get(&preparation) else { return false; };
        self.prepared_present_schedules.get(&request.request.window).is_some_and(|state|
            state.queue.ready().any(|p|
                p.request == preparation && p.kind == crate::XPresentScheduledKind::Pixmap))
    }

    pub(crate) fn prepared_present_msc_serviced(&mut self, preparation: TransactionId) {
        let Some(p) = self.prepared_presents.get(&preparation) else { return; };
        if let Some(state) = self.prepared_present_schedules.get_mut(&p.request.window) {
            state.queue.begin_execution(preparation);
        }
    }

    pub(crate) fn prepared_present_idle_deliveries(&self) -> Vec<TransactionId> {
        self.prepared_presents.iter().filter_map(|(id, p)| p.idle_owed.then_some(*id)).collect()
    }

    pub(crate) fn prepared_present_idle_delivered(&mut self, preparation: TransactionId) {
        if let Some(prepared) = self.prepared_presents.get_mut(&preparation) {
            prepared.idle_owed = false;
        }
    }

    pub(crate) fn ready_prepared_skips(&self) -> Vec<(TransactionId, u64, u64)> {
        self.prepared_present_schedules.values().flat_map(|state| {
            state.queue.ready().filter(|p| p.kind == crate::XPresentScheduledKind::Skip)
                .filter_map(|p| state.queue.completion_sample(p.request).map(|(ust, msc)| (p.request, ust, msc)))
        }).chain(self.prepared_presents.iter().filter_map(|(&id, p)|
            p.terminal.map(|(ust, msc)| (id, ust, msc)))).collect()
    }

    pub(crate) fn prepared_present_clock_bindings(&self) -> Vec<crate::XPresentClockSource> {
        self.prepared_present_schedules.values().flat_map(|s| s.queue.clock_sources())
            .collect::<BTreeSet<_>>().into_iter().collect()
    }

    pub(crate) fn prepared_present_clock_demands(&self) -> Vec<(crate::XPresentClockSource, u64)> {
        let mut demands = BTreeMap::<_, u64>::new();
        for (source, fields) in self.prepared_present_schedules.values().flat_map(|s| s.queue.clock_demands()) {
            demands.entry(source).and_modify(|n| *n = (*n).min(fields)).or_insert(fields);
        }
        demands.into_iter().collect()
    }

    pub(crate) fn lose_prepared_present_clock(&mut self, source: crate::XPresentClockSource)
        -> Result<(), &'static str>
    {
        let idle = self.prepared_present_schedules.values_mut()
            .flat_map(|s| {
                if s.clock.binding().is_ok_and(|binding| binding.source == source)
                    || s.queue.clock_sources().any(|bound| bound == source)
                {
                    s.rejections.insert(source, X_PRESENT_CLOCK_REJECTION_LIMIT);
                }
                s.queue.lose_source(source)
            }).collect::<Vec<_>>();
        for &window in self.prepared_present_schedules.keys() {
            self.publish_present_clock_interest(window);
        }
        self.scrap_clock_lost_preparations(idle)
    }

    /// Executed requests still observe their frozen source after the window's
    /// preparation queue empties. Preserve that accepted sample as the next
    /// rebind's continuity anchor. The caller holds runtime before feedback;
    /// neither a concurrent rebind nor destruction can split the update.
    /// An old request must not alter a window that has already rebound, even
    /// if the new binding later uses the same source with a different offset.
    pub(crate) fn observe_executed_present_clock(
        &mut self, window: crate::XResourceId, binding: crate::XPresentClockBinding,
        sample: crate::XPresentClockSample,
    ) {
        let Some(state) = self.prepared_present_schedules.get_mut(&window) else { return; };
        if state.clock.binding() != Ok(binding) { return; }
        if let Err(error) = state.clock.observe(sample, None) {
            // A newer queued observation may already be the anchor. Keep it;
            // an older accepted feedback sample must never rewind the window.
            if error != crate::XPresentTimingError::StaleSample {
                self.present_timing_statistics.reject_clock(error);
            }
        }
    }

    /// An event may be processed after a newer query. Such historical
    /// evidence is useful for Complete but cannot rewind or reject a clock.
    /// Apply progress to this window's queue and anchor together.
    pub(crate) fn observe_retirement_present_clock(
        &mut self, window: crate::XResourceId, binding: crate::XPresentClockBinding,
        sample: crate::XPresentClockSample,
    ) -> (bool, bool) {
        let Some(state) = self.prepared_present_schedules.get_mut(&window) else { return (false, false); };
        if state.rejects_source(binding.source) { return (false, false); }
        let current = state.clock.binding() == Ok(binding);
        if !current && !state.queue.clock_sources().any(|source| source == sample.source) {
            return (false, false);
        }
        if current && state.clock.sample().is_ok_and(|anchor| sample.ust <= anchor.ust
            || sample.msc.wrapping_sub(anchor.msc) >= (1 << 63)) {
            return (false, true);
        }
        let before = state.queue.ready().count();
        if current {
            if state.observe_clock(sample, None).is_err() { return (false, true); }
        } else if state.queue.observe(sample).is_err() {
            return (false, true);
        }
        state.rejections.remove(&sample.source);
        (before != state.queue.ready().count(), false)
    }

    fn scrap_clock_lost_preparations(&mut self, idle: Vec<crate::XPresentScheduledRequest>)
        -> Result<(), &'static str>
    {
        for request in idle {
            let preparation = request.request;
            self.retire_scrapped_prepared_present(preparation);
        }
        Ok(())
    }

    fn unschedule_prepared_present(&mut self, window: crate::XResourceId, preparation: TransactionId) {
        if let Some(state) = self.prepared_present_schedules.get_mut(&window) {
            state.queue.take(preparation);
        }
        self.publish_present_clock_interest(window);
    }

    pub(crate) fn prepared_present_clock_surfaces(&self) -> Vec<sophia_protocol::SurfaceId> {
        self.prepared_present_schedules.iter().filter(|(_, s)| !s.queue.is_empty())
            .filter_map(|(window, _)| {
                let namespace = self.windows.get(*window)?.namespace;
                self.window_presentation_root_and_offset(namespace, *window).ok().map(|r| r.1)
            }).collect::<BTreeSet<_>>().into_iter().collect()
    }

    pub(crate) fn observe_prepared_surface_clock(
        &mut self, surface: sophia_protocol::SurfaceId, sample: crate::XPresentClockSample,
        previous: Option<crate::XPresentClockSample>,
    ) -> Result<bool, &'static str> {
        let windows = self.prepared_present_schedules.keys().copied().filter(|window| {
            self.windows.get(*window).and_then(|record|
                self.window_presentation_root_and_offset(record.namespace, *window).ok())
                .is_some_and(|root| root.1 == surface)
        }).collect::<Vec<_>>();
        let mut ready = false;
        let mut idle = Vec::new();
        for window in windows {
            let state = self.prepared_present_schedules.get(&window).expect("window collected above");
            // A bad 'previous' belongs to the window's existing binding;
            // never allocate rejection state for arbitrary supplied sources.
            let source = state.clock.binding().map_or(sample.source, |binding| binding.source);
            if state.rejects_source(source) && source == sample.source { continue; }
            let before = state.queue.ready().count();
            let deadline = state.queue.background_deadline();
            let state = self.prepared_present_schedules.get_mut(&window).expect("window remains scheduled");
            if let Err((source, error)) = state.observe_clock(sample, previous) {
                if !state.rejects_source(source) {
                    idle.extend(state.rejects_observation(source, error, &mut self.present_timing_statistics));
                    ready |= state.rejects_source(source) || before != state.queue.ready().count();
                }
                self.publish_present_clock_interest(window);
                continue;
            }
            self.prepared_present_schedules.get_mut(&window).expect("window remains scheduled").rejections.remove(&source);
            let state = self.prepared_present_schedules.get(&window).expect("window remains scheduled");
            ready |= before != state.queue.ready().count()
                || deadline != state.queue.background_deadline();
            self.publish_present_clock_interest(window);
        }
        self.scrap_clock_lost_preparations(idle)?;
        Ok(ready)
    }
}

/// Successfully executed Pixmaps, distinct from composed frames and from
/// queued or scrapped requests. Rectangle areas are clipped sums, not unions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XPresentDamageStatistics {
    pub absent: u64,
    pub explicit_full_rect: u64,
    pub explicit_regions: u64,
    pub effective_empty: u64,
    pub source_pixels: u64,
    pub rect_pixels: u64,
    pub rects: u64,
}

impl XPresentDamageStatistics {
    fn from_request(request: &XPreparedPresent) -> Self {
        let mut result = Self::default();
        let full = Rect {
            x: 0,
            y: 0,
            width: request.pixmap_size.width,
            height: request.pixmap_size.height,
        };
        if !request.has_update_region {
            result.absent = 1;
        } else if request.source_damage.is_empty() {
            result.effective_empty = 1;
        } else if request.source_damage.contains(&full) {
            result.explicit_full_rect = 1;
        } else {
            result.explicit_regions = 1;
        }
        result.source_pixels = (full.width as u64).saturating_mul(full.height as u64);
        for rect in &request.source_damage {
            result.rect_pixels = result
                .rect_pixels
                .saturating_add((rect.width as u64).saturating_mul(rect.height as u64));
            result.rects = result.rects.saturating_add(1);
        }
        result
    }

    fn add(&mut self, other: Self) {
        macro_rules! add { ($($field:ident),+ $(,)?) => { $(self.$field = self.$field.saturating_add(other.$field);)+ }; }
        add!(
            absent,
            explicit_full_rect,
            explicit_regions,
            effective_empty,
            source_pixels,
            rect_pixels,
            rects
        );
    }
}

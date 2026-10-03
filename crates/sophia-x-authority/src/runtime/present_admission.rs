/// Request-scoped demand for Session's source selection. No X resource names
/// cross this boundary; requests cancelled before binding simply disappear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentClockAdmission {
    pub request: TransactionId,
    /// None is an explicit fake-clock demand, not a missing admission. A
    /// valid window need not have a viewable compositor-facing ancestor.
    pub target: Option<(sophia_protocol::SurfaceId, Rect)>,
}

#[derive(Debug)]
struct XQueuedMscNotify {
    client: u64,
    namespace: NamespaceId,
    window: crate::XResourceId,
    serial: u32,
    // Some until Session freezes this request's source; None once scheduled.
    timing: Option<crate::XPresentMscTiming>,
    wire_ready: bool,
    wire_request: bool,
    terminal: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XReadyMscNotify {
    pub request: TransactionId,
    pub window: crate::XResourceId,
    pub serial: u32,
    pub ust: u64,
    pub msc: u64,
}

impl XAuthorityRuntime {
    pub(crate) fn validate_present_window(&self, namespace: NamespaceId, window: crate::XResourceId)
        -> Result<(), XAuthorityRuntimeError>
    {
        // The setup root, WM-check window and clipboard proxies are valid X
        // windows even though they have no compositor presentation record.
        if window.local.raw() == u64::from(crate::X_SETUP_DEFAULT_ROOT) { return Ok(()); }
        self.validate_window_access(namespace, window)
    }

    fn validate_prepared_present_capacity(&self, client: u64, request: TransactionId)
        -> Result<(), XPresentPreparationError>
    {
        if client == 0 || !request.is_valid() {
            return Err(XPresentPreparationError::Invalid(XAuthorityRuntimeError::InvalidResource));
        }
        if self.prepared_presents.contains_key(&request) || self.prepared_msc_notifies.contains_key(&request) {
            return Err(XPresentPreparationError::DuplicateTransaction);
        }
        // Both unbound and bound NotifyMSC remain charged until delivery or
        // cancellation. Pixmaps also retain the socket feedback reservation
        // through Complete+Idle; that separate bound includes executed work.
        if self.prepared_presents.len() + self.prepared_msc_notifies.len() >= X_PREPARED_PRESENT_CAPACITY
            || self.prepared_presents.values().filter(|p| p.client == client).count()
                + self.prepared_msc_notifies.values().filter(|p| p.client == client).count()
                >= X_PRESENT_PER_CLIENT_CAPACITY
        {
            return Err(XPresentPreparationError::Capacity);
        }
        Ok(())
    }

    /// Adds timing demand only; the already retained pixmap is still not
    /// executed and this method neither samples nor predicts a clock.
    pub fn request_prepared_present_clock(&mut self, request: TransactionId, timing: crate::XPresentMscTiming)
        -> Result<(), XPreparedPresentScheduleError>
    {
        let prepared = self.prepared_presents.get_mut(&request)
            .ok_or(XPreparedPresentScheduleError::MissingPreparation)?;
        if prepared.terminal_fake_owed || prepared.terminal.is_some() || prepared.timing.is_some() || self.prepared_present_schedules.get(&prepared.request.window)
            .is_some_and(|state| state.queue.get(request).is_some())
        {
            return Err(XPreparedPresentScheduleError::Queue(crate::XPresentScheduleError::DuplicateRequest));
        }
        prepared.timing = Some(timing);
        Ok(())
    }

    /// A resource-free NotifyMSC shares the window's source/offset and target
    /// ordering, but never has a renderer ticket, pixmap or Idle obligation.
    /// Invalid remainder is rejected before admission; the caller translates
    /// Capacity to a protocol allocation error, never a frontend failure.
    pub fn prepare_present_msc_notify(
        &mut self, client: u64, request: TransactionId, namespace: NamespaceId,
        window: crate::XResourceId, serial: u32, timing: crate::XPresentMscTiming,
    ) -> Result<(), XPresentPreparationError> {
        self.prepare_present_msc_notify_with_publication(client, request, namespace,
            window, serial, timing, XPresentPublication::Direct)
    }

    pub(crate) fn prepare_present_msc_notify_with_publication(
        &mut self, client: u64, request: TransactionId, namespace: NamespaceId,
        window: crate::XResourceId, serial: u32, timing: crate::XPresentMscTiming, publication: XPresentPublication,
    ) -> Result<(), XPresentPreparationError> {
        self.validate_prepared_present_capacity(client, request)?;
        self.validate_present_window(namespace, window).map_err(XPresentPreparationError::Invalid)?;
        self.require_present_service();
        self.prepared_msc_notifies.insert(request, XQueuedMscNotify {
            client, namespace, window, serial, timing: Some(timing),
            wire_ready: publication == XPresentPublication::Direct,
            wire_request: publication == XPresentPublication::PendingWire, terminal: None,
        });
        if publication == XPresentPublication::PendingWire {
            self.present_timing_statistics.wire_prepared = self.present_timing_statistics.wire_prepared.saturating_add(1);
        }
        Ok(())
    }

    /// Stable request order, bounded by the combined preparation limit.
    /// Geometry is only a fallback for Session's actual sampling table.
    /// Never filter out an unresolved target: it would leave this request
    /// unbound and block all later requests on the same window.
    pub fn present_clock_admissions(&self) -> Vec<XPresentClockAdmission> {
        let requests = self.prepared_presents.iter().filter(|(_, p)| p.wire_ready && p.timing.is_some())
            .map(|(id, p)| (*id, p.request.namespace, p.request.window))
            .chain(self.prepared_msc_notifies.iter().filter(|(_, p)| p.wire_ready && p.timing.is_some())
                .map(|(id, p)| (*id, p.namespace, p.window)));
        let mut admissions = requests.map(|(request, namespace, window)| {
            let target = (|| {
                if self.window_map_state(namespace, window).ok()? != crate::XMapState::Viewable {
                    return None;
                }
                let (root, surface, _, _) = self.window_presentation_root_and_offset(namespace, window).ok()?;
                let geometry = self.window_geometry(namespace, root).ok()?;
                Some((surface, geometry))
            })();
            XPresentClockAdmission { request, target }
        }).collect::<Vec<_>>();
        admissions.sort_unstable_by_key(|a| a.request.raw());
        admissions
    }

    /// A wire preparation starts unpublished. Only the dispatch thread which
    /// published its original envelope may make it visible to Session.
    /// Destroy/disconnect can remove it before publication.
    pub(crate) fn publish_prepared_present_wire(&mut self, request: TransactionId) -> bool {
        let state = self.prepared_presents.get_mut(&request).map(|p| &mut p.wire_ready)
            .or_else(|| self.prepared_msc_notifies.get_mut(&request).map(|p| &mut p.wire_ready));
        let Some(state) = state else { return false; };
        if *state { return false; }
        *state = true;
        self.present_timing_statistics.wire_published = self.present_timing_statistics.wire_published.saturating_add(1);
        true
    }

    pub(crate) fn note_present_admission_owner_notification(&mut self) {
        self.present_timing_statistics.wire_owner_notifications = self.present_timing_statistics.wire_owner_notifications.saturating_add(1);
    }

    /// None means cancelled or already bound: an asynchronous selection must
    /// never rebind a request after its target has been frozen. Earlier
    /// requests on this window bind first; unrelated windows may progress.
    pub fn bind_present_clock_admission(&mut self, request: TransactionId,
        sample: crate::XPresentClockSample, previous: Option<crate::XPresentClockSample>)
        -> Result<Option<Vec<TransactionId>>, XPreparedPresentScheduleError>
    {
        let admission = self.prepared_presents.get(&request).filter(|p| p.wire_ready).and_then(|p|
                p.timing.map(|timing| (p.request.window, timing, crate::XPresentScheduledKind::Pixmap)))
            .or_else(|| self.prepared_msc_notifies.get(&request).filter(|p| p.wire_ready).and_then(|p|
                p.timing.map(|timing| (p.window, timing, crate::XPresentScheduledKind::NotifyMsc))));
        let Some((window, timing, kind)) = admission else { return Ok(None); };
        if self.prepared_presents.range(..request).any(|(_, p)| p.request.window == window && p.timing.is_some())
            || self.prepared_msc_notifies.range(..request).any(|(_, p)| p.window == window && p.timing.is_some())
        {
            return Ok(None);
        }
        // Selection runs outside runtime. A frontend fake-clock turn or a
        // retirement can advance this source before binding reacquires it.
        // Use an already accepted sample, never rewind or fabricate a count.
        let sample = self.prepared_present_schedules.get(&window).map_or(sample, |state| {
            state.clock.sample().ok().into_iter()
                .chain(state.queue.latest_observation(sample.source))
                .filter(|accepted| accepted.source == sample.source && accepted.ust > sample.ust)
                .max_by_key(|accepted| accepted.ust).unwrap_or(sample)
        });
        let superseded = if kind == crate::XPresentScheduledKind::Pixmap {
            self.schedule_prepared_present(request, timing, sample, previous)?
        } else {
            let superseded = self.schedule_present_clock_request(window, request, kind, false, timing, sample, previous)?;
            debug_assert!(superseded.is_empty(), "NotifyMSC never scraps a pixmap");
            self.prepared_msc_notifies.get_mut(&request).expect("request held under runtime").timing = None;
            Vec::new()
        };
        let wire_request = self.prepared_presents.get(&request).is_some_and(|p| p.wire_started.is_some())
            || self.prepared_msc_notifies.get(&request).is_some_and(|p| p.wire_request);
        if wire_request {
            let stats = &mut self.present_timing_statistics;
            stats.wire_bound = stats.wire_bound.saturating_add(1);
            if matches!(sample.source, crate::XPresentClockSource::Hardware { .. }) {
                stats.wire_hardware_bound = stats.wire_hardware_bound.saturating_add(1);
            }
        }
        Ok(Some(superseded))
    }

    pub(crate) fn ready_prepared_msc_notifies(&self) -> Vec<XReadyMscNotify> {
        self.prepared_present_schedules.values().flat_map(|state|
            state.queue.ready().filter(|r| r.kind == crate::XPresentScheduledKind::NotifyMsc)
                .filter_map(|r| {
                    let notify = self.prepared_msc_notifies.get(&r.request)?;
                    let (ust, msc) = state.queue.completion_sample(r.request)?;
                    Some(XReadyMscNotify { request: r.request, window: notify.window, serial: notify.serial, ust, msc })
                })).chain(self.prepared_msc_notifies.iter().filter_map(|(&request, notify)| {
                    let (ust, msc) = notify.terminal?;
                    Some(XReadyMscNotify { request, window: notify.window, serial: notify.serial, ust, msc })
                })).collect()
    }

    pub fn cancel_prepared_msc_notify(&mut self, request: TransactionId) -> bool {
        let Some(notify) = self.prepared_msc_notifies.remove(&request) else { return false; };
        self.unschedule_prepared_present(notify.window, request);
        true
    }

    pub fn prepared_msc_notify_count(&self) -> usize { self.prepared_msc_notifies.len() }
}

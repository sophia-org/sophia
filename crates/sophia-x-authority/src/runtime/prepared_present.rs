/// Includes prepared, executing and feedback-pending requests at the socket.
/// A configured socket capacity may be smaller, but never exceeds this cap.
pub const X_PRESENT_PER_CLIENT_CAPACITY: usize = 64;
/// Additional global bound on requests waiting to execute. These have not
/// entered the renderer's separately bounded presentation registry yet.
pub const X_PREPARED_PRESENT_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XPresentFenceResources {
    pub wait: Option<crate::XResourceId>,
    pub idle: Option<crate::XResourceId>,
}

/// Private, non-reused handles resolved at request admission. Destruction
/// clears a queued reference: Present must stop waiting on a destroyed wait
/// fence and must not signal a destroyed idle fence (presentproto 260-266).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XPreparedPresentFences {
    pub wait: Option<sophia_protocol::FenceHandle>,
    pub idle: Option<sophia_protocol::FenceHandle>,
}

/// Execution has a fresh authority ticket; the original request's feedback
/// reservation must be transferred to it under the same runtime-lock hold.
/// These private fence handles are the ones to submit to the renderer.
#[derive(Debug)]
#[must_use]
pub struct XPreparedPresentExecution {
    pub preparation: TransactionId,
    pub client: u64,
    pub namespace: NamespaceId,
    pub window: crate::XResourceId,
    pub offset: (i16, i16),
    pub fences: XPreparedPresentFences,
    pub schedule: Option<crate::XPresentScheduledRequest>,
    pub clock_sample: Option<crate::XPresentClockSample>,
    pub response: XAuthorityResponsePacket,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPresentExecutionError {
    InvalidTransaction,
    Superseded,
    Fence(&'static str),
}

/// The request retains a backing, not a copy of its pixels. Execution reads
/// that backing after its acquire fence and timing gate, as Present requires.
#[derive(Debug)]
struct XPreparedPresent {
    transaction: TransactionId,
    namespace: NamespaceId,
    window: crate::XResourceId,
    pixmap: crate::XResourceId,
    x_offset: i16,
    y_offset: i16,
    has_valid_region: bool,
    has_update_region: bool,
    source_damage: Vec<Rect>,
    pixmap_size: Size,
}

/// Publication is decided at construction. A wire request must not become
/// visible to Session before its original authority envelope is published.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XPresentPublication { Direct, PendingWire }

#[derive(Debug)]
struct XQueuedPresent {
    client: u64,
    request: XPreparedPresent,
    timing: Option<crate::XPresentMscTiming>,
    wire_ready: bool,
    wire_started: Option<std::time::Instant>,
    // Terminal local refusal, still charged until the service delivers Skip.
    terminal: Option<(u64, u64)>,
    // Frozen accepted pair survives a damaged/missing schedule on error paths.
    anchor: Option<(u64, u64)>,
    // Missing every saved pair: service supplies Fake time for terminal Skip.
    terminal_fake_owed: bool,
    fences: XPreparedPresentFences,
    // A scrapped pixmap keeps only its completion reservation until target.
    // Its resource keys are metadata, never backing references, after this
    // flag is cleared. This also keeps it charged to both queue bounds.
    backing_live: bool,
    idle_owed: bool,
    fence_retry: XPresentFenceRetry,
}

/// xshmfence can be triggered by a client writing shared memory without an
/// X request or pollable fd. Back off those checks to 32 ms; unrelated owner
/// wakeups never reset the delay. No retry survives execution/cancellation.
#[derive(Debug, Default)]
struct XPresentFenceRetry {
    next_usec: u64,
    delay_usec: u64,
}

impl XPresentFenceRetry {
    fn blocked(&mut self, now_usec: u64) {
        self.delay_usec = self.delay_usec.saturating_mul(2).clamp(1_000, 32_000);
        self.next_usec = now_usec.saturating_add(self.delay_usec);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPresentPreparationError {
    Invalid(XAuthorityRuntimeError),
    DuplicateTransaction,
    Capacity,
}

impl core::fmt::Display for XPresentPreparationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Invalid(_) => "invalid prepared Present request",
            Self::DuplicateTransaction => "duplicate prepared Present transaction",
            Self::Capacity => "prepared Present capacity exhausted",
        })
    }
}

impl std::error::Error for XPresentPreparationError {}

impl XAuthorityRuntime {
    pub(crate) fn prepared_present_origin(&self, preparation: TransactionId)
        -> Option<(u64, NamespaceId, crate::XResourceId)>
    {
        self.prepared_presents.get(&preparation)
            .map(|p| (p.client, p.request.namespace, p.request.window))
    }

    fn standard_pixmap_request(
        &self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        pixmap: crate::XResourceId,
        x_offset: i16,
        y_offset: i16,
        valid_region: Option<&Region>,
        update_region: Option<&Region>,
    ) -> Result<XPreparedPresent, XAuthorityRuntimeError> {
        if !transaction.is_valid() {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        self.validate_present_window(namespace, window)?;
        self.validate_pixmap_access(namespace, pixmap)?;
        if valid_region.is_some_and(|r| r.rects.len() > X_AUTHORITY_PRESENT_REGION_MAX_RECTS) {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let pixmap_size = self
            .pixmaps
            .get(&pixmap)
            .ok_or(XAuthorityRuntimeError::UnknownResource)?
            .size;
        let source_damage = present_source_damage(pixmap_size, update_region)
            .ok_or(XAuthorityRuntimeError::InvalidResource)?;
        Ok(XPreparedPresent {
            transaction,
            namespace,
            window,
            pixmap,
            x_offset,
            y_offset,
            has_valid_region: valid_region.is_some(),
            has_update_region: update_region.is_some(),
            source_damage,
            pixmap_size,
        })
    }

    /// Existing immediate execution. No queue allocation or delayed-request
    /// capacity is consumed on this path.
    pub fn present_standard_pixmap(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        pixmap: crate::XResourceId,
        x_offset: i16,
        y_offset: i16,
        valid_region: Option<Region>,
        update_region: Option<Region>,
    ) -> XAuthorityResponsePacket {
        match self.standard_pixmap_request(
            transaction,
            namespace,
            window,
            pixmap,
            x_offset,
            y_offset,
            valid_region.as_ref(),
            update_region.as_ref(),
        ) {
            Ok(request) => self.execute_standard_pixmap_request(request),
            Err(error) => XAuthorityResponsePacket::rejected(transaction, error),
        }
    }

    /// Validate and retain without changing the window, its generation, its
    /// presentation raster or any CPU publication. The socket's pending
    /// Present reservation must remain live until Complete and Idle; this
    /// additional bound limits requests that have not executed yet.
    pub fn prepare_standard_pixmap(
        &mut self,
        client: u64,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        pixmap: crate::XResourceId,
        offset: (i16, i16),
        valid_region: Option<Region>,
        update_region: Option<Region>,
        fences: XPresentFenceResources,
    ) -> Result<(), XPresentPreparationError> {
        self.prepare_standard_pixmap_with_publication(client, transaction, namespace,
            window, pixmap, offset, (valid_region, update_region), fences, XPresentPublication::Direct)
    }

    pub(crate) fn prepare_standard_pixmap_with_publication(
        &mut self,
        client: u64,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
        pixmap: crate::XResourceId,
        offset: (i16, i16),
        regions: (Option<Region>, Option<Region>),
        fences: XPresentFenceResources,
        publication: XPresentPublication,
    ) -> Result<(), XPresentPreparationError> {
        let (valid_region, update_region) = regions;
        if client == 0 {
            return Err(XPresentPreparationError::Invalid(
                XAuthorityRuntimeError::InvalidResource,
            ));
        }
        self.validate_prepared_present_capacity(client, transaction)?;
        let request = self
            .standard_pixmap_request(
                transaction,
                namespace,
                window,
                pixmap,
                offset.0,
                offset.1,
                valid_region.as_ref(),
                update_region.as_ref(),
            )
            .map_err(XPresentPreparationError::Invalid)?;
        let fences = XPreparedPresentFences {
            wait: fences
                .wait
                .map(|f| self.dri3_fence_handle(namespace, f))
                .transpose()
                .map_err(XPresentPreparationError::Invalid)?,
            idle: fences
                .idle
                .map(|f| self.dri3_fence_handle(namespace, f))
                .transpose()
                .map_err(XPresentPreparationError::Invalid)?,
        };
        self.require_present_service();
        self.prepared_presents.insert(
            transaction,
            XQueuedPresent {
                client,
                request,
                timing: None,
                wire_ready: publication == XPresentPublication::Direct,
                wire_started: (publication == XPresentPublication::PendingWire).then(std::time::Instant::now),
                terminal: None,
                anchor: None,
                terminal_fake_owed: false,
                fences,
                backing_live: true,
                idle_owed: false,
                fence_retry: Default::default(),
            },
        );
        if publication == XPresentPublication::PendingWire {
            self.present_timing_statistics.wire_prepared = self.present_timing_statistics.wire_prepared.saturating_add(1);
        }
        Ok(())
    }

    /// Read under the same authority lock as the timing/fence gate and
    /// execution. No public XID lookup is permitted at the deadline.
    pub fn prepared_present_fences(
        &self,
        transaction: TransactionId,
    ) -> Option<XPreparedPresentFences> {
        self.prepared_presents.get(&transaction).map(|p| p.fences)
    }

    fn forget_prepared_present_fence(&mut self, handle: sophia_protocol::FenceHandle) {
        self.present_fence_descriptors.remove(&handle);
        for prepared in self.prepared_presents.values_mut() {
            if prepared.fences.wait == Some(handle) {
                prepared.fences.wait = None;
            }
            if prepared.fences.idle == Some(handle) {
                prepared.fences.idle = None;
            }
        }
    }

    pub(crate) fn retain_present_fence_descriptor(
        &mut self, handle: sophia_protocol::FenceHandle, fd: Arc<OwnedFd>,
    ) -> Result<(), XAuthorityRuntimeError> {
        if !self.dri3_fences.values().any(|f| *f == handle)
            || self.present_fence_descriptors.contains_key(&handle)
        {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        self.present_fence_descriptors.insert(handle, fd);
        Ok(())
    }

    pub(crate) fn present_fence_ready(&self, handle: Option<sophia_protocol::FenceHandle>)
        -> Result<bool, &'static str>
    {
        let Some(handle) = handle else { return Ok(true); };
        let fd = self.present_fence_descriptors.get(&handle)
            .ok_or("prepared Present fence descriptor missing")?;
        sophia_xshmfence::query(fd).map_err(|_| "prepared Present fence query failed")
    }

    pub(crate) fn poll_prepared_present_fence(&mut self, preparation: TransactionId, now_usec: u64)
        -> Result<bool, &'static str>
    {
        let Some(prepared) = self.prepared_presents.get(&preparation) else { return Ok(false); };
        let window = prepared.request.window;
        let wait = prepared.fences.wait;
        let retry_at = prepared.fence_retry.next_usec;
        if !self.prepared_present_schedules.get_mut(&window)
            .is_some_and(|s| s.queue.begin_execution(preparation)) { return Ok(false); }
        if wait.is_none() { return Ok(true); }
        if now_usec < retry_at { return Ok(false); }
        self.present_timing_statistics.fence_queries = self.present_timing_statistics.fence_queries.saturating_add(1);
        if self.present_fence_ready(wait)? { return Ok(true); }
        self.present_timing_statistics.fence_blocked = self.present_timing_statistics.fence_blocked.saturating_add(1);
        self.prepared_presents.get_mut(&preparation).expect("preparation remains locked")
            .fence_retry.blocked(now_usec);
        Ok(false)
    }

    pub(crate) fn signal_prepared_present_idle(&self, preparation: TransactionId)
        -> Result<(), &'static str>
    {
        let Some(handle) = self.prepared_presents.get(&preparation).and_then(|p| p.fences.idle)
            else { return Ok(()); };
        self.signal_present_idle_fence(Some(handle))
    }

    fn signal_present_idle_fence(&self, handle: Option<sophia_protocol::FenceHandle>) -> Result<(), &'static str> {
        let Some(handle) = handle else { return Ok(()); };
        let fd = self.present_fence_descriptors.get(&handle)
            .ok_or("prepared Present idle fence descriptor missing")?;
        sophia_xshmfence::trigger(fd).map_err(|_| "prepared Present idle fence signal failed")
    }

    /// The queued Skip retains no pixmap or fence reference. Its socket
    /// reservation still owes Idle now and Complete at the frozen target.
    fn scrap_prepared_present(&mut self, preparation: TransactionId) {
        let prepared = self.prepared_presents.get_mut(&preparation)
            .expect("queued pixmap owns a preparation");
        assert!(prepared.backing_live, "a pixmap is scrapped only once");
        prepared.backing_live = false;
        prepared.idle_owed = true;
        prepared.fences = XPreparedPresentFences::default();
        let backing = prepared.request.pixmap;
        self.release_prepared_pixmap(backing);
    }

    /// Called only after timing and acquire-fence admission. Allocate a fresh
    /// execution ticket only after checking the preparation still exists.
    /// The caller must publish that ticket even for a rejected response
    /// (an empty envelope), so the ordered authority stream has no hole.
    /// A destroyed window's request is cancelled and returns None.
    pub fn execute_prepared_standard_pixmap(
        &mut self,
        preparation: TransactionId,
        execution: TransactionId,
    ) -> Result<Option<XPreparedPresentExecution>, XPresentExecutionError> {
        if !execution.is_valid()
            || execution.raw() <= preparation.raw()
            || self.prepared_presents.contains_key(&execution)
        {
            return Err(XPresentExecutionError::InvalidTransaction);
        }
        if self.prepared_presents.get(&preparation).is_some_and(|p| !p.backing_live) {
            return Err(XPresentExecutionError::Superseded);
        }
        let Some(mut prepared) = self.prepared_presents.remove(&preparation) else {
            return Ok(None);
        };
        if let Some(started) = prepared.wire_started {
            self.present_timing_statistics.record_wire_execution_wait(started.elapsed());
        }
        let backing = prepared.request.pixmap;
        let namespace = prepared.request.namespace;
        let window = prepared.request.window;
        let offset = (prepared.request.x_offset, prepared.request.y_offset);
        let schedule = self.prepared_present_schedules.get(&window).and_then(|s| s.queue.get(preparation));
        let clock_sample = self.prepared_present_schedules.get(&window).and_then(|s| s.queue.observation(preparation));
        // No extra raster or damage clone on successful execution. A failed
        // queued execution needs only enough metadata to finish its Skip.
        let skip_request = XPreparedPresent {
            transaction: preparation, namespace, window, pixmap: backing,
            x_offset: offset.0, y_offset: offset.1,
            has_valid_region: false, has_update_region: false, source_damage: Vec::new(),
            pixmap_size: prepared.request.pixmap_size,
        };
        prepared.request.transaction = execution;
        let response = self.execute_standard_pixmap_request(prepared.request);
        if schedule.is_some() && response.transactions.is_empty() {
            self.signal_present_idle_fence(prepared.fences.idle)
                .map_err(XPresentExecutionError::Fence)?;
            assert!(self.prepared_present_schedules.get_mut(&window).expect("scheduled window")
                .queue.scrap(preparation), "executing request must still own its target");
            self.prepared_presents.insert(preparation, XQueuedPresent {
                client: prepared.client, request: skip_request, timing: None, wire_ready: true, wire_started: None, terminal: None,
                anchor: prepared.anchor, terminal_fake_owed: false,
                fences: XPreparedPresentFences::default(), backing_live: false, idle_owed: true,
                fence_retry: Default::default(),
            });
        } else {
            self.unschedule_prepared_present(window, preparation);
        }
        self.release_prepared_pixmap(backing);
        Ok(Some(XPreparedPresentExecution {
            preparation,
            client: prepared.client,
            namespace,
            window,
            offset,
            fences: prepared.fences,
            schedule,
            clock_sample,
            response,
        }))
    }

    pub fn cancel_prepared_standard_pixmap(&mut self, transaction: TransactionId) -> bool {
        let Some(prepared) = self.prepared_presents.remove(&transaction) else {
            return false;
        };
        self.unschedule_prepared_present(prepared.request.window, transaction);
        if prepared.backing_live {
            self.release_prepared_pixmap(prepared.request.pixmap);
        }
        true
    }

    /// The valid X window has no renderer route at execution (for example
    /// after unmapping). Keep its timed Skip and release pixels immediately;
    /// this is not a BadWindow or a renderer-ticket obligation.
    pub(crate) fn skip_unrouted_prepared_present(&mut self, preparation: TransactionId)
        -> Result<(), &'static str>
    {
        let window = self.prepared_presents.get(&preparation)
            .ok_or("unrouted Present preparation missing")?.request.window;
        if !self.prepared_present_schedules.get_mut(&window)
            .is_some_and(|state| state.queue.scrap(preparation))
        {
            return Err("unrouted Present timing missing");
        }
        self.retire_scrapped_prepared_present(preparation);
        Ok(())
    }

    pub fn cancel_client_prepared_presents(&mut self, client: u64) {
        let notifies = self.prepared_msc_notifies.iter()
            .filter_map(|(id, n)| (n.client == client).then_some(*id)).collect::<Vec<_>>();
        for id in notifies { self.cancel_prepared_msc_notify(id); }
        let transactions = self
            .prepared_presents
            .iter()
            .filter_map(|(id, p)| (p.client == client).then_some(*id))
            .collect::<Vec<_>>();
        for transaction in transactions {
            self.cancel_prepared_standard_pixmap(transaction);
        }
    }

    fn cancel_window_prepared_presents(&mut self, window: crate::XResourceId) {
        let notifies = self.prepared_msc_notifies.iter()
            .filter_map(|(id, n)| (n.window == window).then_some(*id)).collect::<Vec<_>>();
        for id in notifies { self.cancel_prepared_msc_notify(id); }
        self.prepared_present_schedules.remove(&window);
        self.publish_present_clock_interest(window);
        let transactions = self
            .prepared_presents
            .iter()
            .filter_map(|(id, p)| (p.request.window == window).then_some(*id))
            .collect::<Vec<_>>();
        for transaction in transactions {
            self.cancel_prepared_standard_pixmap(transaction);
        }
    }

    fn release_prepared_pixmap(&mut self, backing: crate::XResourceId) {
        if let Some(retained) = self.retained_pixmap_backings.get_mut(&backing) {
            retained.presents = retained
                .presents
                .checked_sub(1)
                .expect("prepared Present holds its retained pixmap");
            self.maybe_drop_retained_pixmap(backing);
        }
    }

    pub fn prepared_present_count(&self) -> usize {
        self.prepared_presents.len()
    }
}

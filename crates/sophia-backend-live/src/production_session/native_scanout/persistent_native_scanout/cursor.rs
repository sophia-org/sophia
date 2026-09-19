use super::*;

pub fn project_native_cursor_logical_viewport(
    position: sophia_protocol::Point,
    logical_viewports: &[(OutputId, sophia_protocol::Rect)],
) -> Result<Option<(OutputId, i32, i32, sophia_protocol::Size)>, &'static str> {
    if !position.x.is_finite() || !position.y.is_finite() {
        return Ok(None);
    }
    let mut seen = BTreeSet::new();
    if logical_viewports.is_empty()
        || logical_viewports.iter().any(|(output, logical)| {
            !output.is_valid() || logical.is_empty() || !seen.insert(*output)
        })
    {
        return Err("hardware cursor projection has invalid logical viewports");
    }
    let x = position.x.floor() as i32;
    let y = position.y.floor() as i32;
    Ok(logical_viewports.iter().find_map(|(output, logical)| {
        (x >= logical.x
            && x < logical.x.saturating_add(logical.width)
            && y >= logical.y
            && y < logical.y.saturating_add(logical.height))
        .then_some((
            *output,
            x.saturating_sub(logical.x),
            y.saturating_sub(logical.y),
            sophia_protocol::Size {
                width: logical.width,
                height: logical.height,
            },
        ))
    }))
}

impl LiveProductionNativeScanout {
    /// What the card said about a cursor plane, once any group has asked.
    ///
    /// One answer per card, and the groups agree by construction: the buffer
    /// and its format belong to the card, not the head.
    /// Drive the cursor atomically, if the card will take it.
    ///
    /// Asked for rather than assumed: the probe says what the card would
    /// accept, and this says what the session chose. A card that refused
    /// keeps the legacy ioctl, which archive `0004` proved works over
    /// directly scanned frames, so there is nothing to gain by insisting.
    pub fn use_atomic_cursor_plane(&mut self) -> crate::HardwareCursorPath {
        let groups_usable = !self.groups.is_empty()
            && self.groups.iter().all(|group| {
                group.session.cursor_plane_probe() == Some(crate::CursorPlaneProbe::Accepted)
                    && group.session.atomic_cursor_resources().is_some()
            });
        let mut heads_usable = !self.heads.is_empty();
        for index in 0..self.heads.len() {
            let Some(plane) = self.heads[index].selection.cursor_plane() else {
                heads_usable = false;
                continue;
            };
            let group = self.heads[index].group;
            let properties = crate::discover_cursor_plane_properties(
                self.groups[group].session.cursor_commit_device(),
                plane,
            );
            self.heads[index].cursor_properties = properties;
            heads_usable &= properties.is_some();
        }
        // Atomic is a topology-wide decision. Mixing it with legacy updates
        // leaves one card's cursor state outside the transaction owner and
        // makes cross-head motion impossible to reason about.
        self.cursor_path = if groups_usable && heads_usable {
            crate::HardwareCursorPath::AtomicPlane
        } else {
            crate::HardwareCursorPath::LegacyIoctl
        };
        self.cursor_path
    }

    pub fn cursor_plane_probe(&self) -> Option<crate::CursorPlaneProbe> {
        let mut probes = self
            .groups
            .iter()
            .map(|group| group.session.cursor_plane_probe());
        let first = probes.next().flatten()?;
        if first == crate::CursorPlaneProbe::Refused
            || probes.any(|probe| probe != Some(crate::CursorPlaneProbe::Accepted))
        {
            Some(crate::CursorPlaneProbe::Refused)
        } else {
            Some(crate::CursorPlaneProbe::Accepted)
        }
    }

    pub fn update_classic_hardware_cursor(
        &mut self,
        position: sophia_protocol::Point,
        logical_viewports: &[(OutputId, sophia_protocol::Rect)],
    ) -> Result<crate::ClassicHardwareCursorUpdate, Box<dyn std::error::Error>> {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Ok(crate::ClassicHardwareCursorUpdate::Hidden);
        }
        let primary_in_flight = self.heads.iter().any(|head| head.submitted_at.is_some());
        let initialized = self
            .groups
            .iter()
            .all(|group| group.session.classic_hardware_cursor_initialized());
        match crate::legacy_hardware_cursor_admission(initialized, primary_in_flight) {
            crate::LegacyHardwareCursorAdmission::DeferredInitialization => {
                self.cursor_initialization_deferrals =
                    self.cursor_initialization_deferrals.saturating_add(1);
                return Ok(crate::ClassicHardwareCursorUpdate::Deferred);
            }
            // Only the atomic path can produce this: an ioctl never waits for
            // a flip. Stated rather than folded into the arm below so that
            // routing this caller through the atomic path later is a compile
            // error here instead of a cursor that silently stops moving.
            crate::LegacyHardwareCursorAdmission::DeferredUpdate => {
                return Err("the legacy cursor path was told to defer an update".into());
            }
            crate::LegacyHardwareCursorAdmission::InitializeThenUpdate => {
                let initialization_started = Instant::now();
                for group in &mut self.groups {
                    if let Err(error) = group.session.initialize_classic_hardware_cursor() {
                        self.max_cursor_initialization = self
                            .max_cursor_initialization
                            .max(initialization_started.elapsed());
                        self.cursor_update_failures = self.cursor_update_failures.saturating_add(1);
                        return Err(
                            format!("hardware cursor initialization failed: {error}").into()
                        );
                    }
                }
                self.max_cursor_initialization = self
                    .max_cursor_initialization
                    .max(initialization_started.elapsed());
            }
            crate::LegacyHardwareCursorAdmission::Update => {}
        }

        let update_started = Instant::now();
        let native_outputs = self.outputs();
        let expected = native_outputs
            .iter()
            .map(|output| output.id)
            .collect::<BTreeSet<_>>();
        let actual = logical_viewports
            .iter()
            .map(|(output, _)| *output)
            .collect::<BTreeSet<_>>();
        if expected != actual
            || actual.len() != logical_viewports.len()
            || logical_viewports
                .iter()
                .any(|(_, logical)| logical.is_empty())
        {
            return Err("hardware cursor projection has stale logical viewports".into());
        }
        let mut targets =
            BTreeMap::<usize, Vec<(crate::LibdrmNativePrimaryPlaneSelection, i32, i32)>>::new();
        // Where the cursor lands on each head, by head index. Heads absent
        // from this map are heads the pointer is not on, and they are told to
        // hide rather than left alone -- a head with nothing said to it is
        // how a cursor ends up showing on two monitors at once.
        let mut head_positions = BTreeMap::<usize, (i32, i32)>::new();
        let (hotspot_x, hotspot_y) = self
            .groups
            .first()
            .map(|group| group.session.hardware_cursor_hotspot())
            .ok_or("hardware cursor has no card group")?;
        let hotspot_x = i32::try_from(hotspot_x).map_err(|_| "cursor hotspot exceeds i32")?;
        let hotspot_y = i32::try_from(hotspot_y).map_err(|_| "cursor hotspot exceeds i32")?;
        if let Some((output, logical_x, logical_y, logical_size)) =
            project_native_cursor_logical_viewport(position, logical_viewports)?
        {
            for head_index in self.head_indices(output) {
                let head = &self.heads[head_index];
                let Some((head_x, head_y)) = crate::project_mirror_coordinates(
                    logical_x,
                    logical_y,
                    logical_size,
                    head.output.size,
                    head.mapping,
                ) else {
                    continue;
                };
                let cursor_x = head_x.saturating_sub(hotspot_x);
                let cursor_y = head_y.saturating_sub(hotspot_y);
                head_positions.insert(head_index, (cursor_x, cursor_y));
                targets
                    .entry(head.group)
                    .or_default()
                    .push((head.selection, cursor_x, cursor_y));
            }
        }
        if self.cursor_path == crate::HardwareCursorPath::AtomicPlane {
            return self.commit_atomic_cursor(&head_positions);
        }

        let mut visible = false;
        for (group_index, group) in self.groups.iter_mut().enumerate() {
            let group_targets = targets.get(&group_index).map_or(&[][..], Vec::as_slice);
            match group.session.update_classic_hardware_cursors(group_targets) {
                Ok(crate::ClassicHardwareCursorUpdate::Visible) => visible = true,
                Ok(crate::ClassicHardwareCursorUpdate::Hidden) => {}
                Ok(crate::ClassicHardwareCursorUpdate::Queued) => {
                    return Err("legacy cursor update entered the atomic queue".into());
                }
                Ok(crate::ClassicHardwareCursorUpdate::Deferred) => {
                    self.max_cursor_update = self.max_cursor_update.max(update_started.elapsed());
                    self.cursor_update_failures = self.cursor_update_failures.saturating_add(1);
                    return Err("initialized legacy cursor update was unexpectedly deferred".into());
                }
                Err(error) => {
                    self.max_cursor_update = self.max_cursor_update.max(update_started.elapsed());
                    self.cursor_update_failures = self.cursor_update_failures.saturating_add(1);
                    return Err(format!("hardware cursor update failed: {error}").into());
                }
            }
        }
        self.max_cursor_update = self.max_cursor_update.max(update_started.elapsed());
        if primary_in_flight {
            self.legacy_cursor_updates_primary_in_flight = self
                .legacy_cursor_updates_primary_in_flight
                .saturating_add(1);
        }
        if visible {
            self.cursor_updates = self.cursor_updates.saturating_add(1);
            Ok(crate::ClassicHardwareCursorUpdate::Visible)
        } else {
            self.cursor_hidden_updates = self.cursor_hidden_updates.saturating_add(1);
            Ok(crate::ClassicHardwareCursorUpdate::Hidden)
        }
    }
}

impl LiveProductionNativeScanout {
    /// The cursor this head's next primary commit should carry, if any.
    ///
    /// `None` when the session is not driving the atomic path, when nothing
    /// is pending, or when the head has no usable cursor plane -- all of
    /// which mean the frame commits exactly as it always did. The placement
    /// is returned beside the request so the caller can settle the cells
    /// with the value it actually armed, not whatever is pending by the time
    /// the submit report comes back.
    pub(crate) fn arm_cursor_ride(
        &mut self,
        index: usize,
    ) -> Option<(
        crate::LibdrmNativeAtomicCursor,
        Option<crate::LibdrmNativeCursorPlacement>,
    )> {
        if self.cursor_path != crate::HardwareCursorPath::AtomicPlane {
            return None;
        }
        let placement = self.heads[index].pending_cursor?;
        let admission = crate::hardware_cursor_admission(
            crate::HardwareCursorPath::AtomicPlane,
            true,
            self.heads[index].submitted_at.is_some()
                || self.heads[index].scanout_custody.submitted().is_some(),
        );
        match crate::plan_cursor_commit(
            crate::HardwareCursorPath::AtomicPlane,
            admission,
            self.heads[index].pending_cursor,
            self.heads[index].committed_cursor,
            true,
            // A frame is going out, which the planner answers before it looks
            // at quietness, so this head's client cannot be quiet anyway.
            false,
        ) {
            crate::CursorCommitPlan::RideNextPrimary => {}
            crate::CursorCommitPlan::Idle => {
                self.heads[index].pending_cursor = None;
                self.heads[index].pending_cursor_since = None;
                return None;
            }
            crate::CursorCommitPlan::CommitCursorOnly | crate::CursorCommitPlan::Wait => {
                return None;
            }
        }
        let plane = self.heads[index].selection.cursor_plane()?;
        if self.heads[index].cursor_properties.is_none() {
            self.heads[index].cursor_properties = crate::discover_cursor_plane_properties(
                self.groups[self.heads[index].group]
                    .session
                    .cursor_commit_device(),
                plane,
            );
        }
        let properties = self.heads[index].cursor_properties?;
        Some((
            crate::LibdrmNativeAtomicCursor {
                plane,
                properties,
                placement,
            },
            placement,
        ))
    }

    fn queue_atomic_cursor(
        &mut self,
        index: usize,
        placement: Option<crate::LibdrmNativeCursorPlacement>,
    ) {
        if placement == self.heads[index].committed_cursor {
            self.heads[index].pending_cursor = None;
            self.heads[index].pending_cursor_since = None;
            return;
        }
        if self.heads[index]
            .pending_cursor
            .is_some_and(|pending| pending != placement)
        {
            self.cursor_updates_coalesced = self.cursor_updates_coalesced.saturating_add(1);
        }
        if self.heads[index].pending_cursor.is_none() {
            self.heads[index].pending_cursor_since = Some(Instant::now());
        }
        self.heads[index].pending_cursor = Some(placement);
    }

    pub(crate) fn settle_atomic_cursor(
        &mut self,
        index: usize,
        placement: Option<crate::LibdrmNativeCursorPlacement>,
        rode_primary: bool,
    ) {
        self.heads[index].committed_cursor = placement;
        self.heads[index].pending_cursor =
            crate::settle_pending_cursor(self.heads[index].pending_cursor, placement);
        if self.heads[index].pending_cursor.is_none()
            && let Some(started) = self.heads[index].pending_cursor_since.take()
        {
            self.max_cursor_queue_delay = self.max_cursor_queue_delay.max(started.elapsed());
        }
        self.cursor_updates = self.cursor_updates.saturating_add(1);
        if rode_primary {
            self.cursor_updates_ridden = self.cursor_updates_ridden.saturating_add(1);
        } else {
            self.cursor_only_commits = self.cursor_only_commits.saturating_add(1);
        }
    }

    pub fn pending_atomic_cursor_count(&self) -> usize {
        self.heads
            .iter()
            .filter(|head| head.pending_cursor.is_some())
            .count()
    }

    /// Service the latest desired cursor after page-flip retirement.
    ///
    /// `WouldBlock` leaves the single pending cell intact. A hard rejection
    /// changes the whole topology to the proven legacy ioctl and applies the
    /// newest desired position there; repeatedly trying an unsupported atomic
    /// request would otherwise turn pointer motion into an unbounded hot loop.
    pub(crate) fn service_pending_atomic_cursors(
        &mut self,
        output: OutputId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.cursor_path != crate::HardwareCursorPath::AtomicPlane {
            return Ok(());
        }
        for index in self.head_indices(output) {
            let Some(placement) = self.heads[index].pending_cursor else {
                continue;
            };
            if self.heads[index].prepared_scanout.is_some()
                || self.exporters[index].pending_frame()
                || self.exporters[index].worker_in_flight()
            {
                continue;
            }
            let admission = crate::hardware_cursor_admission(
                crate::HardwareCursorPath::AtomicPlane,
                true,
                self.heads[index].submitted_at.is_some()
                    || self.heads[index].scanout_custody.submitted().is_some(),
            );
            // The client's own pacing decides whether this head can afford a
            // blocking commit. `presented_page_flip_ust_usec` and this clock
            // are the same monotonic microseconds, which is why the page-flip
            // path already stamps retirements with it.
            let client_quiet = crate::cursor_only_quiet(
                self.heads[index].presented_page_flip_ust_usec,
                Self::monotonic_ust_usec(),
                self.heads[index].refresh_millihz,
            );
            match crate::plan_cursor_commit(
                crate::HardwareCursorPath::AtomicPlane,
                admission,
                self.heads[index].pending_cursor,
                self.heads[index].committed_cursor,
                false,
                client_quiet,
            ) {
                crate::CursorCommitPlan::Idle => {
                    self.heads[index].pending_cursor = None;
                    self.heads[index].pending_cursor_since = None;
                    continue;
                }
                crate::CursorCommitPlan::Wait => continue,
                crate::CursorCommitPlan::CommitCursorOnly => {}
                crate::CursorCommitPlan::RideNextPrimary => {
                    return Err("cursor-only service planned a primary ride".into());
                }
            }
            let plane = self.heads[index]
                .selection
                .cursor_plane()
                .ok_or("atomic cursor head lost its selected plane")?;
            let properties = self.heads[index]
                .cursor_properties
                .ok_or("atomic cursor head lost its property set")?;
            let request = crate::build_native_cursor_only_atomic_request(
                plane,
                self.heads[index].selection.crtc_handle(),
                properties,
                placement,
            );
            let group = self.heads[index].group;
            // This blocks until the kernel applies it. Measured rather than
            // argued: the gate above is only worth its complexity if what it
            // still admits costs approximately nothing.
            let commit_started = Instant::now();
            let status = crate::submit_native_cursor_only_commit(
                self.groups[group].session.cursor_commit_device(),
                request,
            );
            let commit_elapsed = commit_started.elapsed();
            self.max_cursor_only_commit = self.max_cursor_only_commit.max(commit_elapsed);
            self.cursor_only_commit_total =
                self.cursor_only_commit_total.saturating_add(commit_elapsed);
            match status {
                crate::LibdrmNativeAtomicCommitSubmitStatus::Submitted => {
                    self.settle_atomic_cursor(index, placement, false);
                }
                crate::LibdrmNativeAtomicCommitSubmitStatus::WouldBlock => {}
                crate::LibdrmNativeAtomicCommitSubmitStatus::Rejected => {
                    self.cursor_update_failures = self.cursor_update_failures.saturating_add(1);
                    self.fallback_pending_atomic_cursor_to_legacy()?;
                    break;
                }
            }
        }
        Ok(())
    }

    /// Commit cursor-only work only after the visual runtime has drained and
    /// routed primary-frame feedback for this service cycle.
    ///
    /// Input merely replaces the desired-position cell. Centralizing the DRM
    /// side effect here keeps pointer traffic from entering a blocking atomic
    /// ioctl ahead of ready Present completions or primary submissions.
    pub(crate) fn service_idle_atomic_cursors(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let outputs = self
            .heads
            .iter()
            .map(|head| head.output.id)
            .collect::<BTreeSet<_>>();
        for output in outputs {
            self.service_pending_atomic_cursors(output)?;
        }
        Ok(())
    }

    fn fallback_pending_atomic_cursor_to_legacy(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fallback_started = Instant::now();
        self.cursor_path = crate::HardwareCursorPath::LegacyIoctl;
        self.cursor_legacy_fallbacks = self.cursor_legacy_fallbacks.saturating_add(1);
        for group_index in 0..self.groups.len() {
            let targets = self
                .heads
                .iter()
                .filter(|head| head.group == group_index)
                .filter_map(|head| {
                    head.pending_cursor
                        .unwrap_or(head.committed_cursor)
                        .map(|placement| (head.selection, placement.x, placement.y))
                })
                .collect::<Vec<_>>();
            self.groups[group_index]
                .session
                .update_classic_hardware_cursors(&targets)
                .map_err(|error| format!("legacy cursor fallback failed: {error}"))?;
        }
        for head in &mut self.heads {
            if let Some(started) = head.pending_cursor_since.take() {
                self.max_cursor_queue_delay = self.max_cursor_queue_delay.max(started.elapsed());
            }
            head.committed_cursor = head.pending_cursor.unwrap_or(head.committed_cursor);
            head.pending_cursor = None;
        }
        self.cursor_updates = self.cursor_updates.saturating_add(1);
        self.max_cursor_update = self.max_cursor_update.max(fallback_started.elapsed());
        tracing::warn!(
            "sophia_live_cursor_path schema=3 status=fallback reason=atomic_cursor_rejected path=legacy_ioctl"
        );
        Ok(())
    }

    /// Put the cursor where it belongs on every head, atomically.
    ///
    /// One decision per head, taken by `plan_cursor_commit` so the rule lives
    /// in one testable place rather than in this loop. A head the pointer is
    /// not on is told to hide, in its own commit -- the frame path commits
    /// per head, so the model's single group request is not available here
    /// and heads agree across commits rather than within one.
    fn commit_atomic_cursor(
        &mut self,
        head_positions: &BTreeMap<usize, (i32, i32)>,
    ) -> Result<crate::ClassicHardwareCursorUpdate, Box<dyn std::error::Error>> {
        let update_started = Instant::now();
        let mut visible = false;
        for index in 0..self.heads.len() {
            let Some((framebuffer, width, height)) = self.groups[self.heads[index].group]
                .session
                .atomic_cursor_resources()
            else {
                continue;
            };
            let placement =
                head_positions
                    .get(&index)
                    .map(|(x, y)| crate::LibdrmNativeCursorPlacement {
                        framebuffer,
                        x: *x,
                        y: *y,
                        width,
                        height,
                    });
            visible |= placement.is_some();
            self.queue_atomic_cursor(index, placement);
        }
        // The visual runtime services this cell after it has routed ready
        // presentation feedback and submitted any primary work. That gives a
        // frame the first chance to carry the cursor for free and prevents a
        // blocking cursor-only ioctl from delaying X Present's display clock.
        self.max_cursor_update = self.max_cursor_update.max(update_started.elapsed());
        if self.pending_atomic_cursor_count() > 0 {
            self.cursor_updates_queued = self.cursor_updates_queued.saturating_add(1);
            return Ok(crate::ClassicHardwareCursorUpdate::Queued);
        }
        if visible {
            Ok(crate::ClassicHardwareCursorUpdate::Visible)
        } else {
            self.cursor_hidden_updates = self.cursor_hidden_updates.saturating_add(1);
            Ok(crate::ClassicHardwareCursorUpdate::Hidden)
        }
    }
}

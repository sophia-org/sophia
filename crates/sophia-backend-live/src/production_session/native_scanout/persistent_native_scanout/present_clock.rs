use super::*;
use ::drm::Device;

impl LiveProductionNativeHead {
    /// Bound estimated wakes by the fastest selected-mode field, including
    /// when VRR is enabled. The idle observed period can be much longer.
    /// Synthetic selections without a mode retain their admitted nominal rate.
    pub fn present_clock_minimum_period(&self) -> Duration {
        let nominal = super::super::refresh::head_refresh_interval(self.refresh_millihz);
        self.selection
            .mode()
            .and_then(|mode| {
                super::super::refresh::mode_field_interval(
                    mode.clock(),
                    mode.hsync().2,
                    mode.vsync().2,
                    mode.flags().contains(::drm::control::ModeFlags::INTERLACE),
                )
            })
            .map_or(nominal, |exact| nominal.min(exact))
    }
}

impl LiveProductionNativeScanout {
    pub(super) fn clock_source_for_submission(
        &self,
        index: usize,
    ) -> Option<crate::LiveNativePresentClockSource> {
        let head = &self.heads[index];
        self.present_clocks
            .source_for(crate::LiveNativePresentClockKey {
                head: head.head,
                target_generation: head.target_generation,
                card_group: head.group,
                crtc_id: head.selection.crtc_id(),
            })
    }

    /// Source choice for a NEW request. Only the chosen member supplies its
    /// MSC. Logical mirror permission still belongs to the primary; cleanup
    /// remains per head. Never use this to rebind queued work.
    #[must_use]
    pub fn query_present_clock_for_output(
        &mut self,
        output: OutputId,
    ) -> Vec<(
        sophia_engine::RenderHeadId,
        crate::LiveNativePresentClockObservation,
    )> {
        let primary = self
            .output_lifecycles
            .get(&output)
            .map(|group| group.primary_head());
        let heads = self
            .heads
            .iter()
            .filter(|head| head.output.id == output)
            .map(|head| (head.head, head.enabled))
            .collect::<Vec<_>>();
        crate::query_live_present_clock_candidates(heads, primary, |head| {
            self.query_present_clock(head)
        })
    }

    /// Active heads that scan out without a counter, each under a stable
    /// source of its own. Like the clocked set, a source missing from here
    /// after invalidation, disable or resume is retired.
    pub fn present_unclocked_heads(
        &self,
    ) -> impl Iterator<
        Item = (
            sophia_engine::RenderHeadId,
            crate::LiveNativeUnclockedPresentClock,
        ),
    > + '_ {
        self.present_clocks.unclocked()
    }

    /// Observed counter owners only. The frontend retains old queued bindings
    /// and retires a binding missing from this set after invalidation/resume.
    pub fn present_clock_heads(
        &self,
    ) -> impl Iterator<
        Item = (
            sophia_engine::RenderHeadId,
            crate::LiveNativePresentClockSource,
        ),
    > + '_ {
        self.present_clocks.sources()
    }

    pub fn present_clock_sample(
        &self,
        source: crate::LiveNativePresentClockSource,
    ) -> Option<crate::LiveNativePresentClockSample> {
        self.present_clocks.last_sample(source)
    }

    /// Called before modeset preparation, even if it later rolls back to the
    /// previous target without a query in between. Seat resume constructs a
    /// new native owner; all its sources have a different owner domain.
    pub fn invalidate_present_clocks(&mut self) {
        drop(self.present_clocks.invalidate());
    }

    /// Invoke only for a request choosing its source or a queued obligation
    /// on this exact head. GET_SEQUENCE takes a kernel vblank reference, so
    /// ordinary idle service must not call this. No event is consumed and
    /// no future sequence is predicted from the head's nominal refresh rate.
    pub fn query_present_clock(
        &mut self,
        head: sophia_engine::RenderHeadId,
    ) -> crate::LiveNativePresentClockObservation {
        use crate::LiveNativePresentClockStatus::*;
        let Some(index) = self.head_index_for_head(head) else {
            return self.present_clocks.lose_head(head, InvalidTarget);
        };
        let target = &self.heads[index];
        if !target.enabled || self.output_topology_preparation.is_some() {
            return self.present_clocks.lose_head(head, Inactive);
        }
        let key = crate::LiveNativePresentClockKey {
            head,
            target_generation: target.target_generation,
            card_group: target.group,
            crtc_id: target.selection.crtc_id(),
        };
        // A definite unclocked answer holds for this target lifetime and is
        // repeated without another kernel query or vblank reference.
        if let Some(cached) = self.present_clocks.unclocked_for(key) {
            return cached;
        }
        let minimum_period = target.present_clock_minimum_period();
        let card = self.groups[target.group].session.card();
        let Some(monotonic) = crate::cached_monotonic_capability(
            &mut self.present_clock_monotonic,
            target.group,
            || card.get_driver_capability(::drm::DriverCapability::MonotonicTimestamp),
        ) else {
            return self.present_clocks.lose_head(head, QueryFailed);
        };
        // Without monotonic timestamps no UST can be trusted, so the head is
        // neither clocked nor unclocked. The answer is already cached per
        // card fd above; nothing is minted for it.
        if !monotonic {
            return self.present_clocks.lose_head(head, UnsupportedClock);
        }
        // Raw flip observations are optional. Without an explicit CRTC id
        // they cannot safely borrow drm-rs's user_data fallback for a
        // multi-CRTC atomic commit. Keep the GET_SEQUENCE path available.
        if !self.present_clock_crtc_events.contains_key(&target.group)
            && let Ok(value) =
                card.get_driver_capability(::drm::DriverCapability::CRTCInVBlankEvent)
        {
            self.present_clock_crtc_events
                .insert(target.group, value != 0);
        }
        match sophia_drm_clock::query(card, key.crtc_id) {
            Ok(sample) => self.present_clocks.observe(key, sample),
            Err(error) => match crate::unsupported_sequence_errno(&error) {
                Some(errno) => self.present_clocks.observe_unsupported(
                    key,
                    crate::LiveNativeUnclockedReason::SequenceUnsupported { errno },
                    minimum_period,
                ),
                // EINVAL included: the kernel also gives it while a CRTC's
                // vblank is off, so it is asked again.
                None => self.present_clocks.lose_head(head, QueryFailed),
            },
        }
    }
}

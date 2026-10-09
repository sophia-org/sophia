/// Who a quarantine belongs to.
///
/// The phase alone was ambiguous, and two independent writers shared it: a DRM
/// rescan and an authority-driven policy candidate. Whichever finished first
/// released the other's quarantine, so a policy candidate could reach its
/// rebuild to find the owner already `Stable`. Naming the holder makes that a
/// typed refusal instead of silent theft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveOutputTopologyQuarantine {
    /// A DRM hotplug notice, or a retry of one.
    Hotplug,
    /// An output file candidate between admission and settlement.
    Policy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveOutputTopologyPhase {
    Stable,
    Quarantined(LiveOutputTopologyQuarantine),
    Rebuilt,
    Published,
    AwaitingPresentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveOutputTopologyRebuild {
    Unavailable,
    TransportReplaced,
    TopologyChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveOutputTopologyOwner {
    phase: LiveOutputTopologyPhase,
    transition: u64,
    notice_sequence: u64,
    topology_epoch: u64,
    publication_generation: u64,
    outputs: Vec<sophia_engine::HeadlessOutput>,
    /// How many connectors drive each logical output.
    ///
    /// Held beside the output list because losing one head of a mirror group
    /// leaves that list identical: the logical output survives, one of its screens
    /// goes dark, and a comparison on outputs alone calls it no change at all.
    heads: Vec<(sophia_protocol::OutputId, usize)>,
    policy_committed: bool,
    policy_settlement_pending: bool,
    presentation_baseline: usize,
    /// Input may resume after a timeout, but a late flip still owes the
    /// matching hardware publication one observation.
    presentation_pending: bool,
    /// A hotplug notice that arrived under a policy quarantine and still owes
    /// a rescan once the owner settles.
    deferred_hotplug_notice: bool,
}

impl LiveOutputTopologyOwner {
    fn new_at_generation(
        outputs: Vec<sophia_engine::HeadlessOutput>,
        heads: Vec<(sophia_protocol::OutputId, usize)>,
        publication_generation: u64,
    ) -> Result<Self, &'static str> {
        if outputs.is_empty() {
            return Err("live topology owner requires an initial output");
        }
        if publication_generation == 0 {
            return Err("live topology owner requires a valid publication generation");
        }
        Ok(Self {
            phase: LiveOutputTopologyPhase::Stable,
            transition: 0,
            notice_sequence: 0,
            topology_epoch: 1,
            publication_generation,
            outputs,
            heads,
            policy_committed: true,
            policy_settlement_pending: false,
            presentation_baseline: 0,
            presentation_pending: false,
            deferred_hotplug_notice: false,
        })
    }

    /// Returns true only when the caller must advance the application-input
    /// security epoch. Retries while one transition is quarantined retain the
    /// already-advanced epoch.
    fn begin_rescan(&mut self, notice_sequence: u64) -> Result<bool, &'static str> {
        if notice_sequence == 0 || notice_sequence < self.notice_sequence {
            return Err("live topology notice is invalid or stale");
        }
        if notice_sequence == self.notice_sequence {
            return Ok(false);
        }
        self.notice_sequence = notice_sequence;
        self.presentation_pending = false;
        if let LiveOutputTopologyPhase::Quarantined(holder) = self.phase {
            // A notice arriving under a policy candidate is remembered rather
            // than serviced, because servicing it here would consume the
            // candidate's quarantine. It is re-armed when the owner settles.
            if holder == LiveOutputTopologyQuarantine::Policy {
                self.deferred_hotplug_notice = true;
            }
            return Ok(false);
        }
        if self.phase != LiveOutputTopologyPhase::Stable {
            self.transition = self
                .transition
                .checked_add(1)
                .ok_or("live topology transition identity exhausted")?;
            self.phase =
                LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug);
            return Ok(false);
        }
        self.transition = self
            .transition
            .checked_add(1)
            .ok_or("live topology transition identity exhausted")?;
        self.phase = LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug);
        Ok(true)
    }

    /// Whether a hotplug notice arrived while a policy candidate held the
    /// quarantine, and so still owes a rescan.
    const fn hotplug_notice_deferred(&self) -> bool {
        self.deferred_hotplug_notice && matches!(self.phase, LiveOutputTopologyPhase::Stable)
    }

    fn take_deferred_hotplug_notice(&mut self) -> bool {
        if !self.hotplug_notice_deferred() {
            return false;
        }
        self.deferred_hotplug_notice = false;
        true
    }

    /// Quarantines input for an authority-driven topology transaction without
    /// pretending that a DRM hotplug notice occurred. The replacement remains
    /// private until `observe_rebuild` and `mark_published` run after its
    /// physical first-presentation barrier.
    fn begin_policy_change(&mut self) -> Result<bool, &'static str> {
        if self.phase != LiveOutputTopologyPhase::Stable {
            return Err("live policy topology change overlaps another transition");
        }
        self.transition = self
            .transition
            .checked_add(1)
            .ok_or("live topology transition identity exhausted")?;
        self.phase = LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy);
        Ok(true)
    }

    /// Restores the published topology after a policy candidate was rejected
    /// or physically rolled back. No published fields were mutated while the
    /// owner was quarantined, so cancellation is an explicit phase transition.
    fn cancel_policy_change(&mut self) -> Result<(), &'static str> {
        if self.phase != LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
        {
            return Err("live policy topology cancellation is out of order");
        }
        self.phase = LiveOutputTopologyPhase::Stable;
        Ok(())
    }

    /// Records the monotonic X/RandR generation consumed while reverting a
    /// candidate that reached the frontend but never became Engine authority.
    /// The logical topology epoch and published output set remain unchanged.
    fn observe_policy_transport_rollback(
        &mut self,
        publication_generation: u64,
    ) -> Result<(), &'static str> {
        if self.phase != LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
            || publication_generation <= self.publication_generation
        {
            return Err("live policy topology transport rollback is out of order");
        }
        self.publication_generation = publication_generation;
        Ok(())
    }

    fn observe_rebuild(
        &mut self,
        outputs: Vec<sophia_engine::HeadlessOutput>,
        heads: Vec<(sophia_protocol::OutputId, usize)>,
    ) -> Result<LiveOutputTopologyRebuild, &'static str> {
        self.observe_resolved_rebuild(outputs, heads, false)
    }

    fn observe_resolved_rebuild(
        &mut self,
        outputs: Vec<sophia_engine::HeadlessOutput>,
        heads: Vec<(sophia_protocol::OutputId, usize)>,
        realization_changed: bool,
    ) -> Result<LiveOutputTopologyRebuild, &'static str> {
        if self.phase != LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug)
        {
            return Err("live topology rebuild was observed outside a hotplug quarantine");
        }
        if outputs.is_empty() {
            return Ok(LiveOutputTopologyRebuild::Unavailable);
        }
        // A surviving-head topology is a new candidate. Comparing the head counts
        // as well as the outputs is what makes that true here rather than only in
        // the model that requires it.
        let changed = realization_changed || outputs != self.outputs || heads != self.heads;
        if changed {
            self.topology_epoch = self
                .topology_epoch
                .checked_add(1)
                .ok_or("live topology epoch exhausted")?;
            self.publication_generation = self
                .publication_generation
                .checked_add(1)
                .ok_or("live output publication generation exhausted")?;
            self.outputs = outputs;
            self.heads = heads;
        }
        self.phase = LiveOutputTopologyPhase::Rebuilt;
        Ok(if changed {
            LiveOutputTopologyRebuild::TopologyChanged
        } else {
            LiveOutputTopologyRebuild::TransportReplaced
        })
    }

    /// Installs an authority-approved policy candidate after its physical
    /// first-presentation barrier. Unlike hotplug reduction, this always
    /// advances the topology epoch: mode timing, transform, mapping, and VRR
    /// changes may preserve both logical extents and mirror member counts.
    fn observe_policy_rebuild(
        &mut self,
        outputs: Vec<sophia_engine::HeadlessOutput>,
        heads: Vec<(sophia_protocol::OutputId, usize)>,
        candidate_topology_epoch: u64,
    ) -> Result<(), &'static str> {
        if self.phase != LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
            || outputs.is_empty()
            || candidate_topology_epoch != self.topology_epoch.checked_add(1).unwrap_or(0)
        {
            return Err("live policy topology rebuild is invalid or out of order");
        }
        self.topology_epoch = candidate_topology_epoch;
        self.publication_generation = self
            .publication_generation
            .checked_add(1)
            .ok_or("live output publication generation exhausted")?;
        self.outputs = outputs;
        self.heads = heads;
        self.phase = LiveOutputTopologyPhase::Rebuilt;
        Ok(())
    }

    fn mark_published(
        &mut self,
        presentation_baseline: usize,
        policy_required: bool,
    ) -> Result<(), &'static str> {
        if self.phase != LiveOutputTopologyPhase::Rebuilt {
            return Err("live topology publication is out of order");
        }
        self.presentation_baseline = presentation_baseline;
        self.presentation_pending = true;
        self.policy_settlement_pending |= policy_required;
        self.policy_committed = !self.policy_settlement_pending;
        self.phase = if self.policy_committed {
            LiveOutputTopologyPhase::AwaitingPresentation
        } else {
            LiveOutputTopologyPhase::Published
        };
        Ok(())
    }

    fn mark_policy_committed(&mut self, presentation_baseline: usize) -> Result<(), &'static str> {
        if self.phase != LiveOutputTopologyPhase::Published {
            return Err("live topology policy settlement is out of order");
        }
        self.policy_committed = true;
        self.policy_settlement_pending = false;
        // A presentation which retired before the policy projection committed
        // cannot release the input quarantine.
        self.presentation_baseline = presentation_baseline;
        self.phase = LiveOutputTopologyPhase::AwaitingPresentation;
        Ok(())
    }

    /// Releases the post-commit presentation wait without the flip it wanted.
    ///
    /// The wait exists so input does not resume onto stale window positions:
    /// the topology itself is already on screen from first presentation, and
    /// what it additionally waits for is the *policy layout* being presented.
    /// Nothing forces that frame, though. A relayout that moves nothing
    /// produces no damage and therefore no flip, which is indistinguishable
    /// from a client that is merely slow -- and in the first case the displayed
    /// layout is already the committed one, so waiting forever protects
    /// nothing while holding input at shortcuts-only.
    ///
    /// Callers must have waited a bounded time first and must say why.
    fn release_presentation_wait(&mut self) -> bool {
        if self.phase != LiveOutputTopologyPhase::AwaitingPresentation || !self.policy_committed {
            return false;
        }
        self.phase = LiveOutputTopologyPhase::Stable;
        true
    }

    fn observe_presentation(&mut self, retirements: usize) -> bool {
        if !matches!(
            self.phase,
            LiveOutputTopologyPhase::AwaitingPresentation | LiveOutputTopologyPhase::Stable
        ) || !self.presentation_pending
            || !self.policy_committed
            || retirements <= self.presentation_baseline
        {
            return false;
        }
        self.phase = LiveOutputTopologyPhase::Stable;
        self.presentation_pending = false;
        true
    }

    const fn input_quarantined(&self) -> bool {
        !matches!(self.phase, LiveOutputTopologyPhase::Stable)
    }

    /// A cover observed on installed heads may be named by the published
    /// epoch only after both the installation and publication have settled.
    fn settled_coverage_epoch(
        &self,
        published_epoch: Option<u64>,
        publication_pending: bool,
        candidate_active: bool,
    ) -> Option<u64> {
        published_epoch.filter(|epoch| {
            !self.input_quarantined()
                && !publication_pending
                && !candidate_active
                && *epoch == self.topology_epoch
        })
    }
}

impl<Enabled, Disabled> LiveProductionNativeTopologyResourceCohort<Enabled, Disabled> {
    pub fn new(plan: &LiveProductionNativeTopologyPlan) -> Option<Self> {
        let expected = plan
            .heads
            .iter()
            .map(|head| {
                (
                    head.head,
                    (head.card_index, head.disposition, head.previous_enabled),
                )
            })
            .collect::<BTreeMap<_, _>>();
        (expected.len() == plan.heads.len() && !expected.is_empty()).then_some(Self {
            expected,
            candidate: BTreeMap::new(),
            rollback: BTreeMap::new(),
        })
    }

    pub fn prepare_candidate_enabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Enabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Enabled>,
    > {
        let Some((_, disposition, _)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if !matches!(
            disposition,
            LiveProductionNativeTopologyDisposition::Enabled { .. }
        ) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.candidate.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.candidate.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Enabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn prepare_candidate_disabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Disabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Disabled>,
    > {
        let Some((_, disposition, _)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if !matches!(
            disposition,
            LiveProductionNativeTopologyDisposition::Disabled
        ) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.candidate.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.candidate.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Disabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn prepare_rollback(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Enabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Enabled>,
    > {
        self.prepare_rollback_enabled(head, owner)
    }

    pub fn prepare_rollback_enabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Enabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Enabled>,
    > {
        let Some((_, _, previous_enabled)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if !previous_enabled {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.rollback.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.rollback.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Enabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn prepare_rollback_disabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Disabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Disabled>,
    > {
        let Some((_, _, previous_enabled)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if *previous_enabled {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.rollback.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.rollback.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Disabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn ready(&self) -> bool {
        self.candidate.len() == self.expected.len() && self.rollback.len() == self.expected.len()
    }

    pub fn candidate_count(&self) -> usize {
        self.candidate.len()
    }

    pub fn rollback_count(&self) -> usize {
        self.rollback.len()
    }

    pub fn card_heads(&self, card_index: usize) -> Vec<sophia_engine::RenderHeadId> {
        self.expected
            .iter()
            .filter_map(|(head, (card, _, _))| (*card == card_index).then_some(*head))
            .collect()
    }

    pub fn candidate(
        &self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<&LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.candidate.get(&head)
    }

    pub fn rollback(
        &self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<&LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.rollback.get(&head)
    }

    pub fn take_candidate(
        &mut self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.candidate.remove(&head)
    }

    pub fn take_rollback(
        &mut self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.rollback.remove(&head)
    }

    pub fn into_remaining(self) -> CandidateResourceSplit<Enabled, Disabled> {
        (
            self.candidate.into_values().collect(),
            self.rollback.into_values().collect(),
        )
    }

    fn accepted_transition(&self) -> LiveProductionNativeTopologyResourceTransition {
        if self.ready() {
            LiveProductionNativeTopologyResourceTransition::Ready
        } else {
            LiveProductionNativeTopologyResourceTransition::Accepted
        }
    }
}

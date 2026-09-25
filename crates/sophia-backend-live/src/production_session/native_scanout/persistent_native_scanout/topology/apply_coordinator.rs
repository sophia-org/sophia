impl LiveProductionNativeTopologyApplyCoordinator {
    pub fn new(plan: &LiveProductionNativeTopologyPlan) -> Option<Self> {
        let mut by_card = BTreeMap::<usize, Vec<sophia_engine::RenderHeadId>>::new();
        for head in &plan.heads {
            by_card.entry(head.card_index).or_default().push(head.head);
        }
        if by_card.is_empty() {
            return None;
        }
        let cards = by_card
            .into_iter()
            .map(|(card_index, mut heads)| {
                heads.sort();
                heads.dedup();
                LiveProductionNativeTopologyCard { card_index, heads }
            })
            .collect::<Vec<_>>();
        if cards.iter().any(|card| card.heads.is_empty())
            || cards.iter().map(|card| card.heads.len()).sum::<usize>() != plan.heads.len()
        {
            return None;
        }
        Some(Self {
            cards,
            phase: LiveProductionNativeTopologyApplyPhase::Prepared,
            next_apply: 0,
            applied: 0,
            rollback_remaining: 0,
        })
    }

    pub const fn phase(&self) -> LiveProductionNativeTopologyApplyPhase {
        self.phase
    }

    pub fn current_card_index(&self) -> Option<usize> {
        match self.phase {
            LiveProductionNativeTopologyApplyPhase::Applying => {
                self.cards.get(self.next_apply).map(|card| card.card_index)
            }
            LiveProductionNativeTopologyApplyPhase::RollingBack => self
                .rollback_remaining
                .checked_sub(1)
                .and_then(|index| self.cards.get(index))
                .map(|card| card.card_index),
            _ => None,
        }
    }

    pub fn current_heads(&self) -> &[sophia_engine::RenderHeadId] {
        match self.phase {
            LiveProductionNativeTopologyApplyPhase::Applying => self
                .cards
                .get(self.next_apply)
                .map_or(&[], |card| card.heads.as_slice()),
            LiveProductionNativeTopologyApplyPhase::RollingBack => self
                .rollback_remaining
                .checked_sub(1)
                .and_then(|index| self.cards.get(index))
                .map_or(&[], |card| card.heads.as_slice()),
            _ => &[],
        }
    }

    pub fn begin_apply(&mut self) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Prepared {
            return self.out_of_order();
        }
        self.phase = LiveProductionNativeTopologyApplyPhase::Applying;
        LiveProductionNativeTopologyApplyTransition::Accepted
    }

    /// Starts a full reverse-card rollback after every candidate card applied.
    ///
    /// Runtime reconstruction and first presentation remain fallible after the
    /// blocking modesets succeed, so a terminal `Applied` coordinator must
    /// retain a route back to the published topology.
    pub fn begin_rollback_after_apply(&mut self) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Applied
            || self.applied != self.cards.len()
        {
            return self.out_of_order();
        }
        self.phase = LiveProductionNativeTopologyApplyPhase::RollingBack;
        self.rollback_remaining = self.applied;
        LiveProductionNativeTopologyApplyTransition::Accepted
    }

    pub fn begin_rollback_after_partial_apply(
        &mut self,
    ) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Applying
            || self.applied == 0
            || self.applied >= self.cards.len()
        {
            return self.out_of_order();
        }
        self.phase = LiveProductionNativeTopologyApplyPhase::RollingBack;
        self.rollback_remaining = self.applied;
        LiveProductionNativeTopologyApplyTransition::Accepted
    }

    pub fn observe_apply(
        &mut self,
        card_index: usize,
        outcome: crate::NativeTopologySubmitOutcome,
    ) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Applying
            || self.current_card_index() != Some(card_index)
        {
            return self.out_of_order();
        }
        if outcome == crate::NativeTopologySubmitOutcome::Busy {
            return LiveProductionNativeTopologyApplyTransition::Retry;
        }
        if outcome != crate::NativeTopologySubmitOutcome::Accepted {
            if self.applied == 0 {
                self.phase = LiveProductionNativeTopologyApplyPhase::Failed;
                return LiveProductionNativeTopologyApplyTransition::FailedWithoutMutation {
                    card_index,
                };
            }
            self.phase = LiveProductionNativeTopologyApplyPhase::RollingBack;
            self.rollback_remaining = self.applied;
            return LiveProductionNativeTopologyApplyTransition::RollbackRequired {
                failed_card_index: card_index,
            };
        }

        let card = &self.cards[self.next_apply];
        let transition = if self.next_apply + 1 == self.cards.len() {
            self.phase = LiveProductionNativeTopologyApplyPhase::Applied;
            LiveProductionNativeTopologyApplyTransition::Applied {
                card_index,
                heads: card.heads.clone(),
            }
        } else {
            LiveProductionNativeTopologyApplyTransition::CardApplied {
                card_index,
                heads: card.heads.clone(),
            }
        };
        self.next_apply += 1;
        self.applied += 1;
        transition
    }

    pub fn observe_rollback(
        &mut self,
        card_index: usize,
        outcome: crate::NativeTopologySubmitOutcome,
    ) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::RollingBack
            || self.current_card_index() != Some(card_index)
        {
            return self.out_of_order();
        }
        if outcome == crate::NativeTopologySubmitOutcome::Busy {
            return LiveProductionNativeTopologyApplyTransition::Retry;
        }
        if outcome != crate::NativeTopologySubmitOutcome::Accepted {
            self.phase = LiveProductionNativeTopologyApplyPhase::Failed;
            return LiveProductionNativeTopologyApplyTransition::RollbackFailed { card_index };
        }
        let card = &self.cards[self.rollback_remaining - 1];
        self.rollback_remaining -= 1;
        if self.rollback_remaining == 0 {
            self.phase = LiveProductionNativeTopologyApplyPhase::RolledBack;
            LiveProductionNativeTopologyApplyTransition::RolledBack {
                card_index,
                heads: card.heads.clone(),
            }
        } else {
            LiveProductionNativeTopologyApplyTransition::CardRolledBack {
                card_index,
                heads: card.heads.clone(),
            }
        }
    }

    fn out_of_order(&self) -> LiveProductionNativeTopologyApplyTransition {
        if matches!(
            self.phase,
            LiveProductionNativeTopologyApplyPhase::Applied
                | LiveProductionNativeTopologyApplyPhase::RolledBack
                | LiveProductionNativeTopologyApplyPhase::Failed
        ) {
            LiveProductionNativeTopologyApplyTransition::Terminal
        } else {
            LiveProductionNativeTopologyApplyTransition::OutOfOrder
        }
    }
}

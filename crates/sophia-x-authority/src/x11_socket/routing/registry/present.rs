// Presentation routing: who is subscribed, what is queued for them, and how a
// completion or an idle notification reaches them.
//
// Split by subject from the registry's client, surface and window
// bookkeeping. Both are routing, but one is about which connections exist and
// the other about the frames they are waiting on, and they change for
// different reasons.

#[derive(Clone, Copy)]
enum XPresentCompletionClock {
    Legacy { ust: u64, msc: u64 },
    Bound(crate::XPresentClockSample),
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn select_present_input(
        &self,
        client: XServerFrontendClientId,
        event_id: XResourceId,
        window: XResourceId,
        mask: u32,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut subscriptions = self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let key = (client, event_id);
        if mask == 0 {
            subscriptions.remove(&key);
        } else {
            subscriptions.insert(
                key,
                XPresentSubscription {
                    event_id,
                    window,
                    mask,
                },
            );
        }
        Ok(())
    }

    fn present_configure_subscribers(
        &self,
        window: XResourceId,
    ) -> Result<Vec<(XServerFrontendClientId, XResourceId)>, XServerFrontendRouteError> {
        Ok(self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((subscription_client, _), subscription)| {
                (subscription.window == window && subscription.mask & 1 != 0)
                    .then_some((*subscription_client, subscription.event_id))
            })
            .collect())
    }

    fn queue_present(
        &self,
        transaction: TransactionId,
        client: XServerFrontendClientId,
        window: XResourceId,
        pixmap: XResourceId,
        serial: u32,
        idle_fence: Option<XResourceId>,
        suboptimal: bool,
    ) -> Result<(), XServerFrontendRouteError> {
        // Admission precedes the runtime lock. Capacity waits must never
        // hold runtime (execution needs it to release reservations). When
        // nested elsewhere, the order is runtime -> clients -> pending;
        // each lookup here releases its guard before acquiring the next.
        // The window decides which surface a present reaches; the presenting
        // client does not have to be the one that created it. A browser's GPU
        // process presents to a window its browser process owns, which X
        // permits -- requiring creator == presenter here silently locked out
        // every client that splits the two across connections.
        if self.present_clock_owner.is_none() {
            self.surfaces
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                .iter()
                .find_map(|(surface, route)| (route.window == window).then_some(*surface))
                .ok_or(XServerFrontendRouteError::UnknownPresentWindow { window })?;
        }
        // Timed requests may name an unmapped/rootless window. Validate it
        // under runtime at preparation, after this capacity wait.
        let admission = self.clients.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&client).and_then(|entry| entry.admission);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut pending = self
            .pending_presentations
            .entries
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if pending.contains_key(&transaction) {
            return Err(XServerFrontendRouteError::DuplicatePresentation { transaction });
        }
        while pending
            .values()
            .filter(|presentation| presentation.client == client)
            .count()
            >= self.per_client_presentation_capacity.get().min(crate::X_PRESENT_PER_CLIENT_CAPACITY)
        {
            let now = Instant::now();
            if now >= deadline {
                return Err(XServerFrontendRouteError::ClientQueueFull { client });
            }
            let _ = self.pending_presentations.capacity_waits.fetch_update(
                Ordering::Relaxed, Ordering::Relaxed, |count| Some(count.saturating_add(1)));
            let (next, wait) = self
                .pending_presentations
                .capacity_changed
                .wait_timeout(pending, deadline.saturating_duration_since(now))
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            pending = next;
            if wait.timed_out()
                && pending
                    .values()
                    .filter(|presentation| presentation.client == client)
                    .count()
                    >= self.per_client_presentation_capacity.get().min(crate::X_PRESENT_PER_CLIENT_CAPACITY)
            {
                return Err(XServerFrontendRouteError::ClientQueueFull { client });
            }
        }
        pending.insert(
            transaction,
            XPendingPresent {
                client,
                admission,
                window,
                pixmap,
                serial,
                idle_fence,
                suboptimal,
                phases: crate::XPresentFeedbackPhases::default(),
                clock: None,
                allocation_subject: None,
            },
        );
        crate::evidence::present_accepted(
            client,
            transaction,
            window,
            pixmap,
            serial,
            pending.len(),
        );
        Ok(())
    }

    fn route_present_complete(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
    ) -> Result<bool, XServerFrontendRouteError> {
        self.route_present_complete_with_layout(transaction, ust, msc, mode, None)
            .map(|outcome| outcome.routed)
    }

    fn route_present_complete_with_layout(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
        comparison: Option<crate::XPresentLayoutComparison>,
    ) -> Result<crate::XPresentCompleteRouteOutcome, XServerFrontendRouteError> {
        self.route_present_complete_on_clock(transaction,
            XPresentCompletionClock::Legacy { ust, msc }, mode, comparison)
    }

    fn route_present_complete_on_clock(
        &self,
        transaction: TransactionId,
        clock: XPresentCompletionClock,
        mode: XPresentCompletionMode,
        comparison: Option<crate::XPresentLayoutComparison>,
    ) -> Result<crate::XPresentCompleteRouteOutcome, XServerFrontendRouteError> {
        // Reallocation advice is decided here from the current transaction and
        // preference state. A caller-supplied mode cannot replace that decision.
        let mode = match mode {
            XPresentCompletionMode::SuboptimalCopy => XPresentCompletionMode::Copy,
            mode => mode,
        };
        let (presentation, layout_comparison, mode, ust, msc) = {
            let authority = comparison
                .and_then(|_| self.runtime.get())
                .and_then(std::sync::Weak::upgrade);
            let mut runtime = authority.as_ref().and_then(|authority| authority.try_lock().ok());
            let mut pending = self
                .pending_presentations
                .entries
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some(presentation) = pending.get_mut(&transaction) else {
                return Ok(crate::XPresentCompleteRouteOutcome {
                    routed: false,
                    mode,
                    layout_comparison: None,
                });
            };
            let (ust, msc) = match (presentation.clock.as_ref().map(|clock| clock.binding), clock) {
                (None, XPresentCompletionClock::Legacy { ust, msc }) => (ust, msc),
                (Some(binding), XPresentCompletionClock::Bound(sample)) => binding
                    .window_sample(sample)
                    .map_err(|_| XServerFrontendRouteError::PresentClockMismatch { transaction })?,
                _ => return Err(XServerFrontendRouteError::PresentClockMismatch { transaction }),
            };
            let layout_comparison = comparison.map(|comparison| {
                let matched = mode == XPresentCompletionMode::Copy
                    && runtime.as_ref().is_some_and(|runtime| {
                        presentation.allocation_subject.is_some_and(|subject| {
                            runtime.compare_present_layout(subject, comparison)
                        })
                    });
                if matched {
                    crate::XPresentLayoutComparisonResult::Matched
                } else {
                    crate::XPresentLayoutComparisonResult::Rejected
                }
            });
            if !presentation.phases.observe_complete() {
                return Ok(crate::XPresentCompleteRouteOutcome {
                    routed: false,
                    mode,
                    layout_comparison: None,
                });
            }
            let _ = self.pending_presentations.completed.fetch_update(
                Ordering::Relaxed, Ordering::Relaxed, |n| Some(n.saturating_add(1)));
            let advise = presentation.suboptimal
                && layout_comparison == Some(crate::XPresentLayoutComparisonResult::Matched)
                && comparison.zip(presentation.allocation_subject).is_some_and(
                    |(comparison, subject)| {
                        runtime.as_mut().is_some_and(|runtime| {
                            runtime.claim_present_reallocation(subject, comparison)
                        })
                    },
                );
            let mode = if advise {
                XPresentCompletionMode::SuboptimalCopy
            } else {
                mode
            };
            let presentation = *presentation;
            if presentation.phases.finished() {
                pending.remove(&transaction);
                self.pending_presentations.capacity_changed.notify_all();
            }
            (presentation, layout_comparison, mode, ust, msc)
        };
        // This legacy observer is removed when NotifyMSC admission is wired
        // to the per-window schedule. Never feed a window-relative completion
        // from the new path into its global clock in the meantime.
        if matches!(clock, XPresentCompletionClock::Legacy { .. }) {
            *self
                .present_clock
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)? = Some((ust, msc));
            let ripe = {
                let mut pending = self
                    .pending_msc_notifies
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
                let (ripe, waiting) = pending
                    .drain(..)
                    .partition::<Vec<_>, _>(|(_, _, target)| *target <= msc);
                *pending = waiting;
                ripe
            };
            for (window, serial, _) in ripe {
                self.route_present_msc_notify(window, serial, ust, msc)?;
            }
        }
        let subscriptions = self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, _), subscription)| {
                (subscription.window == presentation.window
                    && subscription.mask & (1 << 1) != 0)
                    .then_some((*client, *subscription))
            })
            .collect::<Vec<_>>();
        if subscriptions.is_empty() {
            return Ok(crate::XPresentCompleteRouteOutcome {
                routed: false,
                mode,
                layout_comparison,
            });
        }
        // A Present subscription belongs to whoever took it, not to
        // whoever presents. A browser subscribes from its GPU process for a
        // window its browser process created, which X permits and Mesa
        // relies on: it blocks in xcb_wait_for_special_event until an idle
        // notify arrives, so an event withheld here is not an error the
        // client can see -- it is a client that never draws again.
        for (target, subscription) in subscriptions {
            let event = XClientEvent::PresentCompleteNotify {
                sequence: 0,
                event_id: subscription.event_id,
                window: presentation.window,
                serial: presentation.serial,
                ust,
                msc,
                kind: 0,
                mode: mode as u8,
            };
            crate::evidence::present_event(target, Some(transaction), "ready", event);
            // A subscriber that cannot take it is ended; the ones behind it
            // are still told (t090).
            self.route_protocol_contained(target, event)?;
        }
        Ok(crate::XPresentCompleteRouteOutcome {
            routed: true,
            mode,
            layout_comparison,
        })
    }

    fn cancel_present(&self, transaction: TransactionId) -> Result<(), XServerFrontendRouteError> {
        self.pending_presentations
            .entries
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .remove(&transaction);
        self.pending_presentations.capacity_changed.notify_all();
        Ok(())
    }

    fn cancel_present_window(&self, window: XResourceId) -> Result<(), XServerFrontendRouteError> {
        self.pending_presentations.entries.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .retain(|_, p| p.window != window);
        self.pending_presentations.capacity_changed.notify_all();
        self.pending_msc_notifies.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .retain(|(target, _, _)| *target != window);
        self.present_subscriptions.lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .retain(|_, subscription| subscription.window != window);
        Ok(())
    }

    fn route_present_idle(
        &self,
        transaction: TransactionId,
    ) -> Result<bool, XServerFrontendRouteError> {
        let presentation = {
            let mut pending = self
                .pending_presentations
                .entries
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some(presentation) = pending.get_mut(&transaction) else {
                return Ok(false);
            };
            if !presentation.phases.observe_idle() {
                return Ok(false);
            }
            let presentation = *presentation;
            // Copy may release its source before display completion, while
            // Flip completes before its retained source becomes idle. Keep
            // the route until both independently owned phases arrive.
            if presentation.phases.finished() {
                pending.remove(&transaction);
                self.pending_presentations.capacity_changed.notify_all();
            }
            presentation
        };
        let subscriptions = self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, _), subscription)| {
                (subscription.window == presentation.window
                    && subscription.mask & (1 << 2) != 0)
                    .then_some((*client, *subscription))
            })
            .collect::<Vec<_>>();
        if subscriptions.is_empty() {
            return Ok(false);
        }
        for (target, subscription) in subscriptions {
            let event = XClientEvent::PresentIdleNotify {
                sequence: 0,
                event_id: subscription.event_id,
                window: presentation.window,
                serial: presentation.serial,
                pixmap: presentation.pixmap,
                idle_fence: presentation.idle_fence,
            };
            crate::evidence::present_event(target, Some(transaction), "ready", event);
            // A subscriber that cannot take it is ended; the ones behind it
            // are still told (t090).
            self.route_protocol_contained(target, event)?;
        }
        Ok(true)
    }
}

impl PersistentLiveLayout {
    fn resolve_pending(&mut self) -> Option<LiveWmCommitResult> {
        if !self.pending_is_ready() {
            return None;
        }
        let pending = self.pending.take().expect("checked above");
        Some(self.commit_pending(pending))
    }

    fn pending_is_ready(&self) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        pending.presentation_settlements.is_empty()
            && pending.requested_sizes.iter().all(|(surface, size)| {
            let staged_matches = pending
                .staged_transactions
                .get(surface)
                .is_some_and(|transaction| {
                    live_transaction_observed_size(
                        transaction,
                        &self.dma_buf_sizes,
                        &self.cpu_buffer_sizes,
                    )
                        == *size
                });
            let retained_matches = self.layout_epochs.committed_size(*surface) == Some(*size)
                && pending
                    .layers
                    .iter()
                    .find(|layer| layer.surface == *surface)
                    .is_some_and(|layer| layer.source != BufferSource::None);
            let admission_ready = !pending.admission_surfaces.contains(surface)
                || matches!(
                    self.admissions.state(*surface),
                    sophia_engine::SurfacePresentationAdmissionState::AwaitingPixels { .. }
                        | sophia_engine::SurfacePresentationAdmissionState::Managed
                );
            let pixels_ready = if pending.admission_surfaces.contains(surface) {
                staged_matches
            } else {
                staged_matches || retained_matches
            };
            pixels_ready && admission_ready
            })
    }

    fn acknowledge_presentation_control(
        &mut self,
        transaction: TransactionId,
        surface: SurfaceId,
    ) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        pending.transaction == transaction && pending.presentation_settlements.remove(&surface)
    }

    fn force_pending_timeout(&mut self) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        pending.deadline = Instant::now();
        true
    }

    fn expire_pending(
        &mut self,
        session_controls: &mut SessionControlQueue,
    ) -> Result<Option<LiveWmCommitResult>, Box<dyn std::error::Error>> {
        if self
            .pending
            .as_ref().is_none_or(|pending| Instant::now() < pending.deadline)
        {
            return Ok(None);
        }
        let pending = self.pending.take().expect("checked above");
        let admission_surfaces = pending
            .layers
            .iter()
            .map(|layer| layer.surface)
            .filter(|surface| self.unmanaged_surfaces.contains(surface))
            .collect::<Vec<_>>();
        let terminal_admissions = admission_surfaces
            .iter()
            .copied()
            .filter(|surface| self.admission_retries.get(surface).copied().unwrap_or(0) >= 1)
            .collect::<BTreeSet<_>>();
        for surface in admission_surfaces
            .iter()
            .copied()
            .filter(|surface| !terminal_admissions.contains(surface))
        {
            self.synchronize_admission_extent(surface);
        }
        let recoverable_admissions = admission_surfaces
            .iter()
            .copied()
            .filter(|surface| {
                !terminal_admissions.contains(surface)
                    && self.surface_awaits_visual_candidate(*surface)
                    && self
                        .layout_epochs
                        .recovery_extent(*surface)
                        .is_some_and(|extent| {
                            self.selected_pre_admission_transaction(*surface, extent)
                                .is_some()
                        })
            })
            .collect::<Vec<_>>();
        // A first-launch admission with retained pixels is fenced through a
        // fixed recovery extent, not rolled back. Candidate-less geometry and
        // pixel-silent admission are expected states: they keep the standing
        // target and bounded retry but cannot claim an extent whose exact
        // transaction is unavailable at the quarantine boundary.
        // A recovery request needs a committed extent to configure back to. A
        // surface the coordinator no longer knows — one withdrawn while its
        // resize was outstanding — has none, and naming it fails the whole
        // recovery for every surface that could still be recovered. The fixed
        // set above already applies this test; requests must apply it too.
        let recoverable_requests = pending
            .requested_sizes
            .iter()
            .filter(|(surface, _)| {
                !terminal_admissions.contains(surface)
                    && !admission_surfaces.contains(surface)
                    && self.layout_epochs.safe_size(**surface).is_some()
            })
            .map(|(surface, size)| (*surface, *size))
            .collect::<Vec<_>>();
        let rollback = self
            .layout_epochs
            .begin_recovery(recoverable_requests, recoverable_admissions)?;
        // Retain each fenced admission surface's blind-WM target as a standing
        // obligation. Once its temporary recovery extent clears it is driven to
        // that size rather than staying welded to the extent it first mapped at.
        for surface in &admission_surfaces {
            if terminal_admissions.contains(surface) {
                continue;
            }
            if let Some(target) = pending.requested_sizes.get(surface) {
                self.layout_epochs.set_pending_target(*surface, *target);
            }
        }
        let rollback_transaction = rollback
            .first()
            .map(|request| request.transaction)
            .unwrap_or(pending.transaction);
        let mut rollback_configures = rollback.len();
        let mut rollback_withdrawn = 0usize;
        for request in rollback {
            let surface = request.surface;
            let size = request.size;
            // A surface can be withdrawn between the proposal and the rollback
            // that undoes it -- a window closing while a layout is in flight is
            // ordinary, not exceptional. There is nothing to restore it to and
            // nobody left to tell, so it is skipped and counted.
            //
            // This used to end the session. A workspace switch with a dock
            // present was enough to reach it, and killing a desktop because one
            // window stopped existing is never the right answer.
            let Some(geometry) = self.layers.get(&surface).map(|layer| Rect {
                width: size.width,
                height: size.height,
                ..layer.geometry
            }) else {
                rollback_configures = rollback_configures.saturating_sub(1);
                rollback_withdrawn += 1;
                continue;
            };
            let Some(client) = self.client_routes.client_for_surface(surface) else {
                rollback_configures = rollback_configures.saturating_sub(1);
                rollback_withdrawn += 1;
                continue;
            };
            session_controls.enqueue(XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: rollback_transaction,
                    surface,
                    geometry,
                },
            }, Instant::now()).map_err(|error| {
                format!("failed to queue WM rollback control: {error:?}")
            })?;
        }
        for (surface, desired) in &pending.presentation_states {
            let previous = self
                .committed_policy_presentations
                .get(surface)
                .copied()
                .unwrap_or_default();
            if previous == *desired || terminal_admissions.contains(surface) {
                continue;
            }
            // Withdrawn during the cycle, as above.
            let Some(client) = self.client_routes.client_for_surface(*surface) else {
                rollback_withdrawn += 1;
                continue;
            };
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand {
                        client,
                        command: XAuthorityControlCommand::RestorePresentationState {
                            transaction: rollback_transaction,
                            surface: *surface,
                            state: previous,
                        },
                    },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue WM presentation rollback: {error:?}")
                })?;
        }
        for surface in &terminal_admissions {
            // The most likely of the three to be gone: these are surfaces
            // already on their way out, so losing the route before the
            // withdrawal is delivered is the expected ending, not a fault.
            let Some(client) = self.client_routes.client_for_surface(*surface) else {
                rollback_withdrawn += 1;
                continue;
            };
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand {
                        client,
                        command: XAuthorityControlCommand::WithdrawSurface {
                            transaction: pending.transaction,
                            surface: *surface,
                        },
                    },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue terminal WM admission withdrawal: {error:?}")
                })?;
            // Giving up on a surface is a decision, and it was silent: the
            // coordinator erased an admission with nothing on the record, so a
            // launch that owned it went on waiting for a surface that no longer
            // existed. Say so, and keep the surface for whoever is waiting.
            crate::session_println!(
                "sophia_live_surface_admission schema=1 status=withdrawn transaction={} surface={} reason=retries_exhausted",
                pending.transaction.raw(),
                surface.index(),
            );
            self.withdrawn_admissions.push(*surface);
            self.admissions.remove(*surface);
            self.planning_surfaces.remove(surface);
            self.authority_surface_facts.remove(surface);
            self.unmanaged_surfaces.remove(surface);
            self.admission_retries.remove(surface);
            self.manage_settlements.remove(surface);
            self.layout_epochs.remove(*surface);
            self.remove_admission_groups(*surface);
        }
        let resize_state = pending
            .requested_sizes
            .iter()
            .map(|(surface, expected)| {
                let observed = self
                    .layout_epochs
                    .committed_size(*surface)
                    .unwrap_or(Size {
                    width: 0,
                    height: 0,
                });
                format!(
                    "{}x{}:{}x{}",
                    expected.width, expected.height, observed.width, observed.height
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        crate::session_println!(
            "sophia_live_wm schema=1 status=layout_timeout transaction={} preserved_layout=true rollback_transaction={} rollback_configures={} rollback_withdrawn={rollback_withdrawn} resize_state={}",
            pending.transaction.raw(),
            rollback_transaction.raw(),
            rollback_configures,
            resize_state,
        );
        // A retry is spent by a surface that was asked and did not answer. A
        // surface deferred out of the gate was never asked, so an expiry it
        // took no part in must not count against it — two of those retire an
        // admission, and a launching client would be withdrawn for a deadline
        // that was never its to meet. `stage` and this bound read the same
        // fact so they cannot come apart.
        for surface in admission_surfaces
            .into_iter()
            .filter(|surface| !terminal_admissions.contains(surface))
            .filter(|surface| pending.requested_sizes.contains_key(surface))
        {
            let attempts = self.admission_retries.entry(surface).or_default();
            *attempts = attempts.saturating_add(1);
            self.manage_settlements.remove(&surface);
        }
        crate::session_println!(
            "sophia_live_resize_epoch schema=1 status=aborted transaction={} rejected_surfaces={}",
            pending.transaction.raw(),
            pending.requested_sizes.len(),
        );
        Ok(Some(LiveWmCommitResult {
            update: WmTransactionUpdate {
                commit: TransactionCommit {
                    transaction: pending.transaction,
                    outcome: TransactionOutcome::TimedOut,
                    applied_surfaces: Vec::new(),
                },
            },
            // The external WM has already applied the request that produced
            // this proposal. Retain its source so the owner can restart and
            // reseed that speculative peer after rejecting the layout.
            source: pending.source,
            policy_settlement: pending.policy_settlement,
        }))
    }
}

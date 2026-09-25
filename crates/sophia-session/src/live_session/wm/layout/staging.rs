impl PersistentLiveLayout {
    fn stage(
        &mut self,
        mut proposal: LiveWmProposal,
        session_controls: &mut SessionControlQueue,
    ) -> Result<Option<LiveWmCommitResult>, Box<dyn std::error::Error>> {
        if self.pending.is_some() {
            crate::session_println!(
                "sophia_live_wm schema=1 status=proposal_busy transaction={} preserved_layout=true",
                proposal.transaction.raw()
            );
            return Ok(None);
        }
        // A standing launch target is an obligation, not immutable policy.
        // A new work area can replace it before the recovery frame retires.
        // Keep it only when the WM is echoing the temporary recovery constraint.
        for (&surface, &target) in &proposal.requested_sizes {
            if self.layout_epochs.pending_target(surface).is_some()
                && self.layout_epochs.recovery_extent(surface) != Some(target)
            {
                self.layout_epochs.set_pending_target(surface, target);
            }
        }
        for layer in proposal
            .layers
            .iter()
            .filter(|layer| self.surface_awaits_visual_candidate(layer.surface))
        {
            proposal.requested_sizes.entry(layer.surface).or_insert(Size {
                width: layer.geometry.width,
                height: layer.geometry.height,
            });
        }
        // Drive a standing target left by an aborted launch epoch through this
        // resize transaction rather than out of band. A first-launch client is
        // fenced and admitted at whatever extent it first mapped; its blind-WM
        // target is retained as an obligation and injected here so the same
        // ConfigureSurface, exact-size epoch gate, and record_committed run as
        // one transaction. That keeps the client, the Engine committed size,
        // and the WM layer in agreement, so the resized frame is accepted (not
        // rejected as a stale surface) and a denied reactive client configure
        // is answered with the target rather than the welded launch size.
        let standing_targets = self
            .layout_epochs
            .pending_target_surfaces()
            .filter(|(surface, target)| {
                self.layout_epochs.recovery_extent(*surface).is_none()
                    && self.layout_epochs.committed_size(*surface) != Some(*target)
            })
            .collect::<Vec<_>>();
        for (surface, target) in standing_targets {
            if let Some(layer) = proposal
                .layers
                .iter_mut()
                .find(|layer| layer.surface == surface)
            {
                layer.geometry.width = target.width;
                layer.geometry.height = target.height;
                proposal.requested_sizes.insert(surface, target);
                let attempts = self.layout_epochs.note_standing_redrive(surface);
                // An obligation re-driven this often is not converging, and the
                // reconfigures are identical, so nothing else in the run would
                // say so. Reported once at the boundary rather than every cycle:
                // the loop is cheap, and a line per frame would bury it.
                if attempts == STANDING_TARGET_REDRIVE_REPORT_THRESHOLD {
                    let committed = self.layout_epochs.committed_size(surface);
                    tracing::warn!(
                        "sophia_live_wm_standing_target schema=1 status=not_converging surface={} attempts={} target={}x{} committed={} action=continue",
                        surface.index(),
                        attempts,
                        target.width,
                        target.height,
                        committed.map_or_else(
                            || "none".to_owned(),
                            |size| format!("{}x{}", size.width, size.height)
                        ),
                    );
                }
            }
        }
        for (surface, size) in &proposal.requested_sizes {
            if !self.layout_epochs.request_allowed(*surface, *size)
                && let Some(committed) = self.layout_epochs.committed_size(*surface)
                && let Some(layer) = proposal
                    .layers
                    .iter_mut()
                    .find(|layer| layer.surface == *surface)
            {
                layer.geometry.width = committed.width;
                layer.geometry.height = committed.height;
            }
        }
        proposal.requested_sizes.retain(|surface, size| {
            let installed_visual_target = self
                .layers
                .get(surface)
                .is_some_and(|layer| {
                    layer.geometry.width == size.width && layer.geometry.height == size.height
                })
                && self
                    .awaiting_visual_commits
                    .surface_layout_awaiting(*surface, *size);
            self.surface_awaits_visual_candidate(*surface)
                || (!installed_visual_target
                    && self.layout_epochs.request_allowed(*surface, *size)
                    && self.layout_epochs.committed_size(*surface) != Some(*size))
        });
        let mut staged_transactions = BTreeMap::new();
        for (surface, size) in &proposal.requested_sizes {
            let Some(transaction) = self.selected_pre_admission_transaction(*surface, *size)
            else {
                continue;
            };
            staged_transactions.insert(*surface, transaction.clone());
            if let Some(layer) = proposal
                .layers
                .iter_mut()
                .find(|layer| layer.surface == *surface)
            {
                layer.source = transaction.target_buffer();
                layer.damage = transaction.damage.clone();
                layer.generation = transaction.previous_committed_generation.saturating_add(1);
            }
        }
        // X position feedback is required even when committed pixels remain
        // reusable. Keep it separate from the resize-only readiness map.
        let changed_geometries = proposal
            .layers
            .iter()
            .filter_map(|layer| {
                let moved = self
                    .layers
                    .get(&layer.surface)
                    .is_none_or(|current| current.geometry != layer.geometry);
                // A recovery reseed can retain Engine geometry while late
                // pixels from the aborted target leave the client at another
                // size. Its resize obligation must reassert the same rectangle.
                (moved
                    || proposal.requested_sizes.contains_key(&layer.surface)
                    || self.surface_awaits_visual_candidate(layer.surface))
                    .then_some((layer.surface, layer.geometry))
            })
            .collect::<BTreeMap<_, _>>();
        proposal.moved_surfaces = proposal
            .layers
            .iter()
            .filter(|layer| {
                self.layers
                    .get(&layer.surface)
                    .is_none_or(|current| current.geometry != layer.geometry)
            })
            .count();
        let mut admission_surfaces = BTreeSet::new();
        for (surface, geometry) in changed_geometries {
            let stage =
                self.stage_surface_control(proposal.transaction, surface, geometry)?;
            if stage.admission_owned {
                admission_surfaces.insert(surface);
            }
            let Some(command) = stage.command else {
                continue;
            };
            proposal.configure_deliveries = proposal.configure_deliveries.saturating_add(1);
            let client = self
                .client_routes
                .client_for_surface(surface)
                .ok_or("live WM configure has no X11 client route for its surface")?;
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand { client, command },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue WM configure control: {error:?}")
                })?;
        }
        let presentation_settlements = proposal
            .presentation_states
            .iter()
            .filter_map(|(surface, state)| {
                (self.committed_policy_presentations.get(surface) != Some(state))
                    .then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        for surface in &presentation_settlements {
            let state = proposal.presentation_states[surface];
            let client = self
                .client_routes
                .client_for_surface(*surface)
                .ok_or("live WM presentation state has no X11 client route")?;
            session_controls
                .enqueue(
                    XAuthorityClientControlCommand {
                        client,
                        command: XAuthorityControlCommand::SetPresentationState {
                            transaction: proposal.transaction,
                            surface: *surface,
                            state,
                        },
                    },
                    Instant::now(),
                )
                .map_err(|error| {
                    format!("failed to queue WM presentation-state control: {error:?}")
                })?;
        }
        // A surface that has never presented cannot answer this epoch's gate.
        // The gate admits a surface on an exact-size frame, and a surface with
        // no safe observation has no pixels to resize: it can only pass by
        // drawing its first frame at precisely the requested extent, inside a
        // deadline a blind WM sizes for settled clients. A cold browser needs
        // seconds; the deadline is a few hundred milliseconds. Holding one in
        // the gate does not make it answer faster — it fails the epoch and
        // takes every sibling's resize down with it.
        //
        // Defer it to a standing obligation instead. Its ConfigureSurface has
        // already gone out above, so the client still learns its target; what
        // changes here is only whether the epoch waits on it. The obligation is
        // driven by the standing-target injection at the head of this function
        // once the surface has pixels to resize.
        let deferred_surfaces = proposal
            .requested_sizes
            .keys()
            .copied()
            .filter(|surface| self.layout_epochs.safe_size(*surface).is_none())
            .collect::<Vec<_>>();
        for surface in &deferred_surfaces {
            if let Some(target) = proposal.requested_sizes.remove(surface) {
                self.layout_epochs.set_pending_target(*surface, target);
            }
        }
        let ready = proposal
            .requested_sizes
            .iter()
            .all(|(surface, size)| {
                self.layout_epochs.committed_size(*surface) == Some(*size)
                    && !admission_surfaces.contains(surface)
            })
            && presentation_settlements.is_empty();
        if ready {
            return Ok(Some(self.commit_proposal(proposal)));
        }
        let timeout_msec = proposal.timeout.as_millis();
        self.pending = Some(PendingLiveWmLayout {
            transaction: proposal.transaction,
            layers: proposal.layers,
            requested_sizes: proposal.requested_sizes,
            presentation_states: proposal.presentation_states,
            presentation_settlements,
            configure_deliveries: proposal.configure_deliveries,
            focus: proposal.focus,
            deadline: Instant::now() + proposal.timeout,
            update: proposal.update,
            moved_surfaces: proposal.moved_surfaces,
            staged_transactions,
            admission_surfaces,
            source: proposal.source,
            policy_settlement: proposal.policy_settlement,
        });
        // `surfaces` counts what the epoch actually waits on, so `deferred`
        // must name what was held back: a reader cannot otherwise tell a
        // one-surface epoch from a two-surface one that excused a sibling.
        // The deadline is reported with them because it is the blind WM's
        // choice, and a gate that expires early is indistinguishable from a
        // client that never answered unless both are on the record.
        crate::session_println!(
            "sophia_live_resize_epoch schema=2 status=held transaction={} surfaces={} deferred={} timeout_msec={}",
            self.pending
                .as_ref()
                .expect("pending layout was just installed")
                .transaction
                .raw(),
            self.pending
                .as_ref()
                .expect("pending layout was just installed")
                .requested_sizes
                .len(),
            deferred_surfaces.len(),
            timeout_msec,
        );
        Ok(None)
    }
}

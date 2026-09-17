use super::*;

#[derive(Default)]
pub(super) struct LiveWindowAllocationPublisher {
    generation: u64,
    quiescing: bool,
    next_check: Option<Instant>,
    applied: Option<sophia_x_authority::XWindowAllocationPreferences>,
    pending: Option<(
        sophia_x_authority::XWindowAllocationPreferences,
        Receiver<sophia_x_authority::XWindowAllocationUpdate>,
    )>,
}

#[path = "../../tests/support/window_allocation_comparison.rs"]
mod tests;

fn contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.width > 0
        && inner.height > 0
        && inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

fn frontend_context(
    output: sophia_protocol::OutputId,
    context: sophia_backend_live::LiveOutputAllocationContext,
) -> sophia_x_authority::XWindowAllocationContext {
    sophia_x_authority::XWindowAllocationContext {
        generation: context.generation,
        output,
    }
}

pub(super) fn window_allocation_rows(
    surfaces: &[sophia_protocol::CommittedSurfaceState],
    mapped: &BTreeSet<SurfaceId>,
    bounds: &[(sophia_protocol::OutputId, Rect)],
    preferences: &[sophia_backend_live::LiveOutputAllocationPreference],
) -> Vec<sophia_x_authority::XWindowAllocationPreference> {
    use sophia_x_authority::{
        XDrmDeviceHint, XServerFrontendDmaBufImportFormat, XWindowAllocationPreference,
    };
    // A spanning or ambiguously placed window has no single-output preference.
    surfaces
        .iter()
        .filter(|surface| mapped.contains(&surface.surface))
        .filter_map(|surface| {
            let mut outputs = bounds
                .iter()
                .filter(|(_, bounds)| contains_rect(*bounds, surface.geometry));
            let (output, _) = outputs.next()?;
            if outputs.next().is_some() {
                return None;
            }
            let preference = preferences.iter().find(|row| row.output == *output)?;
            if preference.formats.is_empty() {
                return None;
            }
            Some(XWindowAllocationPreference {
                surface: surface.surface,
                device: XDrmDeviceHint {
                    major: rustix::fs::major(preference.device_number),
                    minor: rustix::fs::minor(preference.device_number),
                },
                identity: preference.identity.map(|identity| {
                    sophia_x_authority::XRenderDeviceIdentity {
                        device: identity.device,
                        inode: identity.inode,
                        device_number: identity.device_number,
                    }
                }),
                context: preference
                    .context
                    .map(|context| frontend_context(*output, context)),
                formats: preference
                    .formats
                    .iter()
                    .map(|row| XServerFrontendDmaBufImportFormat {
                        format: row.format,
                        modifiers: row.modifiers.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}

pub(super) struct LiveWindowAllocationView<'a> {
    pub publisher: &'a LiveWindowAllocationPublisher,
    pub native: Option<&'a LiveProductionNativeScanout>,
    pub layout: &'a PersistentLiveLayout,
    pub outputs: &'a [sophia_engine::HeadlessOutput],
    pub topology_generation: u64,
}

impl LiveWindowAllocationView<'_> {
    pub(super) fn comparison(
        &self,
        surfaces: &[sophia_protocol::CommittedSurfaceState],
        evidence: &sophia_backend_live::LivePresentLayoutComparison,
    ) -> Option<(
        sophia_protocol::TransactionId,
        sophia_x_authority::XPresentLayoutComparison,
    )> {
        if self.layout.pending.is_some()
            || !self
                .layout
                .mapped_surfaces
                .contains(&evidence.candidate.surface)
        {
            return None;
        }
        let surface = surfaces
            .iter()
            .find(|surface| surface.surface == evidence.candidate.surface)?;
        let bounds = wm_output_bounds(self.outputs);
        let mut matches = bounds
            .iter()
            .filter(|(_, bounds)| contains_rect(*bounds, surface.geometry));
        let (output, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let (context, device) = self.native?.output_allocation_context(*output)?;
        self.publisher.compare_retired(
            evidence,
            surface,
            self.topology_generation,
            *output,
            context,
            device,
        )
    }
}

impl LiveWindowAllocationPublisher {
    fn compare_retired(
        &self,
        evidence: &sophia_backend_live::LivePresentLayoutComparison,
        surface: &sophia_protocol::CommittedSurfaceState,
        topology_generation: u64,
        output: sophia_protocol::OutputId,
        context: sophia_backend_live::LiveOutputAllocationContext,
        device: sophia_backend_live::LiveRenderDeviceNodeIdentity,
    ) -> Option<(
        sophia_protocol::TransactionId,
        sophia_x_authority::XPresentLayoutComparison,
    )> {
        let applied = self.applied.as_ref()?;
        let retired = evidence.retired;
        let witness = retired.witness;
        let trace = witness.alternative.trace?;
        let sophia_protocol::BufferSource::DmaBuf { handle } = evidence.candidate.target_buffer
        else {
            return None;
        };
        if applied.topology_generation != topology_generation
            || surface.surface != evidence.candidate.surface
            || surface.buffer() != evidence.candidate.target_buffer
            || trace.output != output
            || trace.head != context.head
            || retired.head != context.head
            || retired.target_generation != context.target_generation
            || retired.context_generation != context.generation
            || retired.device != device
        {
            return None;
        }
        let native_context = frontend_context(output, context);
        let device_identity = sophia_x_authority::XRenderDeviceIdentity {
            device: device.device,
            inode: device.inode,
            device_number: device.device_number,
        };
        let preference = applied
            .windows
            .iter()
            .find(|row| row.surface == surface.surface)?;
        if preference.context != Some(native_context)
            || preference.identity != Some(device_identity)
        {
            return None;
        }
        let row = preference
            .formats
            .iter()
            .find(|row| row.format == witness.format)?;
        if !row.modifiers.contains(&witness.alternative_modifier)
            || row.modifiers.contains(&witness.original_modifier)
        {
            return None;
        }
        Some((
            evidence.candidate.transaction,
            sophia_x_authority::XPresentLayoutComparison {
                surface: surface.surface,
                buffer: sophia_protocol::BufferHandle::from_raw(handle),
                format: witness.format,
                original_modifier: witness.original_modifier,
                alternative_modifier: witness.alternative_modifier,
                preference_generation: applied.generation,
                topology_generation,
                native_context,
                device_identity,
                geometry: surface.geometry,
            },
        ))
    }

    /// Preferences own metadata only. Stop before frontend drain, preserving
    /// the last applied witness for already accepted presentation comparisons.
    pub(super) fn begin_quiescence(&mut self) {
        if self.quiescing {
            return;
        }
        self.quiescing = true;
        let cancelled = self.pending.take().is_some();
        self.next_check = None;
        crate::session_println!(
            "sophia_window_allocation_publisher schema=1 status=stopped pending_cancelled={cancelled} generation={}",
            self.generation,
        );
    }

    pub(super) fn poll(
        &mut self,
        now: Instant,
        topology_generation: u64,
        runtime: &LiveProductionVisualRuntime,
        native: &LiveProductionNativeScanout,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        service: &SyncSender<XServerFrontendServiceCommand>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.poll_snapshot(now, topology_generation, service, || {
            window_allocation_rows(
                runtime.committed_surfaces(),
                &layout.mapped_surfaces,
                &wm_output_bounds(outputs),
                &native.output_allocation_preferences(),
            )
        })
    }

    fn poll_snapshot(
        &mut self,
        now: Instant,
        topology_generation: u64,
        service: &SyncSender<XServerFrontendServiceCommand>,
        snapshot: impl FnOnce() -> Vec<sophia_x_authority::XWindowAllocationPreference>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.quiescing {
            return Ok(());
        }
        if let Some((_, receiver)) = self.pending.as_ref() {
            match receiver.try_recv() {
                Ok(sophia_x_authority::XWindowAllocationUpdate::Applied) => {
                    self.applied = self.pending.take().map(|(snapshot, _)| snapshot);
                }
                Ok(_) => {
                    self.pending = None;
                    self.applied = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("window allocation acknowledgement disconnected".into());
                }
            }
        }
        if self.next_check.is_some_and(|deadline| now < deadline) {
            return Ok(());
        }
        self.next_check = Some(now + Duration::from_millis(250));
        let windows = snapshot();
        if self.applied.as_ref().is_some_and(|old| {
            old.topology_generation == topology_generation && old.windows == windows
        }) {
            return Ok(());
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("window allocation generation exhausted")?;
        let snapshot = sophia_x_authority::XWindowAllocationPreferences {
            generation,
            topology_generation,
            windows,
        };
        let (sender, receiver) = sync_channel(1);
        match service.try_send(
            XServerFrontendServiceCommand::UpdateWindowAllocationPreferences {
                snapshot: snapshot.clone(),
                acknowledgement: sender,
            },
        ) {
            Ok(()) => {
                self.generation = generation;
                self.pending = Some((snapshot, receiver));
            }
            Err(std::sync::mpsc::TrySendError::Full(_)) => {}
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                return Err("window allocation frontend disconnected".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/support/window_allocation_shutdown.rs"]
mod shutdown_tests;

//! Capture retirement is maintenance, independent of future frame composition.
use super::*;
use sophia_renderer_live::{
    LiveRendererSnapshotBudget, LiveRendererSnapshotEpoch, LiveShellCaptureCustody,
    LiveShellCaptureFacts, LiveShellCaptureInbox,
};
use std::{io, sync::Arc};

struct Install {
    output: OutputId,
    store: u64,
    card_device: u64,
    generation: u64,
    epoch: Arc<LiveRendererSnapshotEpoch>,
}

impl Install {
    fn bind_facts(&self, mut facts: LiveShellCaptureFacts) -> Option<LiveShellCaptureFacts> {
        if facts.device != self.card_device {
            return None;
        }
        // GBM uses the held card fd; Session grants use its matched render node.
        facts.device = self.epoch.card;
        Some(facts)
    }
}

#[derive(Default)]
pub(super) struct CaptureOwner {
    managed: bool,
    // One persistent bound for this native owner, including worker replacement.
    // Never reset when closing a mailbox or changing the topology.
    custody: LiveShellCaptureCustody,
    pending: Option<Install>,
    active: Option<Install>,
    closing: bool,
    close_sent: bool,
}

impl CaptureOwner {
    fn observe(&mut self, generation: u64, mut current: impl FnMut(OutputId, u64) -> bool) {
        if self.pending.iter().chain(self.active.iter()).any(|i| {
            i.generation != generation || !i.epoch.is_valid() || !current(i.output, i.store)
        }) {
            self.closing = true;
        }
        if self.closing {
            for i in self.pending.iter().chain(self.active.iter()) {
                i.epoch.invalidate();
            }
        }
    }
}

impl Drop for CaptureOwner {
    fn drop(&mut self) {
        for install in self.pending.iter().chain(self.active.iter()) {
            install.epoch.invalidate();
        }
    }
}

/// Renderer-owned endpoint and budget, not shell admission or completion proof.
pub struct LiveNativeShellCaptureEndpoint {
    inbox: LiveShellCaptureInbox,
    budget: Arc<LiveRendererSnapshotBudget>,
    epoch: Arc<LiveRendererSnapshotEpoch>,
    output: OutputId,
    facts: sophia_renderer_live::LiveShellCaptureFacts,
}

impl LiveNativeShellCaptureEndpoint {
    pub fn output(&self) -> OutputId {
        self.output
    }
    /// Immutable capabilities snapshot taken on the renderer owner at
    /// installation, bound to this endpoint's exact epoch and budget.
    /// Session advertises only what this supports; destination bounds
    /// for a concrete size come from the prepare request, not from here.
    pub fn facts(&self) -> &sophia_renderer_live::LiveShellCaptureFacts {
        &self.facts
    }
    pub fn into_parts(
        self,
    ) -> (
        LiveShellCaptureInbox,
        Arc<LiveRendererSnapshotBudget>,
        Arc<LiveRendererSnapshotEpoch>,
    ) {
        (self.inbox, self.budget, self.epoch)
    }
}

impl LiveProductionNativeScanout {
    /// Opt-in before installing an endpoint. Existing mailbox generations never
    /// change storage mode while they hold requests or prepared allocations.
    pub fn set_shell_capture_destination_reuse(&mut self, enabled: bool) -> io::Result<()> {
        let capture = &mut self.preview_images.capture;
        if capture.pending.is_some() || capture.active.is_some() || capture.closing {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        capture.managed = enabled;
        Ok(())
    }

    /// Existing native owner turn, independent of frames and admission. A lost
    /// output/store or generation withdraws immediately; full queues retry.
    pub fn service_shell_capture_lifecycle(&mut self) -> io::Result<()> {
        let heads = &self.heads;
        let exporters = &self.exporters;
        self.preview_images
            .capture
            .observe(self.render_devices.generation, |output, store| {
                heads.iter().zip(exporters).any(|(head, exporter)| {
                    head.enabled
                        && head.output.id == output
                        && exporter.image_store_identity() == Some(store)
                })
            });
        if !self.preview_images.capture.closing {
            return Ok(());
        }
        self.close_shell_capture()?;
        if self.preview_images.capture.pending.is_some() {
            // An invalidated enable reply is never exposed as a usable inbox.
            let _ = self.poll_shell_capture();
        }
        self.preview_images.capture.closing = self.preview_images.capture.pending.is_some()
            || self.preview_images.capture.active.is_some();
        Ok(())
    }

    pub fn shell_capture_maintenance_wait(&self) -> Option<Duration> {
        (self.preview_images.capture.pending.is_some() || self.preview_images.capture.closing)
            .then_some(Duration::from_millis(10))
    }

    /// Explicit output, existing worker only; no context/device creation.
    /// One outstanding/active endpoint per native owner. Policy remains off.
    pub fn request_shell_capture(&mut self, output: OutputId) -> io::Result<()> {
        self.service_shell_capture_lifecycle()?;
        if self.preview_images.capture.pending.is_some()
            || self.preview_images.capture.active.is_some()
            || !self.output_topology_allows_frame_service()
        {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let index = self
            .heads
            .iter()
            .position(|head| head.enabled && head.output.id == output)
            .ok_or(io::ErrorKind::NotFound)?;
        let group = self.heads[index].group;
        if self
            .heads
            .iter()
            .any(|head| head.enabled && head.output.id == output && head.group != group)
        {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let store = self.exporters[index]
            .image_store_identity()
            .ok_or(io::ErrorKind::Unsupported)?;
        let device = self
            .render_devices
            .group_identity(group)
            .ok_or(io::ErrorKind::NotConnected)?
            .device_number;
        let card_device = rustix::fs::fstat(self.groups[group].session.card())?.st_rdev;
        let generation = self.render_devices.generation;
        // Capture close invalidates only this epoch, not existing preview images.
        let epoch = LiveRendererSnapshotEpoch::new(device, generation, store);
        self.exporters[index].request_shell_capture(
            epoch.clone(),
            self.preview_images.budget.clone(),
            self.preview_images.capture.custody.clone(),
            self.preview_images.capture.managed,
        )?;
        self.preview_images.capture.pending = Some(Install {
            output,
            store,
            card_device,
            generation,
            epoch,
        });
        self.preview_images.capture.close_sent = false;
        Ok(())
    }

    pub fn poll_shell_capture(&mut self) -> io::Result<Option<LiveNativeShellCaptureEndpoint>> {
        let Some(install) = self.preview_images.capture.pending.as_ref() else {
            return Ok(None);
        };
        let Some(index) = self
            .exporters
            .iter()
            .position(|e| e.image_store_identity() == Some(install.store))
        else {
            install.epoch.invalidate();
            self.preview_images.capture.pending = None;
            return Err(io::ErrorKind::NotConnected.into());
        };
        let result = self.exporters[index].poll_shell_capture();
        if matches!(result, Ok(None)) {
            return Ok(None);
        }
        let install = self
            .preview_images
            .capture
            .pending
            .take()
            .expect("pending installation");
        match result {
            Ok(Some((inbox, facts)))
                if install.generation == self.render_devices.generation
                    && install.epoch.is_valid()
                    && self
                        .heads
                        .iter()
                        .zip(&self.exporters)
                        .any(|(head, exporter)| {
                            head.enabled
                                && head.output.id == install.output
                                && exporter.image_store_identity() == Some(install.store)
                        }) =>
            {
                if let Some(facts) = install.bind_facts(facts) {
                    let endpoint = LiveNativeShellCaptureEndpoint {
                        inbox,
                        budget: self.preview_images.budget.clone(),
                        epoch: install.epoch.clone(),
                        output: install.output,
                        facts,
                    };
                    self.preview_images.capture.active = Some(install);
                    return Ok(Some(endpoint));
                }
            }
            Ok(_) => {}
            Err(error) => {
                install.epoch.invalidate();
                return Err(error);
            }
        }
        install.epoch.invalidate();
        // Keep the close obligation on queue pressure; the caller retries close.
        self.preview_images.capture.active = Some(install);
        let _ = self.close_shell_capture();
        Err(io::ErrorKind::Interrupted.into())
    }

    /// Bounded destination preparation for one XRGB size on the active
    /// endpoint for `output`: ask the renderer owner to ensure a
    /// destination allocation exists, then poll for its exact bytes.
    /// One outstanding request; WouldBlock while one is in flight or no
    /// endpoint is active for the output. The returned bound is the
    /// only proven destination bound for a first unseen size; Session
    /// reserves it against the endpoint budget before submitting. `wake` is
    /// rung when the reply, or a disconnect, becomes readable.
    pub fn request_prepare_shell_capture_destination(
        &mut self,
        output: OutputId,
        width: u32,
        height: u32,
        wake: &sophia_wake::Notifier,
    ) -> io::Result<()> {
        let active = self.preview_images.capture.active.as_ref();
        if active.is_none_or(|install| install.output != output) {
            return Err(io::ErrorKind::NotConnected.into());
        }
        let store = active.expect("active endpoint checked").store;
        let index = self
            .exporters
            .iter()
            .position(|e| e.image_store_identity() == Some(store))
            .ok_or(io::ErrorKind::NotConnected)?;
        self.exporters[index].request_prepare_shell_capture_destination(width, height, wake)
    }

    pub fn poll_prepare_shell_capture_destination(
        &mut self,
    ) -> io::Result<Option<sophia_renderer_live::LiveShellCaptureDestination>> {
        let Some(store) = self
            .preview_images
            .capture
            .active
            .as_ref()
            .map(|install| install.store)
        else {
            return Ok(None);
        };
        let index = self
            .exporters
            .iter()
            .position(|e| e.image_store_identity() == Some(store))
            .ok_or(io::ErrorKind::NotConnected)?;
        self.exporters[index].poll_prepare_shell_capture_destination()
    }

    /// Queue close, never claim GPU completion. Session must still drain every
    /// receipt/source obligation; the persistent custody owner is not reset.
    pub fn close_shell_capture(&mut self) -> io::Result<()> {
        self.preview_images.capture.closing = true;
        let install = self.preview_images.capture.active.as_ref().or(self
            .preview_images
            .capture
            .pending
            .as_ref());
        let Some(install) = install else {
            self.preview_images.capture.closing = false;
            return Ok(());
        };
        install.epoch.invalidate();
        if !self.preview_images.capture.close_sent
            && let Some(index) = self
                .exporters
                .iter()
                .position(|e| e.image_store_identity() == Some(install.store))
        {
            self.exporters[index].close_shell_capture()?;
        }
        self.preview_images.capture.close_sent = true;
        self.preview_images.capture.active = None;
        // A pending enable must still be polled and its inbox drained/dropped;
        // its invalid epoch prevents submission even if the reply was queued.
        self.preview_images.capture.closing = self.preview_images.capture.pending.is_some();
        Ok(())
    }

    /// Evict from every current store, including disabled heads. A shared worker
    /// is visited once; inline contexts are each visited. A busy/failed store
    /// refuses the whole acknowledgment so the resource owner retries. False
    /// from one store says nothing about another store or remaining GPU uses.
    pub fn evict_shell_capture_imports(
        &mut self,
        image: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<(), crate::LiveRendererScanoutBufferExportDetail> {
        let stores = self
            .exporters
            .iter()
            .map(|exporter| exporter.image_store_identity())
            .collect::<Vec<_>>();
        evict_all_stores(stores, |index| {
            self.exporters[index]
                .evict_renderer_image_imports(image)
                .map(|_| ())
        })
    }
}

fn evict_all_stores<E>(
    stores: impl IntoIterator<Item = Option<u64>>,
    mut evict: impl FnMut(usize) -> Result<(), E>,
) -> Result<(), E> {
    let mut visited = BTreeSet::new();
    for (index, store) in stores.into_iter().enumerate() {
        if store.is_none_or(|identity| visited.insert(identity)) {
            evict(index)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../../tests/support/native_shell_capture_lifecycle.rs"]
mod tests;

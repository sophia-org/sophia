//! Capture retirement is maintenance, independent of future frame composition.
use super::*;
use sophia_renderer_live::{
    LiveRendererSnapshotBudget, LiveRendererSnapshotEpoch, LiveShellCaptureCustody,
    LiveShellCaptureInbox,
};
use std::{io, sync::Arc};

struct Install {
    output: OutputId,
    store: u64,
    generation: u64,
    epoch: Arc<LiveRendererSnapshotEpoch>,
}

#[derive(Default)]
pub(super) struct CaptureOwner {
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
}

impl LiveNativeShellCaptureEndpoint {
    pub fn output(&self) -> OutputId {
        self.output
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
        let generation = self.render_devices.generation;
        // Capture close invalidates only this epoch, not existing preview images.
        let epoch = LiveRendererSnapshotEpoch::new(device, generation, store);
        self.exporters[index].request_shell_capture(
            epoch.clone(),
            self.preview_images.budget.clone(),
            self.preview_images.capture.custody.clone(),
        )?;
        self.preview_images.capture.pending = Some(Install {
            output,
            store,
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
            Ok(Some(inbox))
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
                let endpoint = LiveNativeShellCaptureEndpoint {
                    inbox,
                    budget: self.preview_images.budget.clone(),
                    epoch: install.epoch.clone(),
                    output: install.output,
                };
                self.preview_images.capture.active = Some(install);
                Ok(Some(endpoint))
            }
            Ok(_) => {
                install.epoch.invalidate();
                // Keep the close obligation on queue pressure; caller retries close.
                self.preview_images.capture.active = Some(install);
                let _ = self.close_shell_capture();
                Err(io::ErrorKind::Interrupted.into())
            }
            Err(error) => {
                install.epoch.invalidate();
                Err(error)
            }
        }
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

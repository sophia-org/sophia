pub const NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeImageTransferStats {
    pub device_initializations: u64,
    pub attempts: u64,
    pub captures: u64,
    pub failures: u64,
    pub bridge_bytes: u64,
    pub bridge_allocations: u64,
    pub bridge_reuses: u64,
    pub bridge_busy: u64,
    pub bridge_live_slots: usize,
    pub bridge_live_bytes: u64,
    pub last_source_failure: Option<NativeGbmScanoutBufferExportDetail>,
    pub last_destination_failure: Option<NativeGbmScanoutBufferExportDetail>,
    pub max_capture_duration: std::time::Duration,
}

enum NativeImageImportDevice {
    Unopened(OwnedFd),
    Ready(Box<NativeGbmRenderedScanoutContext<OwnedFd>>),
    Unavailable,
}

fn image_import_failure(detail: NativeGbmScanoutBufferExportDetail) -> bool {
    matches!(
        detail,
        NativeGbmScanoutBufferExportDetail::DmaBufImageCreateFailed
            | NativeGbmScanoutBufferExportDetail::DmaBufImageBindFailed
            | NativeGbmScanoutBufferExportDetail::DmaBufImportFailed
    )
}

impl<T: AsFd> NativeGbmRenderedScanoutContext<T> {
    /// Latch backend-admitted devices for this worker incarnation. Contexts are
    /// opened lazily; no path discovery or device replacement occurs here.
    pub fn set_image_import_devices(
        &mut self,
        devices: Vec<OwnedFd>,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        if self.import_devices.is_some()
            || !self.renderer_images.is_empty()
            || devices.len() > NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY
        {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        self.import_devices = Some(
            devices
                .into_iter()
                .map(NativeImageImportDevice::Unopened)
                .collect(),
        );
        Ok(())
    }

    /// Replace only source import state between jobs. Busy scratch remains
    /// pinned; the caller may retry with fresh duplicates of its admitted FDs.
    pub fn replace_image_import_devices(
        &mut self,
        devices: Vec<OwnedFd>,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        if devices.len() > NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        self.poll_image_bridges()?;
        if self
            .image_bridges
            .iter()
            .any(|slot| slot.completion.is_some())
        {
            return Ok(false);
        }
        self.destroy_image_bridges();
        self.import_devices = Some(
            devices
                .into_iter()
                .map(NativeImageImportDevice::Unopened)
                .collect(),
        );
        self.transfer_sources.clear();
        Ok(true)
    }

    pub fn image_transfer_stats(&self) -> NativeImageTransferStats {
        NativeImageTransferStats {
            bridge_live_slots: self.image_bridges.len(),
            bridge_live_bytes: self.image_bridge_bytes(),
            ..self.transfer_stats
        }
    }

    fn transfer_renderer_image(
        &mut self,
        image_id: NativeRendererImageId,
        source: NativeMultiPlaneDmaBufFrame<'_>,
        direct_failure: NativeGbmScanoutBufferExportDetail,
    ) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
        let Some(mut devices) = self.import_devices.take() else {
            return Err(direct_failure);
        };
        let started = Instant::now();
        let result =
            self.transfer_renderer_image_on_devices(image_id, source, &mut devices, direct_failure);
        self.import_devices = Some(devices);
        if result.is_ok() {
            self.transfer_stats.captures = self.transfer_stats.captures.saturating_add(1);
        } else {
            self.transfer_stats.failures = self.transfer_stats.failures.saturating_add(1);
        }
        self.transfer_stats.max_capture_duration = self
            .transfer_stats
            .max_capture_duration
            .max(started.elapsed());
        result
    }

    fn transfer_renderer_image_on_devices(
        &mut self,
        image_id: NativeRendererImageId,
        source: NativeMultiPlaneDmaBufFrame<'_>,
        devices: &mut [NativeImageImportDevice],
        direct_failure: NativeGbmScanoutBufferExportDetail,
    ) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
        self.poll_image_bridges()?;
        let minimum_bytes = u64::from(source.width) * u64::from(source.height) * 4;
        if minimum_bytes
            .saturating_add(self.retained_renderer_image_bytes())
            .saturating_add(self.image_bridge_bytes())
            > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
        {
            return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
        }
        let layout = (source.format, source.modifier);
        let preferred = self.transfer_sources.get(&layout).copied();
        let order = image_transfer_policy::source_attempt_order(devices.len(), preferred);
        for index in order {
            let device = &mut devices[index];
            if matches!(device, NativeImageImportDevice::Unopened(_)) {
                let NativeImageImportDevice::Unopened(fd) =
                    std::mem::replace(device, NativeImageImportDevice::Unavailable)
                else {
                    unreachable!();
                };
                self.transfer_stats.device_initializations =
                    self.transfer_stats.device_initializations.saturating_add(1);
                if let Ok(context) = NativeGbmRenderedScanoutContext::new(fd, 1) {
                    *device = NativeImageImportDevice::Ready(Box::new(context));
                }
            }
            let NativeImageImportDevice::Ready(context) = device else {
                continue;
            };
            self.transfer_stats.attempts = self.transfer_stats.attempts.saturating_add(1);
            if let Err(detail) = context.probe_renderer_image_import(source) {
                self.transfer_stats.last_source_failure = Some(detail);
                if image_import_failure(detail) {
                    continue;
                }
                return Err(detail);
            }
            let selection = image_transfer_policy::select_bridge(
                self.image_bridges.iter().map(|slot| {
                    (
                        slot.completion.is_none(),
                        slot.source == index
                            && slot.bridge.buffer.width() == source.width
                            && slot.bridge.buffer.height() == source.height
                            && slot.bridge.buffer.format() == source.format,
                    )
                }),
                NATIVE_IMAGE_BRIDGE_CAPACITY,
            );
            let bridge = match selection {
                image_transfer_policy::BridgeSelection::Reuse(slot) => {
                    self.transfer_stats.bridge_reuses =
                        self.transfer_stats.bridge_reuses.saturating_add(1);
                    self.image_bridges.swap_remove(slot).bridge
                }
                image_transfer_policy::BridgeSelection::Deferred => {
                    self.transfer_stats.bridge_busy =
                        self.transfer_stats.bridge_busy.saturating_add(1);
                    return Err(NativeGbmScanoutBufferExportDetail::RendererImageTransferBusy);
                }
                image_transfer_policy::BridgeSelection::Replace(slot) => {
                    self.image_bridges.swap_remove(slot);
                    if minimum_bytes
                        .saturating_mul(2)
                        .saturating_add(self.retained_renderer_image_bytes())
                        .saturating_add(self.image_bridge_bytes())
                        > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
                    {
                        return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
                    }
                    let bridge = NativeLinearImageBridge::new(context, source)?;
                    self.transfer_stats.bridge_allocations =
                        self.transfer_stats.bridge_allocations.saturating_add(1);
                    bridge
                }
                image_transfer_policy::BridgeSelection::Allocate => {
                    if minimum_bytes
                        .saturating_mul(2)
                        .saturating_add(self.retained_renderer_image_bytes())
                        .saturating_add(self.image_bridge_bytes())
                        > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
                    {
                        return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
                    }
                    let bridge = NativeLinearImageBridge::new(context, source)?;
                    self.transfer_stats.bridge_allocations =
                        self.transfer_stats.bridge_allocations.saturating_add(1);
                    bridge
                }
            };
            let bridge_bytes = bridge.bytes;
            if bridge_bytes
                .saturating_add(minimum_bytes)
                .saturating_add(self.retained_renderer_image_bytes())
                .saturating_add(self.image_bridge_bytes())
                > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
            {
                return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
            }
            bridge.draw(source)?;
            let bridge_buffer = &bridge.buffer;
            let fds = bridge_buffer.export_plane_fds()?.into_plane_fds();
            let pitches = bridge_buffer.plane_pitches();
            let offsets = bridge_buffer.plane_offsets();
            let frame = NativeMultiPlaneDmaBufFrame {
                width: bridge_buffer.width(),
                height: bridge_buffer.height(),
                format: bridge_buffer.format(),
                modifier: 0,
                plane_count: bridge_buffer.plane_count(),
                planes: std::array::from_fn(|index| {
                    fds[index].as_ref().map(|fd| NativeDmaBufPlane {
                        fd: fd.as_fd(),
                        offset: offsets[index],
                        stride: pitches[index],
                    })
                }),
            };
            // The copy imports only temporary FDs. They close before the slot
            // returns to the pool; its completion fence then guards reuse.
            match self.render_renderer_image_snapshot_with_completion(image_id, frame, false, true)
            {
                Ok((buffer, completion)) => {
                    drop(fds);
                    self.image_bridges.push(NativeImageBridgeSlot {
                        source: index,
                        bridge,
                        completion,
                    });
                    let bytes = u64::from(buffer.pitch()) * u64::from(buffer.height());
                    if bytes
                        .saturating_add(self.image_bridge_bytes())
                        .saturating_add(self.retained_renderer_image_bytes())
                        > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
                    {
                        return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
                    }
                    self.transfer_stats.bridge_bytes = self
                        .transfer_stats
                        .bridge_bytes
                        .saturating_add(bridge_bytes);
                    if self.transfer_sources.len() < 64
                        || self.transfer_sources.contains_key(&layout)
                    {
                        self.transfer_sources.insert(layout, index);
                    }
                    return Ok(buffer);
                }
                Err(detail) if image_import_failure(detail) => {
                    self.transfer_stats.last_destination_failure = Some(detail);
                    continue;
                }
                Err(detail) => return Err(detail),
            }
        }
        Err(direct_failure)
    }
}

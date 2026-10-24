use sophia_renderer_image::{
    ManagedCaptureCharge, PreparedCaptureDestination, PreparedCaptureInfo, PreparedCaptureWatch,
};

#[derive(Clone)]
struct ManagedAllocationBacking {
    charge: ManagedCaptureCharge,
    epoch: std::sync::Arc<NativeRendererSnapshotEpoch>,
}

struct ManagedPreparation {
    allocation: std::rc::Rc<NativeCaptureAllocation>,
    watch: PreparedCaptureWatch,
    epoch: std::sync::Arc<NativeRendererSnapshotEpoch>,
}

impl NativeCaptureReuse {
    #[allow(clippy::too_many_arguments)]
    fn prepare_managed<T: AsFd>(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        gbm_device: &gbm::Device<T>,
        width: u32,
        height: u32,
        available: u64,
        limit: usize,
        budget: std::sync::Arc<NativeRendererSnapshotBudget>,
        epoch: std::sync::Arc<NativeRendererSnapshotEpoch>,
        wake: sophia_wake::Notifier,
    ) -> Result<Option<PreparedCaptureDestination>, NativeGbmScanoutBufferExportDetail> {
        use NativeGbmScanoutBufferExportDetail as E;
        if !self.enabled || !epoch.is_valid() {
            return Ok(None);
        }
        if self.source_cleanup_failed() {
            return Err(E::EglImageDestroyFailed);
        }
        self.maintain_managed_preparations();
        self.reap();
        let reusable = self.idle.iter().position(|allocation| {
            allocation.key.width == width
                && allocation.key.height == height
                && allocation.key.format == gbm::Format::Xrgb8888
                && allocation.managed.borrow().as_ref().is_some_and(|held| {
                    held.charge.belongs_to(&budget)
                        && std::sync::Arc::ptr_eq(&held.epoch, &epoch)
                        && held.epoch.is_valid()
                })
        });
        let allocation = if let Some(index) = reusable {
            self.stats.context_reuses = self.stats.context_reuses.saturating_add(1);
            self.stats.reuses = self.stats.reuses.saturating_add(1);
            self.idle.swap_remove(index)
        } else {
            let Some(allocation) = self.acquire_destination(
                egl,
                display,
                gbm_device,
                width,
                height,
                gbm::Format::Xrgb8888,
                available,
                limit,
                false,
            )?
            else {
                return Ok(None);
            };
            allocation
        };
        let bound = self
            .destination_bound(width, height)
            .ok_or(E::InvalidTarget)?;
        let charge = allocation
            .managed
            .borrow()
            .as_ref()
            .map(|held| held.charge.clone());
        let (token, watch) = PreparedCaptureDestination::new(
            PreparedCaptureInfo {
                allocation_id: allocation.id,
                width,
                height,
                bytes: allocation.bytes,
                bound,
            },
            budget,
            epoch.clone(),
            charge,
            wake,
        )?;
        self.managed_prepared.insert(
            allocation.id,
            ManagedPreparation {
                allocation,
                watch,
                epoch,
            },
        );
        Ok(Some(token))
    }

    fn managed_preparation_matches(&self, token: &PreparedCaptureDestination) -> bool {
        self.managed_prepared
            .get(&token.info().allocation_id)
            .is_some_and(|p| {
                p.watch.matches(token)
                    && p.allocation.bytes == token.info().bytes
                    && p.allocation.key.width == token.info().width
                    && p.allocation.key.height == token.info().height
            })
    }

    fn take_managed_preparation(
        &mut self,
        token: &PreparedCaptureDestination,
    ) -> Option<std::rc::Rc<NativeCaptureAllocation>> {
        if !self.managed_preparation_matches(token) || !token.consume() {
            return None;
        }
        self.stats.context_reuses = self.stats.context_reuses.saturating_add(1);
        self.stats.reuses = self.stats.reuses.saturating_add(1);
        self.managed_prepared
            .remove(&token.info().allocation_id)
            .map(|p| p.allocation)
    }

    fn maintain_managed_preparations(&mut self) {
        // A cancelled token has never submitted work. Its allocation can return
        // to idle; withdrawn endpoints are destroyed instead of displacing reuse.
        let cancelled: Vec<_> = self
            .managed_prepared
            .iter()
            .filter(|(_, p)| !p.watch.is_live())
            .map(|(id, _)| *id)
            .collect();
        for id in cancelled {
            let p = self
                .managed_prepared
                .remove(&id)
                .expect("selected preparation");
            self.idle.push(p.allocation);
        }
        self.trim_idle(NATIVE_CAPTURE_IDLE_CAPACITY, NATIVE_CAPTURE_IDLE_BYTES);
    }
}

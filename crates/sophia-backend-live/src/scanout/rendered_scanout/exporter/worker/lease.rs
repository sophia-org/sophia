#[derive(Debug)]
pub struct NativeGbmRendererWorkerScanoutLease {
    pub(super) capture: Option<super::LiveNativeFrameCapture>,
    descriptor: LiveRendererScanoutBufferDescriptor,
    correlation: LiveRendererFrameCorrelation,
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
    command_sender: CommandSender,
    release_enqueue_failures: Arc<AtomicUsize>,
}

impl NativeGbmRendererWorkerScanoutLease {
    pub(crate) fn capture_for(
        &self,
        expected: super::LiveNativeFrameIdentity,
    ) -> Option<super::LiveNativeFrameCapture> {
        let capture = self.capture.as_ref()?;
        (self.correlation.native == Some(expected) && capture.identity() == expected)
            .then(|| capture.clone())
    }
    pub const fn correlation(&self) -> LiveRendererFrameCorrelation {
        self.correlation
    }

    pub const fn descriptor(&self) -> LiveRendererScanoutBufferDescriptor {
        self.descriptor
    }

    pub const fn lease_id(&self) -> LiveRendererWorkerLeaseId {
        self.lease_id
    }

    pub const fn slot_token(&self) -> LiveRendererFrameSlotToken {
        self.slot_token
    }

    pub fn export_scanout_dma_buf_fds(
        &self,
    ) -> io::Result<Option<(u8, [Option<std::os::fd::OwnedFd>; 4])>> {
        Ok(None)
    }
}

impl Drop for NativeGbmRendererWorkerScanoutLease {
    fn drop(&mut self) {
        if self
            .command_sender
            .try_send(WorkerCommand::Release {
                output: self.output,
                lease_id: self.lease_id,
                slot_token: self.slot_token,
            })
            .is_err()
        {
            self.release_enqueue_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

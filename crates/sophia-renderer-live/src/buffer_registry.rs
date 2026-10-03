use std::collections::BTreeMap;
use std::os::fd::OwnedFd;

use sophia_protocol::{
    BufferHandle, BufferSource, DmaBufDescriptor, DmaBufDescriptorError, FenceHandle, SurfaceId,
    TransactionId,
};

pub const LIVE_PRESENTATION_REGISTRY_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveBufferState {
    WaitingForAcquireFence,
    Ready,
    Submitted,
}

#[derive(Debug)]
pub struct LiveDmaBufRegistration {
    pub descriptor: DmaBufDescriptor,
    pub plane_fds: Vec<OwnedFd>,
    pub acquire_fence: Option<OwnedFd>,
    pub state: LiveBufferState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveBufferRegistryError {
    InvalidDescriptor(DmaBufDescriptorError),
    PlaneFdCountMismatch,
    DuplicateHandle,
    UnknownHandle,
    AcquireFencePending,
    AlreadySubmitted,
    FenceQueryFailed,
    FdCloneFailed,
    DuplicateFence,
    UnknownFence,
    SourceInUse,
    DuplicatePresentation,
    UnknownPresentation,
    CapacityExceeded,
    SourceReleasePending,
    FenceReleasePending,
}

impl std::fmt::Display for LiveBufferRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LiveBufferRegistryError {}

#[derive(Debug, Default)]
pub struct LiveBufferRegistry {
    buffers: BTreeMap<BufferHandle, LiveDmaBufRegistration>,
}

#[derive(Debug)]
struct LiveDmaBufSourceRegistration {
    descriptor: DmaBufDescriptor,
    plane_fds: Vec<OwnedFd>,
    references: usize,
    release_pending: bool,
}

#[derive(Debug)]
struct LiveFenceSourceRegistration {
    fd: OwnedFd,
    references: usize,
    release_pending: bool,
}

#[derive(Debug)]
struct LivePresentRegistration {
    source: Option<BufferHandle>,
    acquire_fence: Option<FenceHandle>,
    idle_fence: Option<FenceHandle>,
    plane_fds: Vec<OwnedFd>,
    acquire_fence_fd: Option<OwnedFd>,
    idle_fence_fd: Option<OwnedFd>,
    state: LiveBufferState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivePresentationRegistryLimits {
    pub sources: usize,
    pub fences: usize,
    pub presentations: usize,
}

impl Default for LivePresentationRegistryLimits {
    fn default() -> Self {
        Self {
            sources: LIVE_PRESENTATION_REGISTRY_CAPACITY,
            fences: LIVE_PRESENTATION_REGISTRY_CAPACITY,
            presentations: LIVE_PRESENTATION_REGISTRY_CAPACITY,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveResourceReleaseStatus {
    Unknown,
    Deferred,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveIdleFenceStatus {
    NotRequested,
    Triggered,
    TriggerFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePresentationRetirement {
    pub transaction: TransactionId,
    pub source: BufferSource,
    pub idle_fence: LiveIdleFenceStatus,
    pub released_source: bool,
    pub released_fences: Vec<FenceHandle>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LivePresentationDisconnectReport {
    pub retired_presentations: usize,
    pub triggered_idle_fences: usize,
    pub failed_idle_fences: usize,
    pub released_sources: Vec<BufferSource>,
    pub released_fences: Vec<FenceHandle>,
}

/// Renderer-private reusable DMA-BUF sources plus their in-flight Present
/// lifetimes.
///
/// DRI3 pixmaps outlive any single Present request. Source plane descriptors
/// therefore remain registered while each presentation receives its own
/// duplicated plane and fence ownership in a transaction-keyed presentation. A
/// page-flip retirement drops only that presentation ownership, allowing the
/// same pixmap to be presented again without exposing native FDs outside the
/// renderer boundary.
#[derive(Debug)]
pub struct LiveDmaBufPresentationRegistry {
    sources: BTreeMap<BufferHandle, LiveDmaBufSourceRegistration>,
    fences: BTreeMap<FenceHandle, LiveFenceSourceRegistration>,
    presentations: BTreeMap<TransactionId, LivePresentRegistration>,
    limits: LivePresentationRegistryLimits,
    max_sources: usize,
    max_fences: usize,
    max_presentations: usize,
}

impl Default for LiveDmaBufPresentationRegistry {
    fn default() -> Self {
        Self::with_limits(LivePresentationRegistryLimits::default())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingCpuPresentation {
    handle: u64,
    previous_committed_handle: Option<u64>,
}

#[derive(Debug, Default)]
pub struct LiveCpuBufferLifetimeRegistry {
    committed: BTreeMap<SurfaceId, u64>,
    pending: BTreeMap<SurfaceId, PendingCpuPresentation>,
}

impl LiveCpuBufferLifetimeRegistry {
    pub fn submit(&mut self, surface: SurfaceId, handle: u64) -> Option<BufferSource> {
        let replaced_pending = self.pending.insert(
            surface,
            PendingCpuPresentation {
                handle,
                previous_committed_handle: self.committed.get(&surface).copied(),
            },
        );
        replaced_pending.map(|pending| BufferSource::CpuBuffer {
            handle: pending.handle,
        })
    }

    pub fn retire_page_flip(&mut self, surface: SurfaceId, handle: u64) -> Vec<BufferSource> {
        let Some(pending) = self.pending.get(&surface).copied() else {
            return Vec::new();
        };
        if pending.handle != handle {
            return Vec::new();
        }
        self.pending.remove(&surface);
        self.committed.insert(surface, handle);
        pending
            .previous_committed_handle
            .filter(|previous| *previous != handle)
            .map(|previous| vec![BufferSource::CpuBuffer { handle: previous }])
            .unwrap_or_default()
    }

    pub fn reject(&mut self, surface: SurfaceId, handle: u64) -> Option<BufferSource> {
        let matches = self
            .pending
            .get(&surface)
            .is_some_and(|pending| pending.handle == handle);
        matches.then(|| {
            self.pending.remove(&surface);
            BufferSource::CpuBuffer { handle }
        })
    }

    pub fn committed_handle(&self, surface: SurfaceId) -> Option<u64> {
        self.committed.get(&surface).copied()
    }

    pub fn disconnect(&mut self) -> Vec<BufferSource> {
        let mut handles = self
            .pending
            .values()
            .map(|pending| pending.handle)
            .chain(self.committed.values().copied())
            .collect::<Vec<_>>();
        handles.sort_unstable();
        handles.dedup();
        self.pending.clear();
        self.committed.clear();
        handles
            .into_iter()
            .map(|handle| BufferSource::CpuBuffer { handle })
            .collect()
    }
}

impl LiveBufferRegistry {
    pub fn register(
        &mut self,
        descriptor: DmaBufDescriptor,
        plane_fds: Vec<OwnedFd>,
        acquire_fence: Option<OwnedFd>,
    ) -> Result<(), LiveBufferRegistryError> {
        descriptor
            .validate()
            .map_err(LiveBufferRegistryError::InvalidDescriptor)?;
        if plane_fds.len() != usize::from(descriptor.plane_count) {
            return Err(LiveBufferRegistryError::PlaneFdCountMismatch);
        }
        if self.buffers.contains_key(&descriptor.handle) {
            return Err(LiveBufferRegistryError::DuplicateHandle);
        }
        let state = if acquire_fence.is_some() {
            LiveBufferState::WaitingForAcquireFence
        } else {
            LiveBufferState::Ready
        };
        self.buffers.insert(
            descriptor.handle,
            LiveDmaBufRegistration {
                descriptor,
                plane_fds,
                acquire_fence,
                state,
            },
        );
        Ok(())
    }

    pub fn signal_acquire_fence(
        &mut self,
        handle: BufferHandle,
    ) -> Result<(), LiveBufferRegistryError> {
        let buffer = self
            .buffers
            .get_mut(&handle)
            .ok_or(LiveBufferRegistryError::UnknownHandle)?;
        buffer.acquire_fence.take();
        if buffer.state == LiveBufferState::WaitingForAcquireFence {
            buffer.state = LiveBufferState::Ready;
        }
        Ok(())
    }

    pub fn poll_acquire_fence(
        &mut self,
        handle: BufferHandle,
    ) -> Result<bool, LiveBufferRegistryError> {
        let buffer = self
            .buffers
            .get(&handle)
            .ok_or(LiveBufferRegistryError::UnknownHandle)?;
        if buffer.state != LiveBufferState::WaitingForAcquireFence {
            return Ok(true);
        }
        let signaled = sophia_xshmfence::query(
            buffer
                .acquire_fence
                .as_ref()
                .ok_or(LiveBufferRegistryError::FenceQueryFailed)?,
        )
        .map_err(|_| LiveBufferRegistryError::FenceQueryFailed)?;
        if signaled {
            self.signal_acquire_fence(handle)?;
        }
        Ok(signaled)
    }

    pub fn submit(&mut self, handle: BufferHandle) -> Result<(), LiveBufferRegistryError> {
        let buffer = self
            .buffers
            .get_mut(&handle)
            .ok_or(LiveBufferRegistryError::UnknownHandle)?;
        match buffer.state {
            LiveBufferState::WaitingForAcquireFence => {
                Err(LiveBufferRegistryError::AcquireFencePending)
            }
            LiveBufferState::Ready => {
                buffer.state = LiveBufferState::Submitted;
                Ok(())
            }
            LiveBufferState::Submitted => Err(LiveBufferRegistryError::AlreadySubmitted),
        }
    }

    pub fn retire_page_flip(&mut self, handle: BufferHandle) -> Option<BufferSource> {
        let submitted = self
            .buffers
            .get(&handle)
            .is_some_and(|buffer| buffer.state == LiveBufferState::Submitted);
        submitted.then(|| {
            self.buffers.remove(&handle);
            BufferSource::DmaBuf {
                handle: handle.raw(),
            }
        })
    }

    pub fn reject(&mut self, handle: BufferHandle) -> Option<BufferSource> {
        self.buffers.remove(&handle).map(|_| BufferSource::DmaBuf {
            handle: handle.raw(),
        })
    }

    pub fn disconnect(&mut self) -> Vec<BufferSource> {
        let handles = self.buffers.keys().copied().collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| self.reject(handle))
            .collect()
    }

    pub fn state(&self, handle: BufferHandle) -> Option<LiveBufferState> {
        self.buffers.get(&handle).map(|buffer| buffer.state)
    }

    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

impl LiveDmaBufPresentationRegistry {
    pub fn with_limits(limits: LivePresentationRegistryLimits) -> Self {
        Self {
            sources: BTreeMap::new(),
            fences: BTreeMap::new(),
            presentations: BTreeMap::new(),
            limits,
            max_sources: 0,
            max_fences: 0,
            max_presentations: 0,
        }
    }

    pub fn register_source(
        &mut self,
        descriptor: DmaBufDescriptor,
        plane_fds: Vec<OwnedFd>,
    ) -> Result<(), LiveBufferRegistryError> {
        descriptor
            .validate()
            .map_err(LiveBufferRegistryError::InvalidDescriptor)?;
        if plane_fds.len() != usize::from(descriptor.plane_count) {
            return Err(LiveBufferRegistryError::PlaneFdCountMismatch);
        }
        if self.sources.contains_key(&descriptor.handle) {
            return Err(LiveBufferRegistryError::DuplicateHandle);
        }
        if self.sources.len() >= self.limits.sources {
            return Err(LiveBufferRegistryError::CapacityExceeded);
        }
        self.sources.insert(
            descriptor.handle,
            LiveDmaBufSourceRegistration {
                descriptor,
                plane_fds,
                references: 0,
                release_pending: false,
            },
        );
        self.max_sources = self.max_sources.max(self.sources.len());
        Ok(())
    }

    pub fn register_fence(
        &mut self,
        fence: FenceHandle,
        _initially_triggered: bool,
        fd: OwnedFd,
    ) -> Result<(), LiveBufferRegistryError> {
        if self.fences.contains_key(&fence) {
            return Err(LiveBufferRegistryError::DuplicateFence);
        }
        if self.fences.len() >= self.limits.fences {
            return Err(LiveBufferRegistryError::CapacityExceeded);
        }
        self.fences.insert(
            fence,
            LiveFenceSourceRegistration {
                fd,
                references: 0,
                release_pending: false,
            },
        );
        self.max_fences = self.max_fences.max(self.fences.len());
        Ok(())
    }

    pub fn begin_present(
        &mut self,
        transaction: TransactionId,
        handle: BufferHandle,
        acquire_fence: Option<FenceHandle>,
        idle_fence: Option<FenceHandle>,
    ) -> Result<(), LiveBufferRegistryError> {
        self.begin_present_with_optional_source(
            transaction,
            Some(handle),
            acquire_fence,
            idle_fence,
        )
    }

    pub fn begin_software_present(
        &mut self,
        transaction: TransactionId,
        acquire_fence: Option<FenceHandle>,
        idle_fence: Option<FenceHandle>,
    ) -> Result<(), LiveBufferRegistryError> {
        self.begin_present_with_optional_source(transaction, None, acquire_fence, idle_fence)
    }

    fn begin_present_with_optional_source(
        &mut self,
        transaction: TransactionId,
        handle: Option<BufferHandle>,
        acquire_fence: Option<FenceHandle>,
        idle_fence: Option<FenceHandle>,
    ) -> Result<(), LiveBufferRegistryError> {
        if self.presentations.contains_key(&transaction) {
            return Err(LiveBufferRegistryError::DuplicatePresentation);
        }
        if self.presentations.len() >= self.limits.presentations {
            return Err(LiveBufferRegistryError::CapacityExceeded);
        }
        let plane_fds = handle.map_or(Ok(Vec::new()), |handle| {
            let source = self
                .sources
                .get(&handle)
                .ok_or(LiveBufferRegistryError::UnknownHandle)?;
            if source.release_pending {
                return Err(LiveBufferRegistryError::SourceReleasePending);
            }
            source
                .plane_fds
                .iter()
                .map(OwnedFd::try_clone)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| LiveBufferRegistryError::FdCloneFailed)
        })?;
        let acquire_fence_fd = acquire_fence
            .map(|fence| {
                let fence = self
                    .fences
                    .get(&fence)
                    .ok_or(LiveBufferRegistryError::UnknownFence)?;
                if fence.release_pending {
                    return Err(LiveBufferRegistryError::FenceReleasePending);
                }
                fence
                    .fd
                    .try_clone()
                    .map(Some)
                    .map_err(|_| LiveBufferRegistryError::FdCloneFailed)
            })
            .transpose()?
            .flatten();
        let idle_fence_fd = idle_fence
            .map(|fence| {
                let fence = self
                    .fences
                    .get(&fence)
                    .ok_or(LiveBufferRegistryError::UnknownFence)?;
                if fence.release_pending {
                    return Err(LiveBufferRegistryError::FenceReleasePending);
                }
                fence
                    .fd
                    .try_clone()
                    .map_err(|_| LiveBufferRegistryError::FdCloneFailed)
            })
            .transpose()?;
        let acquire_ready = acquire_fence_fd
            .as_ref()
            .map(|fd| {
                sophia_xshmfence::query(fd).map_err(|_| LiveBufferRegistryError::FenceQueryFailed)
            })
            .transpose()?
            .unwrap_or(true);
        let acquire_fence_fd = (!acquire_ready).then_some(acquire_fence_fd).flatten();

        if let Some(handle) = handle {
            self.sources
                .get_mut(&handle)
                .expect("validated source must remain registered")
                .references += 1;
        }
        for fence in [acquire_fence, idle_fence].into_iter().flatten() {
            self.fences
                .get_mut(&fence)
                .expect("validated fence must remain registered")
                .references += 1;
        }
        self.presentations.insert(
            transaction,
            LivePresentRegistration {
                source: handle,
                acquire_fence,
                idle_fence,
                plane_fds,
                acquire_fence_fd,
                idle_fence_fd,
                state: if acquire_ready {
                    LiveBufferState::Ready
                } else {
                    LiveBufferState::WaitingForAcquireFence
                },
            },
        );
        self.max_presentations = self.max_presentations.max(self.presentations.len());
        Ok(())
    }

    pub fn poll_acquire_fence(
        &mut self,
        transaction: TransactionId,
    ) -> Result<bool, LiveBufferRegistryError> {
        let presentation = self
            .presentations
            .get_mut(&transaction)
            .ok_or(LiveBufferRegistryError::UnknownPresentation)?;
        if presentation.state != LiveBufferState::WaitingForAcquireFence {
            return Ok(true);
        }
        let signaled = sophia_xshmfence::query(
            presentation
                .acquire_fence_fd
                .as_ref()
                .ok_or(LiveBufferRegistryError::FenceQueryFailed)?,
        )
        .map_err(|_| LiveBufferRegistryError::FenceQueryFailed)?;
        if signaled {
            presentation.acquire_fence_fd.take();
            presentation.state = LiveBufferState::Ready;
        }
        Ok(signaled)
    }

    pub fn submit(&mut self, transaction: TransactionId) -> Result<(), LiveBufferRegistryError> {
        let presentation = self
            .presentations
            .get_mut(&transaction)
            .ok_or(LiveBufferRegistryError::UnknownPresentation)?;
        match presentation.state {
            LiveBufferState::WaitingForAcquireFence => {
                Err(LiveBufferRegistryError::AcquireFencePending)
            }
            LiveBufferState::Ready => {
                presentation.state = LiveBufferState::Submitted;
                Ok(())
            }
            LiveBufferState::Submitted => Err(LiveBufferRegistryError::AlreadySubmitted),
        }
    }

    pub fn retire_page_flip(
        &mut self,
        transaction: TransactionId,
    ) -> Option<LivePresentationRetirement> {
        let submitted = self
            .presentations
            .get(&transaction)
            .is_some_and(|presentation| presentation.state == LiveBufferState::Submitted);
        submitted.then(|| self.finish_present(transaction))
    }

    pub fn reject(&mut self, transaction: TransactionId) -> Option<LivePresentationRetirement> {
        self.presentations
            .contains_key(&transaction)
            .then(|| self.finish_present(transaction))
    }

    pub fn remove_source(&mut self, handle: BufferHandle) -> LiveResourceReleaseStatus {
        let Some(source) = self.sources.get_mut(&handle) else {
            return LiveResourceReleaseStatus::Unknown;
        };
        if source.references > 0 {
            source.release_pending = true;
            return LiveResourceReleaseStatus::Deferred;
        }
        self.sources.remove(&handle);
        LiveResourceReleaseStatus::Released
    }

    pub fn remove_fence(&mut self, fence: FenceHandle) -> LiveResourceReleaseStatus {
        let Some(registration) = self.fences.get_mut(&fence) else {
            return LiveResourceReleaseStatus::Unknown;
        };
        if registration.references > 0 {
            registration.release_pending = true;
            return LiveResourceReleaseStatus::Deferred;
        }
        self.fences.remove(&fence);
        LiveResourceReleaseStatus::Released
    }

    pub fn descriptor(&self, handle: BufferHandle) -> Option<DmaBufDescriptor> {
        self.sources.get(&handle).map(|source| source.descriptor)
    }

    pub fn try_clone_presentation_plane_fd(
        &self,
        transaction: TransactionId,
        plane: usize,
    ) -> Result<OwnedFd, LiveBufferRegistryError> {
        self.presentations
            .get(&transaction)
            .ok_or(LiveBufferRegistryError::UnknownPresentation)?
            .plane_fds
            .get(plane)
            .ok_or(LiveBufferRegistryError::PlaneFdCountMismatch)?
            .try_clone()
            .map_err(|_| LiveBufferRegistryError::FdCloneFailed)
    }

    pub fn state(&self, transaction: TransactionId) -> Option<LiveBufferState> {
        self.presentations
            .get(&transaction)
            .map(|presentation| presentation.state)
    }

    pub fn source_for_presentation(&self, transaction: TransactionId) -> Option<BufferHandle> {
        self.presentations
            .get(&transaction)
            .and_then(|presentation| presentation.source)
    }

    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    pub fn fence_count(&self) -> usize {
        self.fences.len()
    }

    pub fn presentation_capacity(&self) -> usize {
        self.limits.presentations
    }

    pub fn presentation_count(&self) -> usize {
        self.presentations.len()
    }

    pub const fn max_source_count(&self) -> usize {
        self.max_sources
    }

    pub const fn max_fence_count(&self) -> usize {
        self.max_fences
    }

    pub const fn max_presentation_count(&self) -> usize {
        self.max_presentations
    }

    pub fn disconnect(&mut self) -> LivePresentationDisconnectReport {
        let mut report = LivePresentationDisconnectReport::default();
        let transactions = self.presentations.keys().copied().collect::<Vec<_>>();
        for transaction in transactions {
            let retirement = self.finish_present(transaction);
            report.retired_presentations += 1;
            match retirement.idle_fence {
                LiveIdleFenceStatus::Triggered => report.triggered_idle_fences += 1,
                LiveIdleFenceStatus::TriggerFailed => report.failed_idle_fences += 1,
                LiveIdleFenceStatus::NotRequested => {}
            }
            if retirement.released_source {
                report.released_sources.push(retirement.source);
            }
            report
                .released_fences
                .extend(retirement.released_fences.iter().copied());
        }
        let handles = self.sources.keys().copied().collect::<Vec<_>>();
        let fences = self.fences.keys().copied().collect::<Vec<_>>();
        self.sources.clear();
        self.fences.clear();
        report
            .released_sources
            .extend(handles.into_iter().map(|handle| BufferSource::DmaBuf {
                handle: handle.raw(),
            }));
        report.released_fences.extend(fences);
        report
    }

    fn finish_present(&mut self, transaction: TransactionId) -> LivePresentationRetirement {
        let presentation = self
            .presentations
            .remove(&transaction)
            .expect("known presentation must remain registered until retirement");
        let idle_fence = match presentation.idle_fence_fd.as_ref() {
            None => LiveIdleFenceStatus::NotRequested,
            Some(fd) if sophia_xshmfence::trigger(fd).is_ok() => LiveIdleFenceStatus::Triggered,
            Some(_) => LiveIdleFenceStatus::TriggerFailed,
        };

        let (source, released_source) =
            presentation
                .source
                .map_or((BufferSource::None, false), |handle| {
                    let registration = self
                        .sources
                        .get_mut(&handle)
                        .expect("presentation source must remain registered");
                    registration.references = registration.references.saturating_sub(1);
                    let released = registration.references == 0 && registration.release_pending;
                    if released {
                        self.sources.remove(&handle);
                    }
                    (
                        BufferSource::DmaBuf {
                            handle: handle.raw(),
                        },
                        released,
                    )
                });

        let mut released_fences = Vec::new();
        for fence in [presentation.acquire_fence, presentation.idle_fence]
            .into_iter()
            .flatten()
        {
            let Some(registration) = self.fences.get_mut(&fence) else {
                continue;
            };
            registration.references = registration.references.saturating_sub(1);
            if registration.references == 0 && registration.release_pending {
                self.fences.remove(&fence);
                released_fences.push(fence);
            }
        }

        LivePresentationRetirement {
            transaction,
            source,
            idle_fence,
            released_source,
            released_fences,
        }
    }
}

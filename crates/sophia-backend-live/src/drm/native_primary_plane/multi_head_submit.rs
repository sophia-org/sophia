use crate::prelude::*;

/// What a caller intends a topology submission to do.
#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTopologySubmitIntent {
    /// Ask the kernel whether the topology is valid and change nothing.
    Validate,
    /// Apply the topology for real.
    Activate,
}

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTopologySubmitOutcome {
    Accepted,
    /// The device could not take the request now. Retrying later is legitimate.
    Busy,
    /// The kernel refused the topology.
    Rejected,
    /// The heads could not be composed into a request, so nothing was submitted.
    Unbuildable(LibdrmNativeMultiHeadRequestBuildStatus),
}

/// Submits one topology as a single atomic request.
///
/// `Validate` sets `TEST_ONLY`, which is the whole reason a caller can check a
/// topology against real hardware without risking the desktop. Both intents set
/// `ALLOW_MODESET`, because changing a topology is by definition a modeset.
///
/// An unbuildable head set never reaches the device. Reporting that separately
/// from a kernel rejection matters: one is a mistake in what was asked for, the
/// other is hardware declining something well-formed.
#[cfg(feature = "libdrm-events")]
pub fn submit_native_multi_head_topology<D>(
    committer: &mut NativeLibdrmAtomicScanoutCommitter<D>,
    heads: &[LibdrmNativeAtomicHead],
    intent: NativeTopologySubmitIntent,
) -> NativeTopologySubmitOutcome
where
    D: LibdrmNativeAtomicCommitDevice,
{
    let build = build_native_multi_head_atomic_request(
        heads,
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );
    if build.status != LibdrmNativeMultiHeadRequestBuildStatus::Built {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    }
    let Some(request) = build.request else {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    };
    let request = match intent {
        NativeTopologySubmitIntent::Validate => request.test_only().allow_modeset(),
        NativeTopologySubmitIntent::Activate => request.allow_modeset(),
    };
    submit(committer, request)
}

/// Submits one topology against a device directly.
///
/// A running session holds a borrowed card, not a committer, and a committer takes
/// its device by value. Rather than clone a card so a counter can be incremented,
/// the device is the primitive and the committer form stays for callers that want
/// the submit and reject tallies.
#[cfg(feature = "libdrm-events")]
pub fn submit_native_multi_head_topology_on_device<D>(
    device: &D,
    heads: &[LibdrmNativeAtomicHead],
    intent: NativeTopologySubmitIntent,
) -> NativeTopologySubmitOutcome
where
    D: LibdrmNativeAtomicCommitDevice,
{
    let build = build_native_multi_head_atomic_request(
        heads,
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );
    if build.status != LibdrmNativeMultiHeadRequestBuildStatus::Built {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    }
    let Some(request) = build.request else {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    };
    let request = match intent {
        NativeTopologySubmitIntent::Validate => request.test_only().allow_modeset(),
        // A modeset applied for real must complete before the caller can believe it
        // did, and it carries no page-flip event to wait on, so it blocks.
        NativeTopologySubmitIntent::Activate => {
            request.allow_modeset().without_page_flip_event().blocking()
        }
    };
    let (flags, native) = request.into_native();
    match device.submit_atomic_commit(flags, native) {
        Ok(()) => NativeTopologySubmitOutcome::Accepted,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            NativeTopologySubmitOutcome::Busy
        }
        Err(_) => NativeTopologySubmitOutcome::Rejected,
    }
}

/// Applies one complete card-scoped topology change with independently owned
/// enabled framebuffers and explicit disabled heads.
///
/// This is the execution counterpart to
/// [`build_native_topology_change_atomic_request`]. The request is deliberately
/// blocking and carries no page-flip event: returning `Accepted` means the card
/// has completed its modeset, so a userspace coordinator may advance to the next
/// card or begin rollback without racing an outstanding commit on this card.
#[cfg(feature = "libdrm-events")]
pub fn submit_native_topology_change_on_device<D>(
    device: &D,
    changes: &[LibdrmNativeAtomicTopologyChange],
) -> NativeTopologySubmitOutcome
where
    D: LibdrmNativeAtomicCommitDevice,
{
    let build = build_native_topology_change_atomic_request(changes);
    if build.status != LibdrmNativeMultiHeadRequestBuildStatus::Built {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    }
    let Some(request) = build.request else {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    };
    let (flags, native) = request
        .allow_modeset()
        .without_page_flip_event()
        .blocking()
        .into_native();
    match device.submit_atomic_commit(flags, native) {
        Ok(()) => NativeTopologySubmitOutcome::Accepted,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            NativeTopologySubmitOutcome::Busy
        }
        Err(_) => NativeTopologySubmitOutcome::Rejected,
    }
}

/// Validates one topology against real hardware without naming a framebuffer.
///
/// There is no `Activate` counterpart on purpose. A topology request carries no
/// plane state, so applying it would leave CRTCs active with nothing to scan out.
/// Validation is the only thing this shape is for, which is why the intent is
/// fixed here rather than passed in.
#[cfg(feature = "libdrm-events")]
pub fn validate_native_multi_head_topology<D>(
    committer: &mut NativeLibdrmAtomicScanoutCommitter<D>,
    heads: &[LibdrmNativeAtomicTopologyHead],
) -> NativeTopologySubmitOutcome
where
    D: LibdrmNativeAtomicCommitDevice,
{
    validate_native_multi_head_topology_on_device(committer.device(), heads)
}

/// Validates a plane-less topology against a device directly. Diagnostic only.
///
/// A request with no plane state is judged against whatever the planes hold
/// now, and amdgpu refuses any enabled CRTC whose primary plane is off. A
/// session must validate with `validate_native_complete_topology_on_device`.
///
/// A validation is not a commit: nothing is scheduled, nothing retires, and the
/// committer's submit and reject counters would describe work that never happened.
/// A caller holding only a borrowed card — which is what a running session has —
/// also has no committer to lend. Both point the same way, so the device is the
/// primitive here and the committer form delegates to it.
#[cfg(feature = "libdrm-events")]
pub fn validate_native_multi_head_topology_on_device<D>(
    device: &D,
    heads: &[LibdrmNativeAtomicTopologyHead],
) -> NativeTopologySubmitOutcome
where
    D: LibdrmNativeAtomicCommitDevice,
{
    let build = build_native_multi_head_topology_request(heads);
    if build.status != LibdrmNativeMultiHeadRequestBuildStatus::Built {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    }
    let Some(request) = build.request else {
        return NativeTopologySubmitOutcome::Unbuildable(build.status);
    };
    let (flags, native) = request.test_only().allow_modeset().into_native();
    match device.submit_atomic_commit(flags, native) {
        Ok(()) => NativeTopologySubmitOutcome::Accepted,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            NativeTopologySubmitOutcome::Busy
        }
        Err(_) => NativeTopologySubmitOutcome::Rejected,
    }
}

#[cfg(feature = "libdrm-events")]
fn submit<D>(
    committer: &mut NativeLibdrmAtomicScanoutCommitter<D>,
    request: LibdrmNativeAtomicCommitRequest,
) -> NativeTopologySubmitOutcome
where
    D: LibdrmNativeAtomicCommitDevice,
{
    match committer.submit_native_atomic_commit(request).status {
        LibdrmNativeAtomicCommitSubmitStatus::Submitted => NativeTopologySubmitOutcome::Accepted,
        LibdrmNativeAtomicCommitSubmitStatus::WouldBlock => NativeTopologySubmitOutcome::Busy,
        LibdrmNativeAtomicCommitSubmitStatus::Rejected => NativeTopologySubmitOutcome::Rejected,
    }
}

/// What the kernel said about one complete topology test, with its errno.
#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeTopologyValidation {
    pub outcome: NativeTopologySubmitOutcome,
    /// The kernel's errno for `Busy` and `Rejected`; zero otherwise or when the
    /// kernel reported none, which cannot collide because errno 0 is success.
    pub errno: i32,
}

/// Validates a complete topology, primary plane state included, and changes
/// nothing.
///
/// Every head names its own framebuffer, so the answer does not depend on what
/// a previous owner left bound. The request is `TEST_ONLY | ALLOW_MODESET`
/// without a page-flip event; there is no apply form of this function.
/// `EAGAIN` and `EBUSY` mean the device could not take the request now.
#[cfg(feature = "libdrm-events")]
pub fn validate_native_complete_topology_on_device<D>(
    device: &D,
    heads: &[LibdrmNativeAtomicHead],
) -> NativeTopologyValidation
where
    D: LibdrmNativeAtomicCommitDevice,
{
    let build = build_native_multi_head_atomic_request(
        heads,
        LibdrmNativeAtomicCommitRequestScope::Modeset,
    );
    let Some(request) = build
        .request
        .filter(|_| build.status == LibdrmNativeMultiHeadRequestBuildStatus::Built)
    else {
        return NativeTopologyValidation {
            outcome: NativeTopologySubmitOutcome::Unbuildable(build.status),
            errno: 0,
        };
    };
    let (flags, native) = request
        .without_page_flip_event()
        .test_only()
        .allow_modeset()
        .into_native();
    classify_validation(device.submit_atomic_commit(flags, native))
}

#[cfg(feature = "libdrm-events")]
fn classify_validation(result: io::Result<()>) -> NativeTopologyValidation {
    const EAGAIN: i32 = 11;
    const EBUSY: i32 = 16;
    let Err(error) = result else {
        return NativeTopologyValidation {
            outcome: NativeTopologySubmitOutcome::Accepted,
            errno: 0,
        };
    };
    let errno = error.raw_os_error().unwrap_or(0);
    let busy = matches!(errno, EAGAIN | EBUSY)
        || matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::ResourceBusy
        );
    NativeTopologyValidation {
        outcome: if busy {
            NativeTopologySubmitOutcome::Busy
        } else {
            NativeTopologySubmitOutcome::Rejected
        },
        errno,
    }
}

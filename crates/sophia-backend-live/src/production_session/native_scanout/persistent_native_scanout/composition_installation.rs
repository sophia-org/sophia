//! Borrowed installation checks before a queued generation transfers any owner.
use super::composition_queue::LiveProductionQueuedMirrorGeneration;
use super::*;

/// Current facts captured by the native owner, independently of the queued payload.
/// This is a call-local validation view, not another ownership ledger.
pub(crate) struct NativeCompositionInstallationHead {
    pub index: usize,
    pub identity: crate::LiveNativeFrameIdentity,
    pub prepared_cleanup_available: bool,
    pub protected_frames: [Option<LiveProductionNativeFrameId>; 3],
}

pub(crate) fn validate_composition_installation(
    generation: &LiveProductionQueuedMirrorGeneration,
    current: &[NativeCompositionInstallationHead],
) -> Result<(), &'static str> {
    if generation.heads.is_empty()
        || generation
            .heads
            .iter()
            .any(|head| head.content.frame() != generation.frame)
    {
        return Err("mirror generation has invalid or mismatched frame identity");
    }
    if current.len() != generation.heads.len()
        || current
            .iter()
            .zip(&generation.heads)
            .any(|(current, queued)| current.index != queued.head_index)
    {
        return Err("mirror generation does not cover every physical head exactly once");
    }
    for (current, queued) in current.iter().zip(&generation.heads) {
        if current.identity != queued.identity {
            return Err("mirror generation does not name the current native targets");
        }
        if !current.prepared_cleanup_available {
            return Err("mirror generation waits for prepared-owner cleanup capacity");
        }
        if current
            .protected_frames
            .iter()
            .flatten()
            .any(|old| *old != generation.frame)
        {
            return Err("composition installation waits for an existing distinct retirement");
        }
    }
    Ok(())
}

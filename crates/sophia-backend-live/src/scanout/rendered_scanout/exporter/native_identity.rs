//! Native ownership identity is minted independently of render requests and pixels.

use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

/// One construction of a native owner. Rebuilding targets never recycles this scope.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct NativeFrameOwner(NonZeroU64);

impl NativeFrameOwner {
    pub(crate) const fn raw(self) -> u64 {
        self.0.get()
    }

    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let raw = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("native ownership identity exhausted");
        Self(NonZeroU64::new(raw).expect("native owner starts at one"))
    }

    pub(crate) fn frame(
        self,
        output: sophia_protocol::OutputId,
        head: sophia_engine::RenderHeadId,
        target_generation: u64,
        frame: u64,
    ) -> LiveNativeFrameIdentity {
        LiveNativeFrameIdentity {
            owner: self,
            output,
            head,
            target_generation: NonZeroU64::new(target_generation)
                .expect("native target generation"),
            frame: NonZeroU64::new(frame).expect("native frame generation"),
        }
    }
}

/// Exact native frame custody. Legacy exporters leave this absent and cannot
/// derive native presentation authority from a worker request or scene trace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveNativeFrameIdentity {
    owner: NativeFrameOwner,
    output: sophia_protocol::OutputId,
    head: sophia_engine::RenderHeadId,
    target_generation: NonZeroU64,
    frame: NonZeroU64,
}

impl LiveNativeFrameIdentity {
    /// Process-local native owner incarnation for exact diagnostic correlation.
    pub const fn owner(self) -> u64 {
        self.owner.raw()
    }

    pub const fn output(self) -> sophia_protocol::OutputId {
        self.output
    }
    pub const fn head(self) -> sophia_engine::RenderHeadId {
        self.head
    }
    pub const fn target_generation(self) -> u64 {
        self.target_generation.get()
    }
    pub const fn frame(self) -> u64 {
        self.frame.get()
    }
}

use super::*;

impl LiveProductionVisualRuntime {
    /// Installs or clears the session lock cover and queues a retained repaint
    /// of every head.
    ///
    /// Installing never rolls back. The cover is runtime state that every
    /// display list consults, so if the repaint cannot be queued now, the next
    /// frame any path composes is still covered. Clearing does roll back on
    /// failure: a session that could not repaint stays covered rather than
    /// reporting an unlock its heads never drew.
    pub fn set_session_lock(
        &mut self,
        cover: Option<sophia_engine::SessionLockCover>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.set_session_lock_on(cover, scene, native_scanout)
    }

    pub(crate) fn set_session_lock_on<T: NativeCompositionTarget>(
        &mut self,
        cover: Option<sophia_engine::SessionLockCover>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut T>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.session_lock == cover {
            return Ok(false);
        }
        let clearing = cover.is_none();
        let previous = std::mem::replace(&mut self.session_lock, cover);
        if let Some(native) = native_scanout {
            if let Err(error) = self.queue_retained_projection(scene, native) {
                if clearing {
                    self.session_lock = previous;
                }
                return Err(error);
            }
        } else {
            self.publish_committed_input_layers();
        }
        Ok(true)
    }

    pub fn session_lock(&self) -> Option<sophia_engine::SessionLockCover> {
        self.session_lock.clone()
    }

    /// The lock every head of every output has presented: `Some` only when
    /// each output's heads all retired a frame drawn for the same lock and
    /// nothing else. An output with no head, or a head still showing an
    /// earlier frame, leaves the session unproven.
    pub fn presented_session_lock(
        &self,
        native: &LiveProductionNativeScanout,
    ) -> Option<sophia_engine::SessionLockEpoch> {
        self.presented_session_lock_on(native)
    }

    pub(crate) fn presented_session_lock_on<T: NativeCompositionTarget>(
        &self,
        native: &T,
    ) -> Option<sophia_engine::SessionLockEpoch> {
        let mut proven = None;
        for (output, _) in self.outputs.logical_viewports() {
            let epoch = sophia_engine::presented_session_lock(
                output,
                &native.presented_head_frames(output),
            )?;
            if *proven.get_or_insert(epoch) != epoch {
                return None;
            }
        }
        proven
    }

    /// The provider image, and its candidate generation, that every head of
    /// `output` retired under the current lock; `None` until they agree.
    pub fn presented_session_lock_image(
        &self,
        native: &LiveProductionNativeScanout,
        output: OutputId,
    ) -> Option<(sophia_engine::SessionLockImageIdentity, u64)> {
        sophia_engine::presented_session_lock_image(output, &native.presented_head_frames(output))
    }
}

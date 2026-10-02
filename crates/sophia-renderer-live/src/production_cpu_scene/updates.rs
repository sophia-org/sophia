use super::*;

impl LiveProductionCpuScene {
    fn apply_cpu_update(
        &mut self,
        update: LiveCpuBufferUpdate,
    ) -> Result<bool, crate::LiveCpuBufferRegistryError> {
        // Registry updates may replace or patch bytes at the same generation.
        // Numeric generations alone cannot distinguish those raster inputs.
        let reused_generation = self
            .buffers
            .get(update.handle())
            .is_some_and(|buffer| buffer.generation == update.generation());
        let applied = self.buffers.apply(update)?;
        if applied && reused_generation {
            self.force_full_repaint();
        }
        Ok(applied)
    }

    pub fn apply_updates(
        &mut self,
        updates: impl IntoIterator<Item = LiveCpuBufferUpdate>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for update in updates {
            self.apply_cpu_update(update)
                .map_err(|error| format!("renderer CPU buffer update failed: {error:?}"))?;
        }
        Ok(())
    }

    pub fn apply_production_updates(
        &mut self,
        updates: impl IntoIterator<Item = LiveCpuBufferUpdate>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for update in updates {
            match self.apply_cpu_update(update) {
                Ok(_) | Err(crate::LiveCpuBufferRegistryError::MissingPatchBase) => {}
                Err(error) => {
                    return Err(format!("renderer CPU buffer update failed: {error:?}").into());
                }
            }
        }
        Ok(())
    }

    pub fn reconcile_buffer_residency(&mut self, retained_handles: &[u64]) {
        let before = self.buffers.len();
        self.buffers
            .retain_handles(|handle| retained_handles.binary_search(&handle).is_ok());
        if self.buffers.len() != before {
            // A later insertion can reuse an evicted handle and generation.
            self.force_full_repaint();
        }
    }
}

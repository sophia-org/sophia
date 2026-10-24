impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: std::os::fd::AsFd,
{
    pub fn persistent_render_stats(&self) -> NativeGbmPersistentRenderStats {
        let mut stats = self.stats;
        let reuse = self.capture_reuse.stats();
        stats.capture_pool_live_count = reuse.live_count;
        stats.capture_pool_live_bytes = reuse.live_bytes;
        stats.capture_pool_peak_bytes = reuse.peak_bytes;
        stats.capture_pool_idle_count = reuse.idle_count;
        stats.capture_pool_idle_bytes = reuse.idle_bytes;
        stats.capture_pending_fences = self.snapshot_fences.len();
        stats.capture_prepared_destinations = self.capture_reuse.prepared.len();
        #[cfg(feature = "experimental-shell-capture")]
        {
            stats.capture_managed_images = self.managed_captures.images.len();
            stats.capture_prepared_destinations += self.capture_reuse.managed_prepared.len();
        }
        stats.sampling = stats.sampling.saturating_add(reuse.sampling);
        stats.capture_context_creations = stats
            .capture_context_creations
            .saturating_add(reuse.context_creations as u64);
        stats.capture_config_selections = stats.capture_config_selections.saturating_add(reuse.config_selections as u64);
        stats.capture_context_reuses = stats.capture_context_reuses.saturating_add(reuse.context_reuses as u64);
        stats.gl_pipeline_creations = stats
            .gl_pipeline_creations
            .saturating_add(reuse.context_creations);
        stats.capture_setup_cpu = stats.capture_setup_cpu.saturating_add(reuse.setup_cpu);
        stats.capture_copy_cpu = stats.capture_copy_cpu.saturating_add(reuse.copy_cpu);
        stats.capture_cleanup_cpu = stats.capture_cleanup_cpu.saturating_add(reuse.cleanup_cpu);
        stats.capture_setup_elapsed = stats.capture_setup_elapsed.saturating_add(reuse.setup_elapsed);
        stats.capture_copy_elapsed = stats.capture_copy_elapsed.saturating_add(reuse.copy_elapsed);
        stats.capture_cleanup_elapsed = stats.capture_cleanup_elapsed.saturating_add(reuse.cleanup_elapsed);
        stats.capture_import_cpu = stats.capture_import_cpu.saturating_add(reuse.import_cpu);
        stats.capture_import_elapsed = stats
            .capture_import_elapsed
            .saturating_add(reuse.import_elapsed);
        stats.capture_source_evictions = stats
            .capture_source_evictions
            .saturating_add(reuse.source_evictions);
        stats.capture_destination_allocations = stats
            .capture_destination_allocations
            .saturating_add(reuse.destination_allocation_calls);
        stats.capture_destination_allocation_cpu = stats
            .capture_destination_allocation_cpu
            .saturating_add(reuse.destination_allocation_cpu);
        stats.capture_destination_allocation_elapsed = stats
            .capture_destination_allocation_elapsed
            .saturating_add(reuse.destination_allocation_elapsed);
        stats.capture_source_transient_imports = stats
            .capture_source_transient_imports
            .saturating_add(reuse.source_transient_imports);
        stats.transfer_captures = self.transfer_stats.captures;
        stats.transfer_attempts = self.transfer_stats.attempts;
        stats.transfer_failures = self.transfer_stats.failures;
        if let Some(persistent) = self.composition_target.as_ref() {
            accumulate_import_cache_stats(&mut stats.import_cache, persistent.import_cache.stats());
            stats.sampling = stats
                .sampling
                .saturating_add(persistent.target.pipeline.sampling_stats());
        }
        for persistent in self
            .target_sets
            .values()
            .flat_map(|set| set.targets.iter().flatten())
        {
            accumulate_import_cache_stats(&mut stats.import_cache, persistent.import_cache.stats());
            stats.sampling = stats
                .sampling
                .saturating_add(persistent.target.pipeline.sampling_stats());
        }
        for cached in self.capture_targets.iter().flatten() {
            stats.sampling = stats
                .sampling
                .saturating_add(cached.target.pipeline.sampling_stats());
        }
        stats
    }
}

fn accumulate_import_cache_stats(
    total: &mut NativeDmaBufImportCacheStats,
    additional: NativeDmaBufImportCacheStats,
) {
    total.imports = total.imports.saturating_add(additional.imports);
    total.hits = total.hits.saturating_add(additional.hits);
    total.evictions = total.evictions.saturating_add(additional.evictions);
    total.live_entries = total.live_entries.saturating_add(additional.live_entries);
    total.descriptor_mismatches = total
        .descriptor_mismatches
        .saturating_add(additional.descriptor_mismatches);
    total.capacity_rejections = total
        .capacity_rejections
        .saturating_add(additional.capacity_rejections);
    total.texture_calls = total.texture_calls.saturating_add(additional.texture_calls);
    total.texture_cpu = total.texture_cpu.saturating_add(additional.texture_cpu);
    total.texture_elapsed = total.texture_elapsed.saturating_add(additional.texture_elapsed);
}

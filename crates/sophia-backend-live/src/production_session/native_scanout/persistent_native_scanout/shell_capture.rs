//! Capture retirement is maintenance, independent of future frame composition.
use super::*;

impl LiveProductionNativeScanout {
    /// Evict from every current store, including disabled heads. A shared worker
    /// is visited once; inline contexts are each visited. A busy/failed store
    /// refuses the whole acknowledgment so the resource owner retries. False
    /// from one store says nothing about another store or remaining GPU uses.
    pub fn evict_shell_capture_imports(
        &mut self,
        image: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<(), crate::LiveRendererScanoutBufferExportDetail> {
        let stores = self
            .exporters
            .iter()
            .map(|exporter| exporter.image_store_identity())
            .collect::<Vec<_>>();
        evict_all_stores(stores, |index| {
            self.exporters[index]
                .evict_renderer_image_imports(image)
                .map(|_| ())
        })
    }
}

fn evict_all_stores<E>(
    stores: impl IntoIterator<Item = Option<u64>>,
    mut evict: impl FnMut(usize) -> Result<(), E>,
) -> Result<(), E> {
    let mut visited = BTreeSet::new();
    for (index, store) in stores.into_iter().enumerate() {
        if store.is_none_or(|identity| visited.insert(identity)) {
            evict(index)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_visits_all_inline_and_distinct_worker_stores_without_a_frame() {
        let mut seen = Vec::new();
        evict_all_stores([Some(7), None, Some(7), Some(9), None], |index| {
            seen.push(index);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(seen, [0, 1, 3, 4]);
    }

    #[test]
    fn failed_store_keeps_acknowledgment_pending_and_retry_revisits_every_store() {
        let mut seen = Vec::new();
        let stores = [Some(7), None, Some(9)];
        assert_eq!(
            evict_all_stores(stores, |index| {
                seen.push(index);
                if index == 1 { Err("busy") } else { Ok(()) }
            }),
            Err("busy")
        );
        assert_eq!(seen, [0, 1]);
        seen.clear();
        evict_all_stores(stores, |index| {
            seen.push(index);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(seen, [0, 1, 2]);
    }
}

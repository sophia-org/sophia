use super::*;

#[derive(Debug, Default)]
pub(super) struct XPixmapExportDamage {
    rects: Vec<Rect>,
}

impl XSoftwareBufferStore {
    pub(crate) fn begin_export_tracking(&mut self, drawable: XResourceId) {
        let rects = self.buffers.get(&drawable).map_or_else(Vec::new, |buffer| {
            vec![Rect {
                x: 0,
                y: 0,
                width: buffer.size.width,
                height: buffer.size.height,
            }]
        });
        self.export_damage
            .entry(drawable)
            .or_insert(XPixmapExportDamage { rects });
    }

    pub(crate) fn end_export_tracking(&mut self, drawable: XResourceId) {
        self.export_damage.remove(&drawable);
    }

    pub(crate) fn export_generation(&self, drawable: XResourceId) -> u64 {
        self.buffers
            .get(&drawable)
            .map_or(0, |buffer| buffer.generation)
    }

    pub(crate) fn export_has_damage(&self, drawable: XResourceId) -> bool {
        self.export_damage
            .get(&drawable)
            .is_some_and(|damage| !damage.rects.is_empty())
    }

    pub(crate) fn take_export_patches(
        &mut self,
        drawable: XResourceId,
    ) -> Option<(u64, Vec<crate::XServerFrontendPixmapPatch>)> {
        let damage = self.export_damage.get_mut(&drawable)?;
        let buffer = self.buffers.get(&drawable)?;
        let patches = damage
            .rects
            .iter()
            .map(|rect| {
                let patch = packed_patch_region(buffer, *rect)?;
                Some(crate::XServerFrontendPixmapPatch {
                    rect: patch.rect,
                    bytes: Arc::unwrap_or_clone(patch.bytes),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        damage.rects.clear();
        Some((buffer.generation, patches))
    }

    pub(crate) fn restore_export_damage(&mut self, drawable: XResourceId, rects: &[Rect]) {
        for rect in rects {
            self.note_export_damage(drawable, false, Some(*rect));
        }
    }

    pub(super) fn note_export_damage(
        &mut self,
        drawable: XResourceId,
        replaced: bool,
        rect: Option<Rect>,
    ) {
        let Some(damage) = self.export_damage.get_mut(&drawable) else {
            return;
        };
        let Some(buffer) = self.buffers.get(&drawable) else {
            return;
        };
        let full = Rect {
            x: 0,
            y: 0,
            width: buffer.size.width,
            height: buffer.size.height,
        };
        let rect = if replaced {
            full
        } else {
            let Some(rect) = rect else { return };
            let left = rect.x.max(0).min(full.width);
            let top = rect.y.max(0).min(full.height);
            let right = rect.x.saturating_add(rect.width).max(0).min(full.width);
            let bottom = rect.y.saturating_add(rect.height).max(0).min(full.height);
            if right <= left || bottom <= top {
                return;
            }
            Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            }
        };
        damage.rects.push(rect);
        if damage.rects.len() > X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS {
            damage.rects = coalesce_damage(
                std::mem::take(&mut damage.rects),
                X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS,
            );
        }
        // Overlapping covers must not turn one bounded backing into an oversized upload.
        let bytes = damage.rects.iter().fold(0usize, |sum, rect| {
            sum.saturating_add(rect.width as usize * rect.height as usize * 4)
        });
        if bytes > buffer.size.width as usize * buffer.size.height as usize * 4 {
            damage.rects.clear();
            damage.rects.push(full);
        }
    }
}

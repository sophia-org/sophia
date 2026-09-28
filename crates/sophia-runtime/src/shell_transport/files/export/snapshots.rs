//! Per-component encoded snapshot custody, separate from content stores,
//! transaction staging and journal bounds. One current and one open-fid pin
//! per disclosed feed share one build scratch for replacement publication.
use super::*;

pub(super) fn object_cap(kind: ShellFileKind) -> Option<usize> {
    Some(match kind {
        ShellFileKind::Descriptors => SHELL_FILE_DESCRIPTORS_MAX_BYTES,
        ShellFileKind::Tabs => SHELL_FILE_TABS_MAX_BYTES,
        ShellFileKind::Shortcuts => SHELL_FILE_SHORTCUTS_MAX_BYTES,
        ShellFileKind::Catalog => SHELL_FILE_MAX_OBJECT_BYTES,
        ShellFileKind::Indicators => SHELL_FILE_INDICATORS_MAX_BYTES,
        ShellFileKind::Outputs => SHELL_FILE_OUTPUTS_MAX_BYTES,
        _ => return None,
    })
}

impl ShellFiles {
    /// Derived from the same root vocabulary that grants access to feeds.
    /// Each feed appears once, including on combined descriptor/content roles.
    pub(super) fn selected_snapshot_bound(&self) -> usize {
        SHELL_FILE_MAX_OBJECT_BYTES
            + 2 * self
                .root_entries()
                .iter()
                .filter_map(|(_, node)| node.object_kind())
                .map(|kind| object_cap(kind).expect("every object has a cap"))
                .sum::<usize>()
    }

    pub(in crate::shell_transport) fn snapshot_accounting(&self) -> (usize, usize) {
        let retained = [
            &self.outputs,
            &self.catalog,
            &self.indicators,
            &self.descriptors,
            &self.tabs,
            &self.shortcuts,
        ]
        .into_iter()
        .map(ObjectSlot::retained_bytes)
        .sum();
        (self.snapshot_reserved_bytes, retained)
    }
}

impl ObjectSlot {
    fn retained_bytes(&self) -> usize {
        let current = self.current.as_ref().map_or(0, |object| object.bytes.len());
        let pinned = self.pinned.as_ref().and_then(Weak::upgrade);
        current
            + pinned
                .as_ref()
                .filter(|pin| {
                    self.current
                        .as_ref()
                        .is_none_or(|object| !Arc::ptr_eq(pin, object))
                })
                .map_or(0, |object| object.bytes.len())
    }
}

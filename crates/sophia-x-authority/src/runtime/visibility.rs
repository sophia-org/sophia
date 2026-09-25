// VisibilityNotify from occlusion: what each viewable window was last
// reported with, and what changed since (XTS Xlib11 VisibilityNotify 2, 3,
// 7 to 9). Included by runtime.rs; one module with it (t211).

impl XAuthorityRuntime {
    /// A client selected VisibilityChange on the window: its occlusion is
    /// computed from now on. Interest is never withdrawn short of the
    /// window's destruction; a stale interest costs a computation, not an
    /// event, since a window nobody selected on is filtered at delivery.
    pub fn note_visibility_interest(&mut self, window: crate::XResourceId) {
        self.visibility_interest.insert(window);
    }

    /// The window's visibility now, recorded as reported: the map that
    /// makes it viewable reports this rather than Unobscured.
    pub fn window_visibility(&mut self, namespace: NamespaceId, window: crate::XResourceId) -> u8 {
        let state = self
            .windows
            .visibility_states(namespace, &self.input_only_windows, &[window])
            .first()
            .map_or(0, |(_, state)| *state);
        self.visibility_reported.insert(window, state);
        state
    }

    /// Every window of interest in the namespace whose visibility differs
    /// from what it was last reported with, recorded as reported; a window
    /// no longer viewable is forgotten, so its next map reports afresh. A
    /// window mapped by a path that did not report it (the Engine's, or
    /// MapSubwindows) counts as reported Unobscured, which is what every
    /// map reported before occlusion was computed.
    pub fn visibility_changes(&mut self, namespace: NamespaceId) -> Vec<(crate::XResourceId, u8)> {
        self.visibility_interest
            .retain(|window| self.windows.get(*window).is_some());
        let candidates = self.visibility_interest.iter().copied().collect::<Vec<_>>();
        let states = self
            .windows
            .visibility_states(namespace, &self.input_only_windows, &candidates);
        let viewable = states.iter().map(|(window, _)| *window).collect::<BTreeSet<_>>();
        self.visibility_reported.retain(|window, _| {
            !candidates.contains(window)
                || viewable.contains(window)
                || self.windows.get(*window).is_some_and(|record| record.namespace != namespace)
        });
        let mut changes = Vec::new();
        for (window, state) in states {
            if self.visibility_reported.get(&window).copied().unwrap_or(0) != state {
                self.visibility_reported.insert(window, state);
                changes.push((window, state));
            }
        }
        changes
    }
}

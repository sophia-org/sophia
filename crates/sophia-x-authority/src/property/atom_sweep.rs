// Clearing properties whose atom has been forgotten. Split from property.rs
// for size; the impl block is the same table, included at the same scope.

impl XPropertyTable {
    /// Drops every property stored under an atom that no longer has a name.
    ///
    /// A record keyed by a forgotten atom is unreachable, because no client
    /// can name the atom again: interning never reissues a freed id. Leaving
    /// it would be a slow leak of rows nothing can read, and would make the
    /// authority's own advertisement rows accumulate one set per connection
    /// era. Called with exactly the atoms the table just forgot.
    pub fn remove_atoms(&mut self, forgotten: &[XAtom]) -> usize {
        let forgotten: BTreeSet<XAtom> = forgotten.iter().copied().collect();
        let before = self.records.len();
        self.records
            .retain(|(_, _, atom), _| !forgotten.contains(atom));
        self.engine_owned
            .retain(|(_, _, atom)| !forgotten.contains(atom));
        before.saturating_sub(self.records.len())
    }

    pub fn remove_window(&mut self, namespace: NamespaceId, window: XResourceId) -> usize {
        let before = self.records.len();
        self.records
            .retain(|(record_namespace, record_window, _), _| {
                *record_namespace != namespace || *record_window != window
            });
        self.engine_owned
            .retain(|(record_namespace, record_window, _)| {
                *record_namespace != namespace || *record_window != window
            });
        before.saturating_sub(self.records.len())
    }
}

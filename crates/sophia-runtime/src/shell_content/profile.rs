/// Immutable store vocabulary selected by the Session admission owner.
/// This prepares storage; it is not a negotiated capability or focus grant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContentStoreProfile {
    #[default]
    Legacy,
    NativeLauncher,
    /// Persistent catalog content; independently negotiated from transient menus.
    PersistentCatalog,
}

//! Wire-neutral typed model for the atomic revision-8 persistent catalog:
//! no partial catalog ever becomes current.
use crate::ShellApplicationCatalog;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellPersistentCatalog {
    pub catalog: ShellApplicationCatalog,
    pub identities: BTreeMap<u16, String>,
}

//! Sophia's metadata broker.
//!
//! Owns the "Metadata broker/shell" row of `docs/architecture.md` and nothing else:
//! disclosure policy, trust assignment, icon tokens, and aggregation across
//! authorities. It does **not** own raw client identity. Authorities reduce their
//! own titles and classes under the rule this crate publishes, so a title never
//! crosses a process boundary and no single component holds every client's identity.
//!
//! Nothing here names an X type. Inputs carry `SurfaceId` and `NamespaceId`, both
//! authority-neutral, which is what lets a second authority arrive without needing a
//! second broker.

pub mod file_client;
mod metadata;
mod requests;
mod trust;

pub use metadata::*;
pub use trust::*;

//! Support for the `shell_files_kdl` conformance test.
//!
//! - [`kdl_model`] parses `protocol/sophia-shell-files-v1.kdl` into blocks
//!   and fields.
//! - [`checks`] reads encoded bytes against those fields.
//! - [`fixtures`], [`fixtures_limits`] and [`fixtures_tables`] build the
//!   distinctly-valued typed values the checks compare against, alongside
//!   the field-name-to-value table each one stands for.
//!
//! The `fmap!` macro and the small id/rect/margin constructors below are
//! shared by every fixture module: each field on a composite type is
//! spelled out, never `..`, so a struct that grows a field breaks the
//! constructor that builds it.

pub mod checks;
pub mod fixtures;
pub mod fixtures_catalog;
pub mod fixtures_indicators;
pub mod fixtures_limits;
pub mod fixtures_native_launcher;
pub mod fixtures_tables;
pub mod kdl_model;

use sophia_shell_protocol::*;

/// Builds a `BTreeMap<&str, i128>` from local bindings, one entry per name,
/// keyed by the binding's own name (via `stringify!`). Every binding used
/// here is named after the KDL field it stands for.
macro_rules! fmap {
    ($($name:ident),+ $(,)?) => {
        ::std::collections::BTreeMap::from([$((stringify!($name), i128::from($name))),+])
    };
}
pub(crate) use fmap;

pub(crate) fn grant(connection_epoch: u64, content_grant_epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch,
        content_grant_epoch,
    }
}

pub(crate) fn oid(id: u64, generation: u64) -> ContentOutputId {
    ContentOutputId { id, generation }
}

pub(crate) fn rid(id: u64, generation: u64) -> ContentResourceId {
    ContentResourceId { id, generation }
}

pub(crate) fn aid(id: u64, generation: u64) -> ContentAllocationId {
    ContentAllocationId { id, generation }
}

pub(crate) fn margins_of(top: i16, right: i16, bottom: i16, left: i16) -> ContentMargins {
    ContentMargins {
        top,
        right,
        bottom,
        left,
    }
}

pub(crate) fn prect(x: i32, y: i32, width: u32, height: u32) -> ContentPixelRect {
    ContentPixelRect {
        x,
        y,
        width,
        height,
    }
}

pub(crate) fn lrect(x: i32, y: i32, width: u32, height: u32) -> ContentLogicalRect {
    ContentLogicalRect {
        x,
        y,
        width,
        height,
    }
}

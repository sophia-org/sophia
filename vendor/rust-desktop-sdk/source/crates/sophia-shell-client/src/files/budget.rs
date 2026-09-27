//! Memory budgets for fetched snapshot objects.
//!
//! RAW: an object's encoded bytes grow only by exact reservation (no
//! geometric doubling), and the requested capacity never passes the feed's
//! cap (`FEEDS`); the vector's reported capacity is checked against it too.
//! A failed reservation is an error, not an abort. The global allocator may
//! still round a request up to its own size class; that overhead is the
//! allocator's and is not counted here.
//!
//! DECODED: the codecs allocate what the declared counts ask for, and every
//! count and text length is bounded before allocation (`table_count`,
//! `take_text_padded`, `rows` with exact capacity), so a decoded object's
//! heap stays within the budget below. After decoding, the client computes a
//! CHARGED UPPER-BOUND ESTIMATE of the value's heap (`catalog_bytes`,
//! `indicators_bytes`): vector and string capacities as reported, plus a
//! fixed charge per map entry and per map, which is not a measurement of
//! what the map allocated. It refuses a value whose charge exceeds the
//! budget, so a construction change cannot silently breach it.
//!
//! PEAK per fetch: the older undelivered object of the same feed is dropped
//! before the new one is decoded, so the transient peak is the raw cap plus
//! one decoded budget, not two; the inbox then holds at most one undelivered
//! decoded object per feed.
//!
//! MAP CHARGE, against the pinned toolchain (rustc 1.96.1, x86_64): a
//! `BTreeMap<u16, String>` leaf node is a 304-byte allocation holding up to
//! eleven entries, and an internal node about 400 bytes (a probe measured
//! 304 bytes for 1 to 11 entries, 1008 for 12: two leaves and one internal
//! node, and 52.8 bytes per entry at 4096 sequential inserts). Every node but
//! the root holds at least five entries, so leaves cost at most 61 bytes per
//! entry and internal nodes, one per six or more children, under 14 more:
//! under 75 bytes per entry. 96 bytes per entry is charged, plus 512 bytes
//! for the root, which may hold a single entry. These are standard-library
//! implementation details, re-checked when the toolchain pin moves.

use std::mem::size_of;

use sophia_shell_protocol::{
    SOPHIA_SHELL_APPLICATION_KEYWORDS_MAX_BYTES, SOPHIA_SHELL_APPLICATION_LABEL_MAX_BYTES,
    SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES, SOPHIA_SHELL_MAX_APPLICATIONS,
    SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES, SOPHIA_SHELL_MAX_INDICATORS,
    SOPHIA_SHELL_MAX_OUTPUT_STATUS, ShellApplicationDescriptor, ShellIndicator,
    ShellIndicatorSnapshot, ShellOutputStatus, ShellPersistentCatalog,
};

const MAP_ENTRY_OVERHEAD: usize = 96;
const MAP_ROOT_ALLOWANCE: usize = 512;

/// The most heap a decoded persistent catalog may hold: every entry at its
/// longest label and keywords, and every identity at its longest.
pub(super) const CATALOG_BUDGET: usize = size_of::<ShellPersistentCatalog>()
    + SOPHIA_SHELL_MAX_APPLICATIONS
        * (size_of::<ShellApplicationDescriptor>()
            + SOPHIA_SHELL_APPLICATION_LABEL_MAX_BYTES
            + SOPHIA_SHELL_APPLICATION_KEYWORDS_MAX_BYTES)
    + MAP_ROOT_ALLOWANCE
    + SOPHIA_SHELL_MAX_APPLICATIONS
        * (MAP_ENTRY_OVERHEAD + SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES);

/// The most heap a decoded indicator snapshot may hold.
pub(super) const INDICATORS_BUDGET: usize = size_of::<ShellIndicatorSnapshot>()
    + SOPHIA_SHELL_MAX_OUTPUT_STATUS
        * (size_of::<ShellOutputStatus>() + SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES)
    + SOPHIA_SHELL_MAX_INDICATORS
        * (size_of::<ShellIndicator>() + SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES);

/// A charged upper-bound estimate of a decoded catalog's heap: reported
/// capacities, plus the fixed map charges above.
pub(super) fn catalog_bytes(catalog: &ShellPersistentCatalog) -> usize {
    let entries = &catalog.catalog.entries;
    size_of::<ShellPersistentCatalog>()
        + entries.capacity() * size_of::<ShellApplicationDescriptor>()
        + entries
            .iter()
            .map(|entry| entry.label.capacity() + entry.keywords.capacity())
            .sum::<usize>()
        + MAP_ROOT_ALLOWANCE
        + catalog
            .identities
            .values()
            .map(|identity| MAP_ENTRY_OVERHEAD + identity.capacity())
            .sum::<usize>()
}

/// A charged upper-bound estimate of a decoded indicator snapshot's heap,
/// from its reported capacities.
pub(super) fn indicators_bytes(snapshot: &ShellIndicatorSnapshot) -> usize {
    size_of::<ShellIndicatorSnapshot>()
        + snapshot.statuses.capacity() * size_of::<ShellOutputStatus>()
        + snapshot
            .statuses
            .iter()
            .map(|status| status.layout.capacity())
            .sum::<usize>()
        + snapshot.indicators.capacity() * size_of::<ShellIndicator>()
        + snapshot
            .indicators
            .iter()
            .map(|indicator| indicator.label.capacity())
            .sum::<usize>()
}

// The arithmetic above, checked at compile time: a type growing
// unexpectedly fails the build here.
const _: () = assert!(CATALOG_BUDGET < 4 * 1024 * 1024);
const _: () = assert!(INDICATORS_BUDGET < 64 * 1024);

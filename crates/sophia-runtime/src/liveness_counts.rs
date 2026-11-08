//! Owner-thread counts of liveness polls, keyed by the transport entry and the
//! innermost labelled boundary. Diagnostic only: nothing here gates, caches or
//! reorders an obligation. A count is one saturating add on a thread-local
//! cell, with no clock read and no allocation.
//!
//! `Entry` is the `ShellFileWire::service` invocation kind, an input turn or a
//! publication flush; work outside any such invocation is `Outside`.
//! `Boundary` is the innermost labelled operation; a call inside an entry but
//! outside every labelled operation, such as an image Hello handshake poll or
//! the experimental custody-dispatch turn, is `Other`. Each count lands in
//! exactly one cell, so the cells partition every total. `custody_polls` and
//! `image_alive_polls` are counted immediately before the executed zero-time
//! `poll`; `export_polls` and `file_checks` count wrapper invocations that may
//! return before any poll and are never syscall totals; `accept_attempts`
//! counts `accept` calls, not joins or errors. Counts made on other threads
//! stay on those threads and are not part of the owner's receipt.
use std::cell::Cell;
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Outside,
    Turn,
    Flush,
}

impl Entry {
    pub const ALL: [Self; 3] = [Self::Outside, Self::Turn, Self::Flush];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Outside => "outside",
            Self::Turn => "turn",
            Self::Flush => "flush",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Other,
    PreTurn,
    ServerTurn,
    ServerFlush,
    PostTurn,
    FileCheck,
    ImageAccept,
    ImageCheck,
}

impl Boundary {
    pub const ALL: [Self; 8] = [
        Self::Other,
        Self::PreTurn,
        Self::ServerTurn,
        Self::ServerFlush,
        Self::PostTurn,
        Self::FileCheck,
        Self::ImageAccept,
        Self::ImageCheck,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Other => "other",
            Self::PreTurn => "pre_turn",
            Self::ServerTurn => "server_turn",
            Self::ServerFlush => "server_flush",
            Self::PostTurn => "post_turn",
            Self::FileCheck => "file_check",
            Self::ImageAccept => "image_accept",
            Self::ImageCheck => "image_check",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// Entry invocations, counted once in the entry's `Other` cell.
    Entries,
    /// `AuthenticatedFiles::poll` wrapper invocations.
    ExportPolls,
    /// Per-file `ProtectedFiles::check` wrapper invocations.
    FileChecks,
    /// Executed `LaunchCustody::is_live` polls, one poll over both pidfds.
    CustodyPolls,
    /// Executed image connector process-alive polls.
    ImageAlivePolls,
    /// Image listener `accept` calls, whatever they return.
    AcceptAttempts,
}

impl Field {
    pub const ALL: [Self; 6] = [
        Self::Entries,
        Self::ExportPolls,
        Self::FileChecks,
        Self::CustodyPolls,
        Self::ImageAlivePolls,
        Self::AcceptAttempts,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Entries => "entries",
            Self::ExportPolls => "export_polls",
            Self::FileChecks => "file_checks",
            Self::CustodyPolls => "custody_polls",
            Self::ImageAlivePolls => "image_alive_polls",
            Self::AcceptAttempts => "accept_attempts",
        }
    }
}

/// One cell's counts, in `Field::ALL` order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts(pub [u64; Field::ALL.len()]);

impl Counts {
    pub const fn get(self, field: Field) -> u64 {
        self.0[field as usize]
    }
}

#[derive(Clone, Copy)]
struct Context {
    entry: Entry,
    boundary: Boundary,
}

const OUTSIDE: Context = Context {
    entry: Entry::Outside,
    boundary: Boundary::Other,
};

type Grid<T> = [[T; Boundary::ALL.len()]; Entry::ALL.len()];

thread_local! {
    static CONTEXT: Cell<Context> = const { Cell::new(OUTSIDE) };
    static TABLE: Grid<[Cell<u64>; Field::ALL.len()]> =
        const { [const { [const { [const { Cell::new(0) }; Field::ALL.len()] }; Boundary::ALL.len()] }; Entry::ALL.len()] };
}

/// Restores the complete prior context when dropped, on return or unwind.
/// Thread-bound: it restores the context of the thread that created it.
#[must_use = "the scope ends as soon as this guard drops"]
pub struct Scope {
    prior: Context,
    thread: PhantomData<*const ()>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        CONTEXT.with(|context| context.set(self.prior));
    }
}

fn scope(next: Context) -> Scope {
    Scope {
        prior: CONTEXT.with(|context| context.replace(next)),
        thread: PhantomData,
    }
}

/// Begin a transport entry: the boundary resets to `Other` and the entry is
/// counted once. Only `Turn` and `Flush` are entered; `Outside` is the default.
pub fn enter(entry: Entry) -> Scope {
    let scope = scope(Context {
        entry,
        boundary: Boundary::Other,
    });
    add(Field::Entries, 1);
    scope
}

/// Label the innermost operation until the guard drops; the entry is kept.
pub fn within(boundary: Boundary) -> Scope {
    let entry = CONTEXT.with(|context| context.get().entry);
    scope(Context { entry, boundary })
}

/// One more of `field` in the calling thread's current cell.
pub fn count(field: Field) {
    add(field, 1);
}

fn add(field: Field, delta: u64) {
    let context = CONTEXT.with(Cell::get);
    TABLE.with(|table| {
        let cell = &table[context.entry as usize][context.boundary as usize][field as usize];
        cell.set(cell.get().saturating_add(delta));
    });
}

/// The calling thread's complete grid, copied once.
pub fn snapshot() -> Snapshot {
    Snapshot(TABLE.with(|table| {
        std::array::from_fn(|entry| {
            std::array::from_fn(|boundary| {
                Counts(std::array::from_fn(|field| {
                    table[entry][boundary][field].get()
                }))
            })
        })
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot(Grid<Counts>);

impl Snapshot {
    pub const fn get(&self, entry: Entry, boundary: Boundary) -> Counts {
        self.0[entry as usize][boundary as usize]
    }

    /// Every cell exactly once: entries outer, boundaries inner.
    pub fn rows(&self) -> impl Iterator<Item = (Entry, Boundary, Counts)> + '_ {
        Entry::ALL.into_iter().flat_map(move |entry| {
            Boundary::ALL
                .into_iter()
                .map(move |boundary| (entry, boundary, self.get(entry, boundary)))
        })
    }

    /// The sum of `field` over the whole grid, saturating.
    pub fn total(&self, field: Field) -> u64 {
        self.rows()
            .map(|(_, _, counts)| counts.get(field))
            .fold(0, u64::saturating_add)
    }
}

mod tests;

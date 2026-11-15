//! Gate 4 C3b: the diagnostic combined witness (specifications 03 and 04).
//! Every record is one `sophia_combined_witness schema=1 kind=<kind>` line of
//! fixed fields from closed sets, written through `session_println!` on the
//! owner thread where the observed call is made. Nothing here decides, waits,
//! clears a wake, retries, or holds a borrow past the statement that emits
//! it. Compiled only under W: the row's three gates and
//! `diagnostic-combined-witness`; the reader lives in test support.
use super::*;
use sophia_backend_live::CombinedAcceptError;
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// The owner iteration and component pass a record belongs to; `None` is an
/// exhausted counter, printed `exhausted`, which the reader rejects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WitnessPass {
    pub(crate) iteration: Option<u64>,
    pub(crate) cpass: Option<u64>,
}

impl WitnessPass {
    pub(crate) fn fields(&self) -> String {
        format!(
            "iteration={} cpass={}",
            opt(self.iteration),
            opt(self.cpass)
        )
    }
}

/// A pass and the slot whose row a record concerns.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PassSlot {
    pub(crate) pass: WitnessPass,
    pub(crate) slot: usize,
}

impl PassSlot {
    fn fields(&self) -> String {
        format!("{} slot={}", self.pass.fields(), self.slot)
    }
}

/// What the row keeps for its records: its pass and slot, written at the top
/// of each step that visits it; the bid of the entry the last `join`
/// matched (spec 03 1.4); the last `decline`'s release, for the arm's record.
#[derive(Default)]
pub(crate) struct RowWitness {
    pub(crate) at: PassSlot,
    /// `None`: the join matched no entry. `Some(None)`: it matched an entry
    /// whose bid allocation was exhausted.
    pub(crate) joined_bid: Option<Option<u64>>,
    declined: Option<(CombinedRelease, bool, String)>,
}

/// The identity of an ordinary entry the arm handed to the install
/// (`CombinedEntry::Ordinary`), read by the submission step.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WitnessEntry {
    at: PassSlot,
    cseq: Option<u64>,
    bid: Option<Option<u64>>,
}

static NEXT_BID: AtomicU64 = AtomicU64::new(1);
static NEXT_CSEQ: AtomicU64 = AtomicU64::new(1);

/// Checked allocation: `None` once the counter is exhausted, never a reuse.
fn next(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
        .ok()
}

pub(crate) fn next_bid() -> Option<u64> {
    next(&NEXT_BID)
}

/// The owner iteration from the wake's saturating pass count: saturated is
/// exhausted.
pub(crate) fn iteration(passes: u64) -> Option<u64> {
    (passes != u64::MAX).then_some(passes)
}

pub(crate) fn opt(value: Option<u64>) -> String {
    value.map_or_else(|| "exhausted".to_owned(), |value| value.to_string())
}

fn bid_field(bid: Option<Option<u64>>) -> String {
    bid.map_or_else(|| "none".to_owned(), opt)
}

/// A worker serial: 0 is the exhausted allocator's never-issued value.
fn serial_field(serial: u64) -> String {
    opt((serial != 0).then_some(serial))
}

pub(crate) fn head_identity(head: &CombinedHead) -> String {
    let (owner, head, ticket) = head.witness_identity();
    format!("({owner},{head},{ticket})")
}

fn grant_field(grant: ContentGrant) -> String {
    format!("({},{})", grant.connection_epoch, grant.content_grant_epoch)
}

pub(crate) fn slot_list(slots: &[usize]) -> String {
    let inner: Vec<String> = slots.iter().map(ToString::to_string).collect();
    format!("[{}]", inner.join(","))
}

fn record(kind: &str, fields: std::fmt::Arguments<'_>) {
    crate::session_println!("sophia_combined_witness schema=1 kind={kind} {fields}");
}

/// The kind of a row state, and the disposition it stores, if any.
fn kind(state: &State) -> (&'static str, Option<CombinedDisposition>) {
    match state {
        State::InFlight { .. } => ("InFlight", None),
        State::Unsettled { disposition, .. }
        | State::Credit { disposition, .. }
        | State::Undelivered { disposition, .. } => ("retry", Some(*disposition)),
        State::Held => ("Held", None),
        State::Failed(disposition) => ("Failed", Some(*disposition)),
        State::Debt(_) => ("Debt", None),
    }
}

/// `collect_one`'s pre-step state, for its record (spec 03 3.1 table).
pub(crate) fn step_from(state: &State) -> (&'static str, Option<CombinedDisposition>) {
    match state {
        State::Unsettled { disposition, .. } => ("retry_settle", Some(*disposition)),
        State::Credit { disposition, .. } => ("retry_credit", Some(*disposition)),
        State::Undelivered { disposition, .. } => ("retry_deliver", Some(*disposition)),
        other => kind(other),
    }
}

pub(crate) fn state_label(state: &State) -> String {
    match state {
        State::InFlight { .. } => "InFlight".to_owned(),
        State::Unsettled { .. } => "Unsettled".to_owned(),
        State::Credit { .. } => "Credit".to_owned(),
        State::Undelivered { .. } => "Undelivered".to_owned(),
        State::Held => "Held".to_owned(),
        State::Failed(disposition) => format!("Failed:{disposition:?}"),
        State::Debt(release) => format!("Debt:{release:?}"),
    }
}

impl CombinedRow {
    fn at(&self) -> String {
        self.witness.at.fields()
    }

    /// Where the admission is submitted (spec 03 2.1): the job's bid, the
    /// issuing validator's serial and job, the grant and the head identity.
    pub(super) fn witness_binding(
        &self,
        bid: Option<u64>,
        ticket: u64,
        reservation: &CombinedReservation,
        open: &Open,
    ) {
        record(
            "binding",
            format_args!(
                "{} bid={} ticket={ticket} serial={} job={} grant={} head={}",
                self.at(),
                opt(bid),
                serial_field(reservation.validator_serial()),
                reservation.job(),
                grant_field(open.grant),
                head_identity(&open.head),
            ),
        );
    }

    /// One record per `collect_one` call: a fresh result taken, a retry of a
    /// stored disposition, a route error, no result, or a plain visit; and
    /// the state the job is left in.
    pub(super) fn witness_collect(
        &self,
        job: &Job,
        from: (&'static str, Option<CombinedDisposition>),
        taken: Option<CombinedDisposition>,
        route_error: bool,
    ) {
        let result = match (from, taken, route_error) {
            (("InFlight", _), Some(disposition), _) => format!("taken:{disposition:?}"),
            (("InFlight", _), None, true) => "route_error".to_owned(),
            (("InFlight", _), None, false) => "none".to_owned(),
            ((retry, Some(disposition)), _, _) if retry.starts_with("retry_") => {
                format!("{retry}:{disposition:?}")
            }
            _ => "visit".to_owned(),
        };
        record(
            "collect",
            format_args!(
                "{} bid={} result={result} state_after={}",
                self.at(),
                opt(job.bid),
                state_label(&job.state),
            ),
        );
    }

    /// The returned-plan removal (spec 03 2.1): the job ends here. `bid` is
    /// copied before the cancellation takes the job's head.
    pub(super) fn witness_collect_returned(&self, bid: Option<u64>, confirmed: bool) {
        let confirmed = if confirmed {
            "confirmed"
        } else {
            "unconfirmed"
        };
        record(
            "collect",
            format_args!(
                "{} bid={} result=returned_plan:{confirmed} state_after=removed",
                self.at(),
                opt(bid),
            ),
        );
    }

    pub(super) fn witness_removed(&self, bid: Option<u64>, cause: &str) {
        record(
            "removed",
            format_args!("{} bid={} cause={cause}", self.at(), opt(bid)),
        );
    }

    pub(super) fn witness_removed_job(&self, job: Option<&Job>, cause: &str) {
        if let Some(job) = job {
            self.witness_removed(job.bid, cause);
        }
    }

    /// A walk-site Release attempt, after `released` decided whether the
    /// job stays (spec 03 2.1); the arm's own site is recorded by the arm.
    pub(super) fn witness_release(
        &self,
        job: &Job,
        site: &str,
        outcome: CombinedRelease,
        keep: bool,
    ) {
        let state_after = if keep {
            state_label(&job.state)
        } else {
            "removed".to_owned()
        };
        record(
            "release_attempt",
            format_args!(
                "{} bid={} site={site} cseq=none outcome={outcome:?} keep={keep} state_after={state_after}",
                self.at(),
                opt(job.bid),
            ),
        );
    }

    /// `decline`'s outcome, kept for the arm's `release_attempt site=arm`.
    pub(super) fn witness_declined(&mut self, outcome: CombinedRelease, keep: bool, job: &Job) {
        let state_after = if keep {
            state_label(&job.state)
        } else {
            "removed".to_owned()
        };
        self.witness.declined = Some((outcome, keep, state_after));
    }

    /// An unconfirmed pre-submission cancellation (no job, no bid), where
    /// the hooks cancel it; identified by ticket and head (spec 02 R2).
    pub(super) fn witness_reserve_cancelled(&self, ticket: u64, head: &str, confirmed: bool) {
        record(
            "reserve_cancelled",
            format_args!(
                "{} ticket={ticket} head={head} confirmed={confirmed}",
                self.at()
            ),
        );
    }

    /// The walk's retry of an unconfirmed cancellation.
    pub(super) fn witness_cancel_retry(&self, head: &CombinedHead, confirmed: bool) {
        record(
            "cancel_retry",
            format_args!(
                "{} head={} confirmed={confirmed}",
                self.at(),
                head_identity(head)
            ),
        );
    }

    /// The hooked image service visited this slot's row (turn.rs step 1).
    pub(super) fn witness_images_visit(&self) {
        record("images_visit", format_args!("{} hooked=true", self.at()));
    }

    /// The walk is about to visit this slot's row (turn.rs step 3), with
    /// what the row holds before it.
    pub(super) fn witness_walk_visit(&self) {
        record(
            "walk_visit",
            format_args!(
                "{} jobs={} open={} cancels={}",
                self.at(),
                self.jobs.len(),
                usize::from(self.open.is_some()),
                self.cancels.len(),
            ),
        );
    }
}

/// One `consume_combined` call's record, printed once when the call returns
/// (spec 03 3.1): fixed fields, each set at the statement that decides it;
/// `entry` stays `error` unless a return sets it, and is `unwinding` on a
/// panic. Holds Copy data and cells only, no borrow of the row or native.
pub(crate) struct ConsumeWitness {
    at: PassSlot,
    cseq: Option<u64>,
    bid: Cell<Option<Option<u64>>>,
    pub(crate) join: Cell<&'static str>,
    pub(crate) native: Cell<&'static str>,
    pub(crate) deadline: Cell<&'static str>,
    pub(crate) check: Cell<&'static str>,
    pub(crate) classify: Cell<&'static str>,
    pub(crate) accept: Cell<&'static str>,
    pub(crate) decline: Cell<&'static str>,
    pub(crate) terminal: Cell<&'static str>,
    pub(crate) entry: Cell<&'static str>,
}

fn accept_error(error: &CombinedAcceptError) -> &'static str {
    match error {
        CombinedAcceptError::Runtime(_) => "refused:Runtime",
        CombinedAcceptError::Output => "refused:Output",
        CombinedAcceptError::RetirementClaim => "refused:RetirementClaim",
        CombinedAcceptError::Native(_) => "refused:Native",
        CombinedAcceptError::Unchanged => "refused:Unchanged",
    }
}

impl ConsumeWitness {
    pub(crate) fn new(at: PassSlot) -> Self {
        Self {
            at,
            cseq: next(&NEXT_CSEQ),
            bid: Cell::new(None),
            join: Cell::new("none"),
            native: Cell::new("none"),
            deadline: Cell::new("none"),
            check: Cell::new("none"),
            classify: Cell::new("none"),
            accept: Cell::new("none"),
            decline: Cell::new("none"),
            terminal: Cell::new("none"),
            entry: Cell::new("error"),
        }
    }

    fn fields(&self) -> String {
        format!(
            "{} cseq={} bid={}",
            self.at.fields(),
            opt(self.cseq),
            bid_field(self.bid.get())
        )
    }

    /// The join's kind, and the bid of the entry it matched, taken from the
    /// row before any later step can remove that entry.
    pub(crate) fn joined(&self, join: &CombinedJoin, row: &CombinedRow) {
        self.join.set(match join {
            CombinedJoin::Ordinary => "ordinary",
            CombinedJoin::Held(_) => "held",
            CombinedJoin::Failed(_) => "failed",
            CombinedJoin::Debt(_) => "debt",
            CombinedJoin::Invariant(_) => "invariant",
        });
        self.bid.set(row.witness.joined_bid);
    }

    /// Entry to the existing terminal request, whatever it returns.
    pub(crate) fn terminal_requested(&self) {
        record("terminal_requested", format_args!("{}", self.fields()));
    }

    pub(crate) fn terminal_result(&self, ok: bool) {
        self.terminal.set(if ok { "ok" } else { "error" });
    }

    pub(crate) fn checked(&self, checked: &Result<(), CombinedAcceptError>) {
        self.check.set(match checked {
            Ok(()) => "ok",
            Err(error) => accept_error(error),
        });
    }

    pub(crate) fn accept_refused(&self, error: &CombinedAcceptError) {
        self.accept.set(accept_error(error));
    }

    /// The arm's decline: `Ok` records the Release attempt the row made,
    /// `Err` is the not-Held invariant, which attempts none.
    pub(crate) fn declined<E>(&self, declined: &Result<(), E>, row: &CombinedRow) {
        if declined.is_err() {
            self.decline.set("not_held");
            return;
        }
        self.decline.set("ok");
        if let Some((outcome, keep, state_after)) = &row.witness.declined {
            record(
                "release_attempt",
                format_args!(
                    "{} site=arm outcome={outcome:?} keep={keep} state_after={state_after}",
                    self.fields()
                ),
            );
        }
    }

    /// The identity the submission step records for an ordinary entry.
    pub(crate) fn entry(&self) -> WitnessEntry {
        WitnessEntry {
            at: self.at,
            cseq: self.cseq,
            bid: self.bid.get(),
        }
    }
}

impl Drop for ConsumeWitness {
    fn drop(&mut self) {
        let entry = if std::thread::panicking() {
            "unwinding"
        } else {
            self.entry.get()
        };
        record(
            "consume",
            format_args!(
                "{} join={} native={} deadline={} check={} classify={} accept={} decline={} terminal={} entry={entry}",
                self.fields(),
                self.join.get(),
                self.native.get(),
                self.deadline.get(),
                self.check.get(),
                self.classify.get(),
                self.accept.get(),
                self.decline.get(),
                self.terminal.get(),
            ),
        );
    }
}

/// The submission step's handoff of a frame to the ordinary install (spec 04
/// R1): with the row lent, the arm's identity; without it, `row=absent`.
pub(crate) fn ordinary_entry(entry: Option<WitnessEntry>) {
    match entry {
        Some(entry) => record(
            "ordinary_entry",
            format_args!(
                "{} cseq={} bid={} row=lent",
                entry.at.fields(),
                opt(entry.cseq),
                bid_field(entry.bid)
            ),
        ),
        None => record("ordinary_entry", format_args!("cseq=none row=absent")),
    }
}

pub(crate) fn capture_begin(iteration: Option<u64>) {
    record(
        "capture_begin",
        format_args!("iteration={}", opt(iteration)),
    );
}

pub(crate) fn native_begin(iteration: Option<u64>, site: &str) {
    record(
        "native_begin",
        format_args!("iteration={} site={site}", opt(iteration)),
    );
}

pub(crate) fn native_end(iteration: Option<u64>, site: &str, ok: bool) {
    let result = if ok { "ok" } else { "error" };
    record(
        "native_end",
        format_args!("iteration={} site={site} result={result}", opt(iteration)),
    );
}

/// The component pass begins: the slots expected to be visited, read before
/// the poll and independently of the hooks (spec 04 R3).
pub(crate) fn cpass_begin(pass: WitnessPass, native: bool, paused: bool, slots: &[usize]) {
    let native = if native { "present" } else { "absent" };
    record(
        "cpass_begin",
        format_args!(
            "{} native={native} paused={paused} slots={}",
            pass.fields(),
            slot_list(slots)
        ),
    );
}

pub(crate) fn cpass_end(pass: WitnessPass, exit: &str, poll_failed: bool, slots_after: &[usize]) {
    let poll = if poll_failed { "failed" } else { "ok" };
    record(
        "cpass_end",
        format_args!(
            "{} exit={exit} poll={poll} slots_after={}",
            pass.fields(),
            slot_list(slots_after)
        ),
    );
}

/// The absent-components inventory: no headers follow.
pub(crate) fn inventory_absent() {
    record("inventory_begin", format_args!("slots=none"));
}

/// The terminal record of the production capture, after the inventory.
pub(crate) fn capture_end(
    iteration: Option<u64>,
    cpass: Option<Option<u64>>,
    error: bool,
    slots: Option<usize>,
) {
    let exit = if error { "error" } else { "normal" };
    let cpass = cpass.map_or_else(|| "none".to_owned(), opt);
    let slots = slots.map_or_else(|| "none".to_owned(), |slots| slots.to_string());
    record(
        "capture_end",
        format_args!(
            "iteration={} cpass={cpass} exit={exit} slots={slots}",
            opt(iteration)
        ),
    );
}

/// The fixture envelope (spec 04 R2): opened once by the witness child.
pub(crate) fn fixture_begin() {
    record("fixture_begin", format_args!("owner=child"));
}

pub(crate) fn fixture_pass_begin(pass: WitnessPass, slots: &[usize]) {
    record(
        "fixture_pass_begin",
        format_args!(
            "{} native=present paused=false slots={}",
            pass.fields(),
            slot_list(slots)
        ),
    );
}

pub(crate) fn fixture_pass_end(pass: WitnessPass) {
    record("fixture_pass_end", format_args!("{}", pass.fields()));
}

/// The terminal record of a fixture stream, after the child's inventory.
pub(crate) fn fixture_end(pass: WitnessPass, slots: usize) {
    record(
        "fixture_end",
        format_args!("{} slots={slots}", pass.fields()),
    );
}

impl ShellComponentConnections {
    /// The slots the hooked images step is expected to visit: a connection
    /// with an image owner in phase Connected, read from the connections
    /// themselves, not from any hook (spec 04 R3).
    pub(crate) fn witness_expected_slots(&self) -> Vec<usize> {
        self.connections
            .iter()
            .enumerate()
            .filter(|(_, connection)| {
                connection.images.is_some()
                    && matches!(
                        connection.attempt,
                        Some((_, ComponentConnectionPhase::Connected))
                    )
            })
            .map(|(slot, _)| slot)
            .collect()
    }

    /// The closing inventory (spec 04 R2): `inventory_begin`, then for every
    /// slot in order its header and exactly its jobs, open admission and
    /// cancellation obligations, duplicates preserved. The caller emits the
    /// terminal record. Read-only; the number of slots.
    pub(crate) fn witness_inventory(&self) -> usize {
        let slots = self.connections.len();
        record("inventory_begin", format_args!("slots={slots}"));
        for (slot, connection) in self.connections.iter().enumerate() {
            let row = &connection.combined;
            record(
                "retained_row",
                format_args!(
                    "slot={slot} jobs={} open={} cancels={}",
                    row.jobs.len(),
                    usize::from(row.open.is_some()),
                    row.cancels.len(),
                ),
            );
            for job in &row.jobs {
                record(
                    "retained",
                    format_args!(
                        "slot={slot} bid={} state={} verdict={:?}",
                        opt(job.bid),
                        state_label(&job.state),
                        job.verdict,
                    ),
                );
            }
            if let Some(open) = &row.open {
                record(
                    "retained_open",
                    format_args!("slot={slot} ticket={}", open.ticket),
                );
            }
            for (grant, head) in &row.cancels {
                record(
                    "retained_cancel",
                    format_args!(
                        "slot={slot} head={} grant={}",
                        head_identity(head),
                        grant_field(*grant)
                    ),
                );
            }
        }
        slots
    }
}

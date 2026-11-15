//! Gate 4 C3b: the one reader of a witness stream (spec 03 section 5.3,
//! amended by spec 04 R1-R3, corrected per source-01 review R2-R5). It is a
//! consistency checker over the fixed record grammar; it carries no
//! acceptance meaning. Every line that is not a witness record is ignored.
//! The outcome separates REJECTION (malformed, incomplete or contradictory
//! stream: nothing can be judged), VIOLATION (a contract a complete stream
//! breaks) and INSUFFICIENT evidence (a predicate no pass of this stream
//! could test), which the summary lists. The slot count is pinned by the
//! caller: an untrusted stream cannot choose its own obligations away.
use std::collections::{BTreeMap, BTreeSet};

const PREFIX: &str = "sophia_combined_witness schema=1 kind=";
const RELEASES: [&str; 5] = [
    "Transferred",
    "Pending",
    "Disconnected",
    "Mismatch",
    "NotFound",
];
const DISPOSITIONS: [&str; 5] = ["Rendered", "RenderFailed", "Expired", "Stopped", "Mismatch"];
const REFUSALS: [&str; 5] = [
    "refused:Runtime",
    "refused:Output",
    "refused:RetirementClaim",
    "refused:Native",
    "refused:Unchanged",
];
/// Spec 04 R3: the exits after which the normal walk has run.
const WALKED_NORMAL: [&str; 5] = [
    "normal",
    "error:walk_fail_service",
    "error:service_execution",
    "error:role_loop_service",
    "error:settle_revocations",
];
const WALKED_PAUSED: [&str; 2] = ["paused", "error:service_execution_paused"];
const EXITS: [&str; 17] = [
    "error:presentation_available",
    "error:negotiation",
    "error:settle_revocations_early",
    "error:reconcile_early",
    "error:service_execution_paused",
    "paused",
    "error:root_bounds",
    "error:role_loop",
    "error:completion",
    "error:reconcile",
    "error:walk_fail_service",
    "error:role_loop_service",
    "error:service_execution",
    "error:settle_revocations",
    "normal",
    "error:unlabelled",
    "unwinding",
];
const INVENTORY: [&str; 4] = [
    "retained_row",
    "retained",
    "retained_open",
    "retained_cancel",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Record {
    /// 1-based position among the witness records.
    pub(super) index: usize,
    pub(super) kind: String,
    pub(super) fields: BTreeMap<String, String>,
}

impl Record {
    pub(super) fn get(&self, key: &str) -> &str {
        self.fields.get(key).map_or("", String::as_str)
    }

    fn num(&self, key: &str) -> Option<u64> {
        self.get(key).parse().ok()
    }

    fn bid(&self) -> Option<u64> {
        self.num("bid")
    }

    fn pass(&self) -> (Option<u64>, Option<u64>) {
        (self.num("iteration"), self.num("cpass"))
    }

    /// The consume identity a shared cseq binds: pass, slot and bid text.
    fn identity(&self) -> (Option<u64>, Option<u64>, Option<u64>, &str) {
        (
            self.num("iteration"),
            self.num("cpass"),
            self.num("slot"),
            self.get("bid"),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Finding {
    Reject(String),
    Violation(String),
}

/// One component or fixture pass: its two boundary records and the
/// records between them.
#[derive(Clone, Debug, Default)]
pub(super) struct Pass {
    pub(super) iteration: u64,
    pub(super) cpass: u64,
    pub(super) native: bool,
    pub(super) paused: bool,
    pub(super) slots: Vec<usize>,
    pub(super) slots_after: Vec<usize>,
    pub(super) exit: String,
    pub(super) poll_ok: bool,
    /// Indices into the record list, begin and end excluded.
    pub(super) records: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Summary {
    pub(super) fixture: bool,
    pub(super) expected_slots: usize,
    pub(super) passes: Vec<Pass>,
    pub(super) bindings: usize,
    pub(super) consumes: usize,
    /// The live set at the terminal record.
    pub(super) live: BTreeSet<u64>,
    /// (slot, bid, state, verdict) in inventory order.
    pub(super) retained: Vec<(usize, u64, String, String)>,
    pub(super) retained_open: usize,
    pub(super) retained_cancels: usize,
    /// Live set equal to the retained bids: the retention statement holds.
    pub(super) retention_demonstrated: bool,
    pub(super) insufficient: Vec<String>,
    pub(super) notes: Vec<String>,
}

/// The witness records of `lines`, in order; other lines are skipped.
pub(super) fn parse<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Vec<Record>, Vec<Finding>> {
    let mut records = Vec::new();
    let mut findings = Vec::new();
    for line in lines {
        let Some(rest) = line.strip_prefix(PREFIX) else {
            continue;
        };
        let index = records.len() + 1;
        let mut tokens = rest.split(' ');
        let kind = tokens.next().unwrap_or("").to_owned();
        let mut fields = BTreeMap::new();
        for token in tokens {
            let Some((key, value)) = token.split_once('=') else {
                findings.push(Finding::Reject(format!(
                    "record {index}: malformed token {token:?}"
                )));
                continue;
            };
            if value == "exhausted" {
                findings.push(Finding::Reject(format!("record {index}: {key} exhausted")));
            }
            if fields.insert(key.to_owned(), value.to_owned()).is_some() {
                findings.push(Finding::Reject(format!(
                    "record {index}: duplicate field {key}"
                )));
            }
        }
        records.push(Record {
            index,
            kind,
            fields,
        });
    }
    if findings.is_empty() {
        Ok(records)
    } else {
        Err(findings)
    }
}

/// Spec 03 sections 1-4 and spec 04 R1-R3 over one complete stream, with
/// `expected_slots` the pinned slot count (MAX_SHELL_COMPONENTS in
/// production and in the fixture).
pub(super) fn check(records: &[Record], expected_slots: usize) -> Result<Summary, Vec<Finding>> {
    let mut c = Checker {
        expected: expected_slots,
        ..Checker::default()
    };
    c.summary.expected_slots = expected_slots;
    c.grammar(records);
    if c.findings.is_empty() {
        c.envelope(records);
    }
    if c.findings.is_empty() {
        c.passes(records);
        c.live_set(records);
        c.cseq(records);
        c.inventory(records);
    }
    if c.findings.is_empty() {
        c.boundaries(records);
    }
    if c.findings.is_empty() {
        c.obligations(records);
        c.compare();
    }
    if c.findings.is_empty() {
        Ok(c.summary)
    } else {
        Err(c.findings)
    }
}

#[derive(Default)]
struct Checker {
    findings: Vec<Finding>,
    summary: Summary,
    expected: usize,
    /// bid -> (serial, job, slot); every bid ever bound.
    bound: BTreeMap<u64, (u64, u64, u64)>,
    /// Iterations in stream order over passes and native services.
    first_iteration: Option<u64>,
    last_iteration: Option<u64>,
    /// The inventory's declared slot count, `None` for components absent.
    inventory_slots: Option<usize>,
}

fn list(value: &str) -> Option<Vec<usize>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    inner.split(',').map(|s| s.parse().ok()).collect()
}

/// `(n,n,...)` with exactly `arity` numeric components.
fn pair(value: &str, arity: usize) -> bool {
    let Some(inner) = value.strip_prefix('(').and_then(|v| v.strip_suffix(')')) else {
        return false;
    };
    let parts: Vec<&str> = inner.split(',').collect();
    parts.len() == arity && parts.iter().all(|p| p.parse::<u64>().is_ok())
}

fn state(value: &str) -> bool {
    matches!(
        value,
        "InFlight" | "Unsettled" | "Credit" | "Undelivered" | "Held"
    ) || value
        .strip_prefix("Failed:")
        .is_some_and(|d| DISPOSITIONS.contains(&d))
        || value
            .strip_prefix("Debt:")
            .is_some_and(|r| RELEASES.contains(&r))
}

fn collect_result(value: &str) -> bool {
    matches!(value, "none" | "route_error" | "visit")
        || matches!(
            value,
            "returned_plan:confirmed" | "returned_plan:unconfirmed"
        )
        || ["taken:", "retry_settle:", "retry_credit:", "retry_deliver:"]
            .iter()
            .any(|p| {
                value
                    .strip_prefix(p)
                    .is_some_and(|d| DISPOSITIONS.contains(&d))
            })
}

impl Checker {
    fn reject(&mut self, r: &Record, what: &str) {
        self.findings.push(Finding::Reject(format!(
            "record {} {}: {what}",
            r.index, r.kind
        )));
    }

    fn violation(&mut self, what: String) {
        self.findings.push(Finding::Violation(what));
    }

    /// Exactly the fields of the kind, each from its closed set, every slot
    /// within the pinned count.
    fn grammar(&mut self, records: &[Record]) {
        for r in records {
            let at: &[&str] = &["iteration", "cpass"];
            let row: &[&str] = &["iteration", "cpass", "slot"];
            let (required, extra): (Vec<&str>, Vec<&str>) = match r.kind.as_str() {
                "capture_begin" => (vec!["iteration"], vec![]),
                "capture_end" => (vec!["iteration", "cpass", "exit", "slots"], vec![]),
                "fixture_begin" => (vec!["owner"], vec![]),
                "fixture_end" => ([at, &["slots"][..]].concat(), vec![]),
                "cpass_begin" | "fixture_pass_begin" => {
                    ([at, &["native", "paused", "slots"][..]].concat(), vec![])
                }
                "cpass_end" => ([at, &["exit", "poll", "slots_after"][..]].concat(), vec![]),
                "fixture_pass_end" => (at.to_vec(), vec![]),
                "native_begin" => (vec!["iteration", "site"], vec![]),
                "native_end" => (vec!["iteration", "site", "result"], vec![]),
                "images_visit" => ([row, &["hooked"][..]].concat(), vec![]),
                "walk_visit" => ([row, &["jobs", "open", "cancels"][..]].concat(), vec![]),
                "binding" => (
                    [
                        row,
                        &["bid", "ticket", "serial", "job", "grant", "head"][..],
                    ]
                    .concat(),
                    vec![],
                ),
                "collect" => (
                    [row, &["bid", "result", "state_after"][..]].concat(),
                    vec![],
                ),
                "removed" => ([row, &["bid", "cause"][..]].concat(), vec![]),
                "release_attempt" => (
                    [
                        row,
                        &["bid", "site", "cseq", "outcome", "keep", "state_after"][..],
                    ]
                    .concat(),
                    vec![],
                ),
                "reserve_cancelled" => {
                    ([row, &["ticket", "head", "confirmed"][..]].concat(), vec![])
                }
                "cancel_retry" => ([row, &["head", "confirmed"][..]].concat(), vec![]),
                "consume" => (
                    [
                        row,
                        &[
                            "cseq", "bid", "join", "native", "deadline", "check", "classify",
                            "accept", "decline", "terminal", "entry",
                        ][..],
                    ]
                    .concat(),
                    vec![],
                ),
                "terminal_requested" => ([row, &["cseq", "bid"][..]].concat(), vec![]),
                "ordinary_entry" => (
                    vec!["cseq", "row"],
                    vec!["iteration", "cpass", "slot", "bid"],
                ),
                "inventory_begin" => (vec!["slots"], vec![]),
                "retained_row" => (vec!["slot", "jobs", "open", "cancels"], vec![]),
                "retained" => (vec!["slot", "bid", "state", "verdict"], vec![]),
                "retained_open" => (vec!["slot", "ticket"], vec![]),
                "retained_cancel" => (vec!["slot", "head", "grant"], vec![]),
                _ => {
                    self.reject(r, "unknown kind");
                    continue;
                }
            };
            for key in &required {
                if !r.fields.contains_key(*key) {
                    self.reject(r, &format!("missing {key}"));
                }
            }
            for key in r.fields.keys() {
                if !required.contains(&key.as_str()) && !extra.contains(&key.as_str()) {
                    self.reject(r, &format!("unexpected {key}"));
                }
            }
            self.values(r);
        }
    }

    fn values(&mut self, r: &Record) {
        let expected = self.expected;
        let slot_list = |v: &str| {
            list(v).is_some_and(|slots| {
                slots.iter().all(|s| *s < expected) && slots.windows(2).all(|w| w[0] < w[1])
            })
        };
        let mut bad = Vec::new();
        for (key, value) in &r.fields {
            let v = value.as_str();
            let ok = match key.as_str() {
                "iteration" | "ticket" | "serial" | "job" | "jobs" | "cancels" => {
                    v.parse::<u64>().is_ok()
                }
                "cpass" if r.kind == "capture_end" => v == "none" || v.parse::<u64>().is_ok(),
                "cpass" => v.parse::<u64>().is_ok(),
                "slot" => v.parse::<usize>().is_ok_and(|s| s < expected),
                "open" => matches!(v, "0" | "1"),
                "bid" | "cseq" => v == "none" || v.parse::<u64>().is_ok(),
                "slots" | "slots_after" => {
                    if matches!(
                        r.kind.as_str(),
                        "cpass_begin" | "fixture_pass_begin" | "cpass_end"
                    ) {
                        slot_list(v)
                    } else {
                        v == "none" || v.parse::<usize>().is_ok_and(|s| s == expected)
                    }
                }
                // A pass declares native present or absent; only the consume
                // table has a `none` (the arm never reached the native step).
                "native" if r.kind == "consume" => matches!(v, "present" | "absent" | "none"),
                "native" => matches!(v, "present" | "absent"),
                "paused" | "keep" | "confirmed" => matches!(v, "true" | "false"),
                "hooked" => v == "true",
                "poll" => matches!(v, "ok" | "failed"),
                "exit" if r.kind == "capture_end" => matches!(v, "normal" | "error"),
                "exit" => EXITS.contains(&v),
                "owner" => v == "child",
                "site" if r.kind == "release_attempt" => {
                    matches!(v, "arm" | "walk_held_ended" | "walk_debt" | "walk_dispose")
                }
                "site" => matches!(v, "authority" | "lifecycle"),
                "result" if r.kind == "native_end" => matches!(v, "ok" | "error"),
                "result" => collect_result(v),
                "state_after" => v == "removed" || state(v),
                "state" => state(v),
                "verdict" => matches!(v, "Open" | "Expired" | "Consumed"),
                "cause" => matches!(v, "accepted" | "consumed_failed" | "ended" | "retired"),
                "outcome" => RELEASES.contains(&v),
                "grant" => pair(v, 2),
                "head" => pair(v, 3),
                "join" => matches!(v, "ordinary" | "invariant" | "failed" | "debt" | "held"),
                "deadline" => matches!(v, "ok" | "past" | "none"),
                "check" | "accept" => matches!(v, "none" | "ok") || REFUSALS.contains(&v),
                "classify" => matches!(
                    v,
                    "none" | "match" | "churn" | "topology" | "candidate_failure" | "error"
                ),
                "decline" => matches!(v, "none" | "ok" | "not_held"),
                "terminal" => matches!(v, "none" | "ok" | "error"),
                "entry" => matches!(
                    v,
                    "ordinary" | "accepted" | "finished" | "error" | "unwinding"
                ),
                "row" => matches!(v, "lent" | "absent"),
                _ => true,
            };
            if !ok {
                bad.push(format!("{key}={value}"));
            }
        }
        for b in bad {
            self.reject(r, &format!("value out of set: {b}"));
        }
        match r.kind.as_str() {
            "release_attempt" => {
                let removed = r.get("state_after") == "removed";
                if (r.get("keep") == "false") != removed {
                    self.reject(r, "keep and state_after disagree");
                }
                // The arm's release carries its consume's cseq; a walk site has none.
                let arm = r.get("site") == "arm";
                if arm != r.num("cseq").is_some() || (!arm && r.get("cseq") != "none") {
                    self.reject(r, "cseq is numeric at the arm site and none at a walk site");
                }
                if r.get("bid") == "none" {
                    self.reject(r, "a release names its job's bid");
                }
            }
            "collect" => {
                let returned = r.get("result").starts_with("returned_plan:");
                if returned != (r.get("state_after") == "removed") {
                    self.reject(r, "returned_plan and state_after=removed imply each other");
                }
            }
            "ordinary_entry" => {
                let lent = r.get("row") == "lent";
                let has = ["iteration", "cpass", "slot", "bid"]
                    .iter()
                    .all(|k| r.fields.contains_key(*k));
                if lent != has || lent != r.num("cseq").is_some() {
                    self.reject(
                        r,
                        "row=lent carries pass, slot, cseq and bid; row=absent none of them",
                    );
                }
            }
            "capture_end" => {
                if (r.get("cpass") == "none") != (r.get("slots") == "none") {
                    self.reject(r, "cpass=none and slots=none imply each other");
                }
            }
            "cpass_end" => {
                if r.get("exit") == "unwinding" {
                    self.reject(r, "incomplete: the pass unwound");
                }
                // Spec 04 R3: an unwrapped outer exit is an implementation
                // omission; the pass is incomplete evidence, never a verdict.
                if r.get("exit") == "error:unlabelled" {
                    self.reject(r, "incomplete: unlabelled exit, an unwrapped outer exit");
                }
            }
            "consume" => {
                if r.get("entry") == "unwinding" {
                    self.reject(r, "incomplete: the consume unwound");
                }
            }
            _ => {}
        }
    }

    /// One envelope kind, begun by the first record and closed by the last.
    fn envelope(&mut self, records: &[Record]) {
        let Some(first) = records.first() else {
            self.findings
                .push(Finding::Reject("incomplete: empty stream".into()));
            return;
        };
        let fixture = match first.kind.as_str() {
            "fixture_begin" => true,
            "capture_begin" => false,
            _ => {
                self.reject(first, "the stream does not begin with an envelope");
                return;
            }
        };
        self.summary.fixture = fixture;
        let end = if fixture {
            "fixture_end"
        } else {
            "capture_end"
        };
        let last = records.last().unwrap();
        if last.kind != end {
            self.reject(last, &format!("incomplete: the last record is not {end}"));
        }
        for r in records {
            let production = matches!(
                r.kind.as_str(),
                "capture_begin"
                    | "capture_end"
                    | "cpass_begin"
                    | "cpass_end"
                    | "native_begin"
                    | "native_end"
            );
            let fixt = r.kind.starts_with("fixture_");
            if (fixture && production) || (!fixture && fixt) {
                self.reject(r, "mixed envelopes");
            }
            if (r.kind == first.kind && r.index != first.index)
                || (r.kind == end && r.index != last.index)
            {
                self.reject(r, "a second envelope boundary");
            }
        }
    }

    fn iteration_seen(&mut self, r: &Record, iteration: u64) {
        if self.first_iteration.is_none() {
            self.first_iteration = Some(iteration);
        }
        if self.last_iteration.is_some_and(|last| iteration < last) {
            self.reject(r, "iteration goes backwards");
        }
        self.last_iteration = Some(iteration);
    }

    /// Pass pairing, nesting, the +1 pass sequence from one, iteration
    /// order over passes and native services, and every row record's pass.
    fn passes(&mut self, records: &[Record]) {
        let (begin, end) = if self.summary.fixture {
            ("fixture_pass_begin", "fixture_pass_end")
        } else {
            ("cpass_begin", "cpass_end")
        };
        let mut open: Option<Pass> = None;
        let mut native_open: Option<(u64, String)> = None;
        let mut last_cpass: u64 = 0;
        for (i, r) in records.iter().enumerate() {
            let kind = r.kind.as_str();
            if kind == begin {
                if open.is_some() {
                    self.reject(r, "a pass begins inside a pass");
                }
                if native_open.is_some() {
                    self.reject(r, "a pass begins inside native service");
                }
                let (Some(iteration), Some(cpass)) = r.pass() else {
                    self.reject(r, "pass without identity");
                    continue;
                };
                // Checked: a numeric maximum is a malformed counter, rejected
                // without advancing the trusted sequence or overflowing.
                if last_cpass.checked_add(1) == Some(cpass) {
                    last_cpass = cpass;
                } else {
                    self.reject(r, "pass counters do not advance by one");
                }
                if self.summary.fixture && iteration != cpass {
                    self.reject(r, "fixture passes have iteration = cpass");
                }
                self.iteration_seen(r, iteration);
                open = Some(Pass {
                    iteration,
                    cpass,
                    native: r.get("native") == "present",
                    paused: r.get("paused") == "true",
                    slots: list(r.get("slots")).unwrap_or_default(),
                    exit: String::new(),
                    poll_ok: true,
                    ..Pass::default()
                });
                continue;
            }
            if kind == end {
                let Some(mut pass) = open.take() else {
                    self.reject(r, "a pass ends without beginning");
                    continue;
                };
                if r.pass() != (Some(pass.iteration), Some(pass.cpass)) {
                    self.reject(r, "pass end identity differs from its begin");
                }
                if self.summary.fixture {
                    pass.exit = "normal".into();
                    pass.slots_after = pass.slots.clone();
                } else {
                    pass.exit = r.get("exit").to_owned();
                    pass.poll_ok = r.get("poll") == "ok";
                    pass.slots_after = list(r.get("slots_after")).unwrap_or_default();
                }
                self.summary.passes.push(pass);
                continue;
            }
            match kind {
                "native_begin" => {
                    if open.is_some() || native_open.is_some() {
                        self.reject(r, "native service inside a pass or another native service");
                    }
                    let iteration = r.num("iteration").unwrap_or(0);
                    self.iteration_seen(r, iteration);
                    native_open = Some((iteration, r.get("site").to_owned()));
                }
                "native_end" => match native_open.take() {
                    Some((iteration, site))
                        if Some(iteration) == r.num("iteration") && site == r.get("site") => {}
                    _ => self.reject(r, "native_end without its native_begin"),
                },
                "capture_begin" | "capture_end" | "fixture_begin" | "fixture_end"
                | "inventory_begin" | "retained_row" | "retained" | "retained_open"
                | "retained_cancel" => {
                    if open.is_some() {
                        self.reject(r, "boundary or inventory record inside a pass");
                    }
                    if native_open.is_some() {
                        self.reject(r, "boundary or inventory record inside native service");
                    }
                }
                _ => match &mut open {
                    Some(pass) => {
                        if (kind != "ordinary_entry" || r.get("row") == "lent")
                            && r.pass() != (Some(pass.iteration), Some(pass.cpass))
                        {
                            self.reject(r, "record names another pass than the open one");
                        }
                        pass.records.push(i);
                    }
                    None => self.reject(r, "row record outside any pass"),
                },
            }
        }
        if open.is_some() {
            self.findings.push(Finding::Reject(
                "incomplete: the last pass has no end".into(),
            ));
        }
        if native_open.is_some() {
            self.findings.push(Finding::Reject(
                "incomplete: native service has no end".into(),
            ));
        }
    }

    /// Spec 03 2.1 and the identity rules of section 1; every record that
    /// names a bid names the slot that bound it.
    fn live_set(&mut self, records: &[Record]) {
        let mut live: BTreeSet<u64> = BTreeSet::new();
        let mut last_bid = 0;
        for r in records {
            match r.kind.as_str() {
                "binding" => {
                    let (Some(bid), Some(serial), Some(job), Some(slot)) =
                        (r.bid(), r.num("serial"), r.num("job"), r.num("slot"))
                    else {
                        self.reject(r, "binding without a bid");
                        continue;
                    };
                    if bid <= last_bid {
                        self.reject(r, "bids are not strictly increasing");
                    }
                    last_bid = bid;
                    if live.iter().any(|b| {
                        self.bound
                            .get(b)
                            .is_some_and(|(s, j, _)| *s == serial && *j == job)
                    }) {
                        self.reject(r, "(serial, job) repeats while the first bid is live");
                    }
                    if self.bound.insert(bid, (serial, job, slot)).is_some() || !live.insert(bid) {
                        self.reject(r, "duplicate bid");
                    }
                    self.summary.bindings += 1;
                }
                "collect" | "removed" | "release_attempt" => {
                    let Some(bid) = r.bid() else {
                        self.reject(r, "no bid");
                        continue;
                    };
                    if !live.contains(&bid) {
                        self.reject(r, "bid is not live");
                        continue;
                    }
                    self.same_slot(r, bid);
                    let removes = (r.kind == "removed")
                        || r.get("result").starts_with("returned_plan:")
                        || (r.kind == "release_attempt" && r.get("keep") == "false");
                    if removes {
                        live.remove(&bid);
                    }
                }
                "consume" | "terminal_requested" | "ordinary_entry" | "retained" => {
                    if let Some(bid) = r.bid() {
                        if !self.bound.contains_key(&bid) {
                            self.reject(r, "bid was never bound");
                        } else {
                            self.same_slot(r, bid);
                        }
                    }
                    if r.kind == "consume" {
                        self.summary.consumes += 1;
                    }
                }
                _ => {}
            }
        }
        self.summary.live = live;
    }

    fn same_slot(&mut self, r: &Record, bid: u64) {
        if let Some((_, _, slot)) = self.bound.get(&bid).copied() {
            if r.num("slot") != Some(slot) {
                self.reject(r, "the bid belongs to another slot");
            }
        }
    }

    /// Spec 03 3.1: the consume table, and the records a shared cseq binds
    /// to the consume's exact identity, in exact numbers.
    fn cseq(&mut self, records: &[Record]) {
        let mut consumes: BTreeMap<u64, &Record> = BTreeMap::new();
        let mut arm: BTreeMap<u64, Vec<&Record>> = BTreeMap::new();
        let mut terminals: BTreeMap<u64, Vec<&Record>> = BTreeMap::new();
        let mut entries: BTreeMap<u64, Vec<&Record>> = BTreeMap::new();
        let mut last = 0;
        for r in records {
            let cseq = r.num("cseq");
            match r.kind.as_str() {
                "consume" => {
                    let Some(c) = cseq else {
                        self.reject(r, "consume without cseq");
                        continue;
                    };
                    if c <= last {
                        self.reject(r, "cseq is not strictly increasing");
                    }
                    last = c;
                    consumes.insert(c, r);
                    self.table(r);
                }
                "release_attempt" if r.get("site") == "arm" => {
                    arm.entry(cseq.unwrap_or(0)).or_default().push(r);
                }
                "terminal_requested" => terminals.entry(cseq.unwrap_or(0)).or_default().push(r),
                "ordinary_entry" if r.get("row") == "lent" => {
                    entries.entry(cseq.unwrap_or(0)).or_default().push(r);
                }
                _ => {}
            }
        }
        for (c, consume) in &consumes {
            let release = arm.remove(c).unwrap_or_default();
            let terminal = terminals.remove(c).unwrap_or_default();
            let entry = entries.remove(c).unwrap_or_default();
            let want_release = usize::from(consume.get("decline") == "ok");
            let want_terminal = usize::from(consume.get("terminal") != "none");
            let want_entry = usize::from(consume.get("entry") == "ordinary");
            if release.len() != want_release {
                self.reject(
                    consume,
                    &format!(
                        "decline={} needs exactly {want_release} arm release_attempt, found {}",
                        consume.get("decline"),
                        release.len()
                    ),
                );
            }
            if terminal.len() != want_terminal {
                self.reject(
                    consume,
                    &format!(
                        "terminal={} needs exactly {want_terminal} terminal_requested, found {}",
                        consume.get("terminal"),
                        terminal.len()
                    ),
                );
            }
            if entry.len() != want_entry {
                self.reject(
                    consume,
                    &format!(
                        "entry={} needs exactly {want_entry} ordinary_entry row=lent, found {}",
                        consume.get("entry"),
                        entry.len()
                    ),
                );
            }
            for r in release.iter().chain(&terminal).chain(&entry) {
                if r.identity() != consume.identity() {
                    self.reject(
                        r,
                        "cseq record names another pass, slot or bid than its consume",
                    );
                }
            }
            for r in release.iter().chain(&terminal) {
                if r.index > consume.index {
                    self.reject(r, "arm record after its consume record");
                }
            }
            if let (Some(rel), Some(term)) = (release.first(), terminal.first()) {
                if rel.index > term.index {
                    self.reject(term, "the terminal request precedes the arm release");
                }
            }
            for r in &entry {
                if r.index < consume.index {
                    self.reject(r, "ordinary_entry before its consume record");
                }
            }
        }
        for r in arm
            .values()
            .chain(terminals.values())
            .chain(entries.values())
            .flatten()
        {
            self.reject(r, "cseq names no consume record");
        }
    }

    /// The return table, as implications between the fixed fields.
    fn table(&mut self, r: &Record) {
        let f = |k: &str| r.get(k);
        let all_none = |keys: &[&str]| keys.iter().all(|k| f(k) == "none");
        let fin = |t: &str, e: &str| {
            (t == "ok" && matches!(e, "finished" | "error")) || (t == "error" && e == "error")
        };
        let after_decline = |decline: &str, then: &dyn Fn() -> bool| match decline {
            "not_held" => f("terminal") == "none" && f("entry") == "error",
            "ok" => then(),
            _ => false,
        };
        let refused_tail = |variant: &str| match variant {
            "refused:Runtime" | "refused:Output" => {
                f("terminal") == "none" && f("entry") == "error"
            }
            "refused:RetirementClaim" | "refused:Native" => fin(f("terminal"), f("entry")),
            "refused:Unchanged" => f("terminal") == "none" && f("entry") == "ordinary",
            _ => false,
        };
        let ok = match f("join") {
            "ordinary" => {
                all_none(&[
                    "deadline", "check", "classify", "accept", "decline", "terminal",
                ]) && f("native") == "none"
                    && f("entry") == "ordinary"
            }
            "invariant" => {
                all_none(&[
                    "deadline", "check", "classify", "accept", "decline", "terminal",
                ]) && f("native") == "none"
                    && f("entry") == "error"
            }
            "failed" | "debt" => {
                all_none(&["deadline", "check", "classify", "accept", "decline"])
                    && f("native") == "none"
                    && fin(f("terminal"), f("entry"))
            }
            "held" => match (f("native"), f("deadline")) {
                ("absent", "none") => {
                    all_none(&["check", "classify", "accept", "decline"])
                        && fin(f("terminal"), f("entry"))
                }
                ("present", "none") => {
                    all_none(&["check", "classify", "accept", "decline", "terminal"])
                        && f("entry") == "error"
                }
                ("present", "past") => {
                    all_none(&["check", "classify", "accept"])
                        && after_decline(f("decline"), &|| fin(f("terminal"), f("entry")))
                }
                ("present", "ok") => match f("check") {
                    "ok" => match f("classify") {
                        "error" => {
                            f("accept") == "none"
                                && after_decline(f("decline"), &|| {
                                    f("terminal") == "none" && f("entry") == "error"
                                })
                        }
                        "topology" | "candidate_failure" => {
                            f("accept") == "none"
                                && after_decline(f("decline"), &|| fin(f("terminal"), f("entry")))
                        }
                        "churn" => {
                            f("accept") == "none"
                                && after_decline(f("decline"), &|| {
                                    f("terminal") == "none" && f("entry") == "ordinary"
                                })
                        }
                        "match" => match f("accept") {
                            "ok" => {
                                f("decline") == "none"
                                    && f("terminal") == "none"
                                    && matches!(f("entry"), "accepted" | "error")
                            }
                            refused => after_decline(f("decline"), &|| refused_tail(refused)),
                        },
                        _ => false,
                    },
                    refused if refused != "none" => {
                        f("classify") == "none"
                            && f("accept") == "none"
                            && after_decline(f("decline"), &|| refused_tail(refused))
                    }
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        };
        // A matched entry (held, failed, debt) has a bid; no match has none.
        let bid_ok = match f("join") {
            "ordinary" => f("bid") == "none",
            "held" | "failed" | "debt" => f("bid") != "none",
            _ => true,
        };
        if !ok || !bid_ok {
            self.reject(r, "fields contradict the consume return table");
        }
    }

    /// Spec 04 R2: the inventory phase, checked for completeness against the
    /// pinned slot count before anything is compared with it.
    fn inventory(&mut self, records: &[Record]) {
        let begins: Vec<&Record> = records
            .iter()
            .filter(|r| r.kind == "inventory_begin")
            .collect();
        let [begin] = begins.as_slice() else {
            self.findings.push(Finding::Reject(format!(
                "incomplete: {} inventory_begin records, one required",
                begins.len()
            )));
            return;
        };
        for r in records {
            if INVENTORY.contains(&r.kind.as_str()) && r.index < begin.index {
                self.reject(r, "inventory record before the inventory phase");
            }
        }
        let terminal = records.last().unwrap();
        if terminal.get("slots") != begin.get("slots") {
            self.reject(terminal, "terminal slots differ from inventory_begin");
        }
        let tail = &records[begin.index..records.len() - 1];
        if begin.get("slots") == "none" {
            if self.summary.fixture {
                self.reject(begin, "a fixture inventory names its slots");
            } else if !tail.is_empty() {
                self.reject(begin, "components absent but inventory records follow");
            }
            return;
        }
        // Grammar already pinned a numeric count to the expected one.
        let slots = self.expected;
        self.inventory_slots = Some(slots);
        let mut expected_slot = 0;
        let mut i = 0;
        let mut bids = Vec::new();
        let mut cancels: Vec<(String, String)> = Vec::new();
        while i < tail.len() {
            let header = &tail[i];
            if header.kind != "retained_row" || header.num("slot") != Some(expected_slot as u64) {
                self.reject(
                    header,
                    &format!("expected retained_row slot={expected_slot}"),
                );
                return;
            }
            i += 1;
            let want = [
                ("retained", header.num("jobs").unwrap_or(0)),
                ("retained_open", header.num("open").unwrap_or(0)),
                ("retained_cancel", header.num("cancels").unwrap_or(0)),
            ];
            for (kind, count) in want {
                for _ in 0..count {
                    let Some(r) = tail.get(i) else {
                        self.reject(
                            header,
                            &format!("incomplete: fewer {kind} records than declared"),
                        );
                        return;
                    };
                    if r.kind != kind || r.num("slot") != Some(expected_slot as u64) {
                        self.reject(r, &format!("expected {kind} slot={expected_slot}"));
                        return;
                    }
                    match kind {
                        "retained" => {
                            let Some(bid) = r.bid() else {
                                self.reject(r, "retained without a bid");
                                return;
                            };
                            bids.push(bid);
                            self.summary.retained.push((
                                expected_slot,
                                bid,
                                r.get("state").to_owned(),
                                r.get("verdict").to_owned(),
                            ));
                        }
                        "retained_open" => self.summary.retained_open += 1,
                        _ => {
                            self.summary.retained_cancels += 1;
                            let identity = (r.get("head").to_owned(), r.get("grant").to_owned());
                            if cancels.contains(&identity) {
                                self.summary.notes.push(format!(
                                    "duplicate cancellation identity slot={expected_slot} head={} grant={}",
                                    identity.0, identity.1
                                ));
                            }
                            cancels.push(identity);
                        }
                    }
                    i += 1;
                }
            }
            expected_slot += 1;
        }
        if expected_slot != slots {
            self.findings.push(Finding::Reject(format!(
                "incomplete: {expected_slot} retained_row headers, {slots} declared"
            )));
        }
        let distinct: BTreeSet<u64> = bids.iter().copied().collect();
        if distinct.len() != bids.len() {
            self.findings
                .push(Finding::Reject("a bid is retained twice".into()));
        }
    }

    /// The envelope's counters against what the stream showed: the first
    /// iteration is the capture's, the terminal iteration is not before the
    /// last one seen, and the terminal pass count is the number of passes.
    fn boundaries(&mut self, records: &[Record]) {
        let first = records.first().unwrap();
        let last = records.last().unwrap();
        let passes = self.summary.passes.len() as u64;
        if let Some(begin) = first.num("iteration") {
            if self.first_iteration.is_some_and(|seen| seen < begin) {
                self.reject(first, "capture_begin iteration is after a later record's");
            }
            // With no interior observation the envelope still orders itself.
            if last.num("iteration").is_some_and(|end| end < begin) {
                self.reject(last, "terminal iteration is before the capture's begin");
            }
        }
        if let (Some(end), Some(seen)) = (last.num("iteration"), self.last_iteration) {
            if end < seen {
                self.reject(last, "terminal iteration is before the last one seen");
            }
        }
        if self.summary.fixture {
            if last.pass() != (Some(passes), Some(passes)) {
                self.reject(
                    last,
                    "fixture_end names a pass count other than the passes seen",
                );
            }
        } else if self.inventory_slots.is_none() {
            if passes != 0 {
                self.reject(last, "components absent but component passes were recorded");
            }
        } else if last.num("cpass") != Some(passes) {
            self.reject(last, "capture_end cpass is not the number of passes seen");
        }
    }

    /// Spec 03 4.1 as amended by spec 04 R1/R3, per pass, against the
    /// pinned slot count.
    fn obligations(&mut self, records: &[Record]) {
        let passes = std::mem::take(&mut self.summary.passes);
        let max = self.expected;
        let mut eligible_images = false;
        let mut paused_native = false;
        let mut held_consume = false;
        for pass in &passes {
            let rs: Vec<&Record> = pass.records.iter().map(|i| &records[*i]).collect();
            let at = format!("iteration={} cpass={}", pass.iteration, pass.cpass);
            let visits = |kind: &str| -> Vec<(usize, usize)> {
                rs.iter()
                    .filter(|r| r.kind == kind)
                    .map(|r| (r.num("slot").unwrap_or(0) as usize, r.index))
                    .collect()
            };
            let consumes: Vec<(usize, usize)> = visits("consume");
            held_consume |= rs
                .iter()
                .any(|r| r.kind == "consume" && r.get("join") == "held");
            if !pass.native {
                continue;
            }
            for r in &rs {
                if r.kind == "ordinary_entry" && r.get("row") == "absent" {
                    self.violation(format!("content served without the row: pass {at}"));
                }
            }
            // Images: the slots in both snapshots, when the poll completed.
            if pass.exit != "error:presentation_available" {
                if !pass.poll_ok {
                    self.summary
                        .notes
                        .push(format!("pass {at}: poll failed, images unobserved"));
                } else {
                    let both: Vec<usize> = pass
                        .slots
                        .iter()
                        .copied()
                        .filter(|s| pass.slots_after.contains(s))
                        .collect();
                    eligible_images |= !both.is_empty();
                    let images: Vec<(usize, usize)> = rs
                        .iter()
                        .filter(|r| r.kind == "images_visit" && r.get("hooked") == "true")
                        .map(|r| (r.num("slot").unwrap_or(0) as usize, r.index))
                        .collect();
                    for slot in &both {
                        let n = images.iter().filter(|(s, _)| s == slot).count();
                        if n != 1 {
                            self.violation(format!(
                                "images hook: slot {slot} visited {n} times in pass {at}"
                            ));
                        }
                    }
                    for (slot, _) in &images {
                        if !both.contains(slot) {
                            self.summary
                                .notes
                                .push(format!("pass {at}: extra images visit slot {slot}"));
                        }
                    }
                }
            }
            // Walk: only after an exit that reached it.
            let walked_normal = !pass.paused && WALKED_NORMAL.contains(&pass.exit.as_str());
            let walked_paused = pass.paused && WALKED_PAUSED.contains(&pass.exit.as_str());
            paused_native |= walked_paused;
            if !walked_normal && !walked_paused {
                self.summary
                    .notes
                    .push(format!("pass {at}: ended before the walk ({})", pass.exit));
                continue;
            }
            let walks = visits("walk_visit");
            if walked_paused && !consumes.is_empty() {
                self.violation(format!("paused pass {at} served content"));
            }
            for slot in 0..max {
                let mine: Vec<usize> = walks
                    .iter()
                    .filter(|(s, _)| *s == slot)
                    .map(|(_, i)| *i)
                    .collect();
                if mine.len() != 1 {
                    self.violation(format!(
                        "walk: slot {slot} visited {} times in pass {at}",
                        mine.len()
                    ));
                    continue;
                }
                if consumes.iter().any(|(s, i)| *s == slot && *i > mine[0]) {
                    self.violation(format!("walk before content: slot {slot} in pass {at}"));
                }
            }
            for r in &rs {
                if r.kind == "collect"
                    && r.get("state_after") == "Held"
                    && !consumes
                        .iter()
                        .any(|(s, _)| Some(*s as u64) == r.num("slot"))
                {
                    self.summary.insufficient.push(format!(
                        "held result not consumed in pass {at}, slot {}",
                        r.get("slot")
                    ));
                }
            }
        }
        if !eligible_images {
            self.summary
                .insufficient
                .push("PM1: no native pass with an obligated slot".into());
        }
        if !paused_native {
            self.summary
                .insufficient
                .push("PM4: no paused native pass reached its walk".into());
        }
        if !held_consume {
            self.summary
                .insufficient
                .push("PM3: no consume with join=held".into());
        }
        self.summary.passes = passes;
    }

    /// Spec 04 R2: the live set against the complete inventory.
    fn compare(&mut self) {
        let retained: BTreeSet<u64> = self.summary.retained.iter().map(|r| r.1).collect();
        if retained == self.summary.live {
            self.summary.retention_demonstrated = true;
        } else {
            self.violation(format!(
                "live-set mismatch: live {:?}, retained {:?}",
                self.summary.live, retained
            ));
        }
    }
}

// The reader's own control over synthetic streams, in its own file for the
// source-layout limit.
#[path = "combined_witness_reader_cases.rs"]
mod cases;

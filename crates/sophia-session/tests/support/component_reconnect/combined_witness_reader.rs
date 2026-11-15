//! Gate 4 C3b: the one reader of a witness stream (spec 03 section 5.3,
//! amended by spec 04 R1-R3). It is a consistency checker over the fixed
//! record grammar; it carries no acceptance meaning. Every line that is not
//! a witness record is ignored. The outcome separates REJECTION (malformed,
//! incomplete or contradictory stream: nothing can be judged), VIOLATION (a
//! contract the complete stream breaks) and INSUFFICIENT evidence (a
//! predicate no pass of this stream could test), which the summary lists.
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

/// Spec 03 sections 1-4 and spec 04 R1-R3 over one complete stream.
pub(super) fn check(records: &[Record]) -> Result<Summary, Vec<Finding>> {
    let mut c = Checker::default();
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
    /// bid -> (serial, job, slot); every bid ever bound.
    bound: BTreeMap<u64, (u64, u64, usize)>,
    max_slots: Option<usize>,
}

fn list(value: &str) -> Option<Vec<usize>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    inner.split(',').map(|s| s.parse().ok()).collect()
}

fn pair(value: &str, arity: usize) -> bool {
    value
        .strip_prefix('(')
        .and_then(|v| v.strip_suffix(')'))
        .is_some_and(|v| v.split(',').filter(|p| p.parse::<u64>().is_ok()).count() == arity)
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

    /// Exactly the fields of the kind, each from its closed set.
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
                    [row, &["site", "outcome", "keep", "state_after"][..]].concat(),
                    vec!["bid", "cseq"],
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
        let mut bad = Vec::new();
        for (key, value) in &r.fields {
            let v = value.as_str();
            let ok = match key.as_str() {
                "iteration" | "cpass" | "ticket" | "serial" | "job" | "jobs" | "cancels" => {
                    v.parse::<u64>().is_ok()
                }
                "slot" => v.parse::<usize>().is_ok(),
                "open" => matches!(v, "0" | "1"),
                "bid" => v == "none" || v.parse::<u64>().is_ok(),
                "cseq" => v == "none" || v.parse::<u64>().is_ok(),
                "slots" | "slots_after" => {
                    if r.kind == "cpass_begin"
                        || r.kind == "fixture_pass_begin"
                        || r.kind == "cpass_end"
                    {
                        list(v).is_some()
                    } else {
                        v == "none" || v.parse::<usize>().is_ok()
                    }
                }
                "native" => matches!(v, "present" | "absent" | "none"),
                "paused" | "keep" | "confirmed" | "hooked" => matches!(v, "true" | "false"),
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
                "classify" => {
                    matches!(
                        v,
                        "none" | "match" | "churn" | "topology" | "candidate_failure" | "error"
                    )
                }
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
        if r.kind == "release_attempt" {
            let removed = r.get("state_after") == "removed";
            if (r.get("keep") == "false") != removed {
                self.reject(r, "keep and state_after disagree");
            }
            if (r.get("site") == "arm") != r.fields.contains_key("cseq") {
                self.reject(r, "cseq is the arm site's field");
            }
            if r.get("site") == "arm" && r.get("cseq") == "none" {
                self.reject(r, "arm release without a cseq");
            }
            if !r.fields.contains_key("bid") {
                self.reject(r, "missing bid");
            }
        }
        if r.kind == "collect" {
            let returned = r.get("result").starts_with("returned_plan:");
            if returned != (r.get("state_after") == "removed") {
                self.reject(r, "returned_plan and state_after=removed imply each other");
            }
        }
        if r.kind == "ordinary_entry" {
            let lent = r.get("row") == "lent";
            let has = ["iteration", "cpass", "slot", "bid"]
                .iter()
                .all(|k| r.fields.contains_key(*k));
            if lent != has
                || (!lent && r.get("cseq") != "none")
                || (lent && r.get("cseq") == "none")
            {
                self.reject(
                    r,
                    "row=lent carries pass, slot, cseq and bid; row=absent none of them",
                );
            }
        }
        if r.kind == "cpass_end" && r.get("exit") == "unwinding" {
            self.reject(r, "incomplete: the pass unwound");
        }
        // Spec 04 R3: the declared treatment of no known exit site.
        if r.kind == "cpass_end" && r.get("exit") == "error:unlabelled" {
            self.violation(format!(
                "record {}: unlabelled exit, an unwrapped outer exit",
                r.index
            ));
        }
        if r.kind == "consume" && r.get("entry") == "unwinding" {
            self.reject(r, "incomplete: the consume unwound");
        }
    }

    /// One envelope kind, begun by the first record and closed by the last.
    fn envelope(&mut self, records: &[Record]) {
        let Some(first) = records.first() else {
            self.findings.push(Finding::Reject("empty stream".into()));
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

    /// Pass pairing, nesting, counters and every row record's pass.
    fn passes(&mut self, records: &[Record]) {
        let (begin, end) = if self.summary.fixture {
            ("fixture_pass_begin", "fixture_pass_end")
        } else {
            ("cpass_begin", "cpass_end")
        };
        let mut open: Option<Pass> = None;
        let mut native_open: Option<(u64, String)> = None;
        let mut last_cpass = 0;
        let mut last_iteration = 0;
        for (i, r) in records.iter().enumerate() {
            let kind = r.kind.as_str();
            if kind == begin {
                if open.is_some() {
                    self.reject(r, "a pass begins inside a pass");
                }
                let (Some(iteration), Some(cpass)) = r.pass() else {
                    self.reject(r, "pass without identity");
                    continue;
                };
                if cpass <= last_cpass || iteration < last_iteration {
                    self.reject(r, "pass counters do not advance");
                }
                if self.summary.fixture && (iteration != cpass || cpass != last_cpass + 1) {
                    self.reject(r, "fixture passes count 1, 2, ... with iteration = cpass");
                }
                last_cpass = cpass;
                last_iteration = iteration;
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
                if !self.summary.fixture {
                    pass.exit = r.get("exit").to_owned();
                    pass.poll_ok = r.get("poll") == "ok";
                    pass.slots_after = list(r.get("slots_after")).unwrap_or_default();
                } else {
                    pass.exit = "normal".into();
                    pass.slots_after = pass.slots.clone();
                }
                self.summary.passes.push(pass);
                continue;
            }
            match kind {
                "native_begin" => {
                    if open.is_some() || native_open.is_some() {
                        self.reject(r, "native service inside a pass or another native service");
                    }
                    native_open = Some((r.num("iteration").unwrap_or(0), r.get("site").to_owned()));
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
                        if kind != "ordinary_entry" || r.get("row") == "lent" {
                            if r.pass() != (Some(pass.iteration), Some(pass.cpass)) {
                                self.reject(r, "record names another pass than the open one");
                            }
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

    /// Spec 03 2.1 and the identity rules of section 1.
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
                    if self
                        .bound
                        .insert(bid, (serial, job, slot as usize))
                        .is_some()
                        || !live.insert(bid)
                    {
                        self.reject(r, "duplicate bid");
                    }
                    if live
                        .iter()
                        .any(|b| *b != bid && self.bound[b].0 == serial && self.bound[b].1 == job)
                    {
                        self.reject(r, "(serial, job) repeats while the first bid is live");
                    }
                    self.summary.bindings += 1;
                }
                "collect" | "removed" | "release_attempt" => {
                    let Some(bid) = r.bid() else {
                        if r.kind != "release_attempt" || r.get("bid") != "none" {
                            self.reject(r, "no bid");
                        }
                        continue;
                    };
                    if !live.contains(&bid) {
                        self.reject(r, "bid is not live");
                        continue;
                    }
                    let removes = (r.kind == "removed")
                        || r.get("result").starts_with("returned_plan:")
                        || (r.kind == "release_attempt" && r.get("keep") == "false");
                    if removes {
                        live.remove(&bid);
                    }
                }
                "consume" | "terminal_requested" | "ordinary_entry" => {
                    if let Some(bid) = r.bid() {
                        if !self.bound.contains_key(&bid) {
                            self.reject(r, "bid was never bound");
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

    /// Spec 03 3.1: the consume table and the cseq-associated records.
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
            if (consume.get("decline") == "ok") != (release.len() == 1) {
                self.reject(
                    consume,
                    "decline=ok and exactly one arm release_attempt imply each other",
                );
            }
            if (consume.get("terminal") != "none") != (terminal.len() == 1) {
                self.reject(
                    consume,
                    "terminal=ok|error and exactly one terminal_requested imply each other",
                );
            }
            if (consume.get("entry") == "ordinary") != (entry.len() == 1) {
                self.reject(
                    consume,
                    "entry=ordinary and exactly one ordinary_entry row=lent imply each other",
                );
            }
            for r in release.iter().chain(&terminal) {
                if r.index > consume.index || r.get("bid") != consume.get("bid") {
                    self.reject(r, "arm record after its consume record or with another bid");
                }
            }
            if let (Some(rel), Some(term)) = (release.first(), terminal.first()) {
                if rel.index > term.index {
                    self.reject(term, "the terminal request precedes the arm release");
                }
            }
            for r in &entry {
                if r.index < consume.index
                    || r.get("bid") != consume.get("bid")
                    || r.pass() != consume.pass()
                {
                    self.reject(
                        r,
                        "ordinary_entry before its consume record or with another identity",
                    );
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
                    && f("bid") == "none"
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

    /// Spec 04 R2: the inventory, checked for completeness before anything
    /// is compared with it.
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
        let terminal = records.last().unwrap();
        if terminal.get("slots") != begin.get("slots") {
            self.reject(terminal, "terminal slots differ from inventory_begin");
        }
        let tail = &records[begin.index..records.len() - 1];
        if begin.get("slots") == "none" {
            if !self.summary.fixture && !tail.is_empty() {
                self.reject(begin, "components absent but inventory records follow");
            }
            if self.summary.fixture {
                self.reject(begin, "a fixture inventory names its slots");
            }
            return;
        }
        let slots: usize = begin.num("slots").unwrap_or(0) as usize;
        self.max_slots = Some(slots);
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

    /// Spec 03 4.1 as amended by spec 04 R1/R3, per pass.
    fn obligations(&mut self, records: &[Record]) {
        let passes = std::mem::take(&mut self.summary.passes);
        let max = self.max_slots;
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
                    let images = visits("images_visit");
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
            let Some(max) = max else {
                self.summary
                    .insufficient
                    .push(format!("pass {at}: walk obligation needs the slot count"));
                continue;
            };
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

/// Spec 04's rejection paths over small synthetic streams: each stream is
/// the accepted fixture shape with one defect, and each defect is named by
/// the finding it must produce.
#[test]
fn the_reader_rejects_incomplete_and_contradictory_streams() {
    const P: &str = "sophia_combined_witness schema=1 kind=";
    let at = "iteration=1 cpass=1";
    let good = [
        "fixture_begin owner=child".to_owned(),
        format!("fixture_pass_begin {at} native=present paused=false slots=[0]"),
        format!("images_visit {at} slot=0 hooked=true"),
        format!("binding {at} slot=0 bid=1 ticket=0 serial=1 job=1 grant=(1,1) head=(1,1,0)"),
        format!("collect {at} slot=0 bid=1 result=taken:Rendered state_after=Held"),
        format!("removed {at} slot=0 bid=1 cause=accepted"),
        format!(
            "consume {at} slot=0 cseq=1 bid=1 join=held native=present deadline=ok check=ok classify=match accept=ok decline=none terminal=none entry=accepted"
        ),
        format!("walk_visit {at} slot=0 jobs=0 open=0 cancels=0"),
        format!("fixture_pass_end {at}"),
        "inventory_begin slots=2".to_owned(),
        "retained_row slot=0 jobs=0 open=0 cancels=0".to_owned(),
        "retained_row slot=1 jobs=0 open=0 cancels=0".to_owned(),
        format!("fixture_end {at} slots=2"),
    ];
    let run = |lines: &[String]| {
        let text: Vec<String> = lines.iter().map(|l| format!("{P}{l}")).collect();
        parse(text.iter().map(String::as_str)).and_then(|r| check(&r))
    };
    let summary = run(&good).unwrap_or_else(|f| panic!("{f:?}"));
    assert!(summary.retention_demonstrated && summary.live.is_empty());
    assert_eq!(
        (summary.bindings, summary.consumes, summary.passes.len()),
        (1, 1, 1)
    );
    let with = |edit: &dyn Fn(&mut Vec<String>)| {
        let mut lines = good.to_vec();
        edit(&mut lines);
        run(&lines)
    };
    let expect = |result: Result<Summary, Vec<Finding>>, reject: bool, word: &str| {
        let findings = result.err().unwrap_or_else(|| panic!("{word}: accepted"));
        assert!(
            findings.iter().any(|f| match f {
                Finding::Reject(m) => reject && m.contains(word),
                Finding::Violation(m) => !reject && m.contains(word),
            }),
            "{word}: {findings:?}"
        );
    };
    // Truncations: no terminal, no inventory, a declared row missing.
    expect(
        with(&|l| {
            l.pop();
        }),
        true,
        "incomplete",
    );
    expect(
        with(&|l| {
            l.remove(9);
        }),
        true,
        "inventory_begin",
    );
    expect(
        with(&|l| {
            l.remove(11);
        }),
        true,
        "retained_row",
    );
    expect(
        with(&|l| l[12] = format!("fixture_end {at} slots=1")),
        true,
        "terminal slots",
    );
    // Nonempty inventory: a retained bid that the live set lacks, twice.
    expect(
        with(&|l| {
            l[10] = "retained_row slot=0 jobs=1 open=0 cancels=0".into();
            l.insert(11, "retained slot=0 bid=1 state=Held verdict=Open".into());
        }),
        false,
        "live-set mismatch",
    );
    expect(
        with(&|l| {
            l[10] = "retained_row slot=0 jobs=2 open=0 cancels=0".into();
            l.insert(11, "retained slot=0 bid=1 state=Held verdict=Open".into());
            l.insert(12, "retained slot=0 bid=1 state=Held verdict=Open".into());
        }),
        true,
        "retained twice",
    );
    // MW5's shape: the accepted removal missing leaves bid 1 live.
    expect(
        with(&|l| {
            l.remove(5);
        }),
        false,
        "live-set mismatch",
    );
    // Duplicate bid; exhausted; (serial, job) repeated while live.
    expect(with(&|l| l.insert(4, l[3].clone())), true, "duplicate bid");
    expect(
        with(&|l| l[3] = l[3].replace("bid=1", "bid=exhausted")),
        true,
        "exhausted",
    );
    expect(
        with(&|l| l.insert(4, l[3].replace("bid=1", "bid=2"))),
        true,
        "(serial, job) repeats",
    );
    // Duplicate cancellation identities are preserved and reported, not rejected.
    let dup = with(&|l| {
        l[11] = "retained_row slot=1 jobs=0 open=0 cancels=2".into();
        l.insert(12, "retained_cancel slot=1 head=(1,1,0) grant=(1,1)".into());
        l.insert(13, "retained_cancel slot=1 head=(1,1,0) grant=(1,1)".into());
    })
    .unwrap();
    assert_eq!(dup.retained_cancels, 2);
    assert!(
        dup.notes
            .iter()
            .any(|n| n.contains("duplicate cancellation"))
    );
    // Native-absent exemption: an ordinary entry without the row is no violation there.
    let absent = with(&|l| {
        l[1] = format!("fixture_pass_begin {at} native=absent paused=false slots=[0]");
        l.insert(2, "ordinary_entry cseq=none row=absent".into());
    })
    .unwrap();
    assert!(absent.insufficient.iter().any(|n| n.starts_with("PM1")));
    expect(
        with(&|l| l.insert(2, "ordinary_entry cseq=none row=absent".into())),
        false,
        "content served without the row",
    );
    // Order: the walk before the content (MW2 / PM3).
    expect(
        with(&|l| {
            let walk = l.remove(7);
            l.insert(2, walk);
        }),
        false,
        "walk before content",
    );
    // Omitted hooks: no images visit (PM1), no walk (MW... native-only PM4 shape).
    expect(
        with(&|l| {
            l.remove(2);
        }),
        false,
        "images hook",
    );
    expect(
        with(&|l| {
            l.remove(7);
        }),
        false,
        "walk: slot 0",
    );
    // The consume table and its associated records.
    expect(
        with(&|l| l[6] = l[6].replace("decline=none", "decline=ok")),
        true,
        "decline=ok",
    );
    expect(
        with(&|l| l[6] = l[6].replace("terminal=none", "terminal=ok")),
        true,
        "terminal_requested",
    );
    expect(
        with(&|l| l[6] = l[6].replace("entry=accepted", "entry=ordinary")),
        true,
        "ordinary_entry",
    );
    expect(
        with(&|l| l[6] = l[6].replace("classify=match", "classify=churn")),
        true,
        "return table",
    );
    // MW3's shape: the consume names no bid although the join was held.
    expect(
        with(&|l| l[6] = l[6].replace("bid=1", "bid=none")),
        true,
        "return table",
    );
    expect(
        with(&|l| l[6] = l[6].replace("join=held", "join=ordinary")),
        true,
        "return table",
    );
    // Envelopes: mixed, unpaired, nested; unknown kind; unlabelled exit set.
    expect(
        with(&|l| {
            l.insert(
                1,
                format!("cpass_begin {at} native=present paused=false slots=[0]"),
            )
        }),
        true,
        "mixed",
    );
    expect(
        with(&|l| {
            l.remove(8);
        }),
        true,
        "incomplete",
    );
    expect(with(&|l| l.insert(2, l[1].clone())), true, "inside a pass");
    expect(
        with(&|l| l.insert(2, "unknown_kind a=1".into())),
        true,
        "unknown kind",
    );
    expect(
        with(&|l| l.insert(2, format!("images_visit {at} slot=0 hooked=false"))),
        true,
        "out of set",
    );
    // A production stream: an unlabelled exit is accepted as a record and
    // reported through the pass; an unwinding one is incomplete.
    let production = [
        "capture_begin iteration=1".to_owned(),
        "native_begin iteration=1 site=authority".to_owned(),
        "native_end iteration=1 site=authority result=ok".to_owned(),
        format!("cpass_begin {at} native=present paused=false slots=[]"),
        format!("cpass_end {at} exit=error:root_bounds poll=ok slots_after=[]"),
        "inventory_begin slots=none".to_owned(),
        "capture_end iteration=2 cpass=1 exit=normal slots=none".to_owned(),
    ];
    let p = run(&production).unwrap_or_else(|f| panic!("{f:?}"));
    assert!(p.notes.iter().any(|n| n.contains("ended before the walk")));
    assert!(p.insufficient.len() >= 3, "{:?}", p.insufficient);
    let mut unlabelled = production.to_vec();
    unlabelled[4] = format!("cpass_end {at} exit=error:unlabelled poll=ok slots_after=[]");
    expect(run(&unlabelled), false, "unlabelled exit");
    let mut unwound = production.to_vec();
    unwound[4] = format!("cpass_end {at} exit=unwinding poll=ok slots_after=[]");
    expect(run(&unwound), true, "unwound");
    let mut absent_rows = production.to_vec();
    absent_rows.insert(6, "retained_row slot=0 jobs=0 open=0 cancels=0".into());
    expect(run(&absent_rows), true, "components absent");
}

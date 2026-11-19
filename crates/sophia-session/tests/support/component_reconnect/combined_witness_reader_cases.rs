//! Gate 4 C3b: the reader's control (spec 04 R2, source-01 review R2-R5):
//! small synthetic streams, each the accepted fixture or production shape
//! with one named defect, and the finding it must produce. Compiled only
//! under test, like every file of tests/support.
use super::*;

/// A synthetic stream: record lines without the prefix, edited by kind so
/// that each case names the one defect it introduces.
#[derive(Clone)]
struct Stream(Vec<String>);

impl Stream {
    fn at(&self, kind: &str, nth: usize) -> usize {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, l)| l.split(' ').next() == Some(kind))
            .map(|(i, _)| i)
            .nth(nth)
            .unwrap_or_else(|| panic!("no {kind} #{nth} in {:?}", self.0))
    }

    fn without(mut self, kind: &str, nth: usize) -> Self {
        let i = self.at(kind, nth);
        self.0.remove(i);
        self
    }

    fn edit(mut self, kind: &str, nth: usize, from: &str, to: &str) -> Self {
        let i = self.at(kind, nth);
        assert!(self.0[i].contains(from), "{from} not in {}", self.0[i]);
        self.0[i] = self.0[i].replace(from, to);
        self
    }

    fn after(mut self, kind: &str, nth: usize, line: &str) -> Self {
        let i = self.at(kind, nth);
        self.0.insert(i + 1, line.to_owned());
        self
    }

    fn before(mut self, kind: &str, nth: usize, line: &str) -> Self {
        let i = self.at(kind, nth);
        self.0.insert(i, line.to_owned());
        self
    }

    fn run(&self, expected: usize) -> Result<Summary, Vec<Finding>> {
        let text: Vec<String> = self.0.iter().map(|l| format!("{PREFIX}{l}")).collect();
        parse(text.iter().map(String::as_str)).and_then(|r| check(&r, expected))
    }
}

/// `result` must fail with a finding of the kind containing `word`.
fn expect(case: &str, result: Result<Summary, Vec<Finding>>, reject: bool, word: &str) {
    let findings = result
        .err()
        .unwrap_or_else(|| panic!("{case}: accepted, expected {word:?}"));
    assert!(
        findings.iter().any(|f| match f {
            Finding::Reject(m) => reject && m.contains(word),
            Finding::Violation(m) => !reject && m.contains(word),
        }),
        "{case}: expected {word:?} ({}), got {findings:?}",
        if reject { "reject" } else { "violation" }
    );
}

fn accepted(case: &str, result: Result<Summary, Vec<Finding>>) -> Summary {
    result.unwrap_or_else(|f| panic!("{case}: {f:?}"))
}

/// The reader's rejection, violation and insufficient-evidence paths over
/// small synthetic streams (spec 04 R2 and the source-01 review): each case
/// is the accepted fixture or production shape with one named defect.
#[test]
fn the_reader_rejects_incomplete_and_contradictory_streams() {
    let at = "iteration=1 cpass=1";
    let row = "iteration=1 cpass=1 slot=0";
    let good = Stream(
        [
            "fixture_begin owner=child".to_owned(),
            format!("fixture_pass_begin {at} native=present paused=false slots=[0]"),
            format!("images_visit {row} hooked=true"),
            format!("binding {row} bid=1 ticket=0 serial=1 job=1 grant=(1,1) head=(1,1,0)"),
            format!("collect {row} bid=1 result=taken:Rendered state_after=Held"),
            format!("removed {row} bid=1 cause=accepted"),
            format!("consume {row} cseq=1 bid=1 join=held native=present deadline=ok check=ok classify=match accept=ok decline=none terminal=none entry=accepted"),
            format!("walk_visit {row} jobs=0 open=0 cancels=0"),
            format!("walk_visit {at} slot=1 jobs=0 open=0 cancels=0"),
            format!("fixture_pass_end {at}"),
            "inventory_begin slots=2".to_owned(),
            "retained_row slot=0 jobs=0 open=0 cancels=0".to_owned(),
            "retained_row slot=1 jobs=0 open=0 cancels=0".to_owned(),
            format!("fixture_end {at} slots=2"),
        ]
        .to_vec(),
    );
    // Two registered rows under a capacity of three: the valid stream names
    // and walks exactly its rows; slot 2 is unregistered, not owed.
    const MAX: usize = sophia_config::MAX_SHELL_COMPONENTS;
    const _: () = assert!(2 < MAX, "the two-row cases sit below capacity");
    let s = accepted("good", good.run(2));
    assert_eq!(s.expected_slots, 2);
    assert!(s.fixture && s.retention_demonstrated && s.live.is_empty());
    assert_eq!((s.bindings, s.consumes, s.passes.len()), (1, 1, 1));
    assert_eq!(
        s.insufficient,
        ["PM4: no paused native pass reached its walk"]
    );
    let arm_release = format!(
        "release_attempt {row} bid=1 site=arm cseq=1 outcome=Transferred keep=false state_after=removed"
    );
    // Positive variants of the arm's other rows: a walk-site release, a
    // declined check, a failed join's terminal, a churn's ordinary entry.
    let walked = good
        .clone()
        .without("removed", 0)
        .without("consume", 0)
        .after("walk_visit", 0, &format!("release_attempt {row} bid=1 site=walk_dispose cseq=none outcome=Transferred keep=false state_after=removed"));
    let s = accepted("walk release", walked.run(2));
    assert!(s.live.is_empty() && s.consumes == 0);
    let declined = good
        .clone()
        .without("removed", 0)
        .edit(
            "consume",
            0,
            "check=ok classify=match accept=ok decline=none",
            "check=refused:Runtime classify=none accept=none decline=ok",
        )
        .edit("consume", 0, "entry=accepted", "entry=error")
        .before("consume", 0, &arm_release);
    accepted("declined", declined.run(2));
    let failed = good
        .clone()
        .edit("collect", 0, "taken:Rendered state_after=Held", "taken:RenderFailed state_after=Failed:RenderFailed")
        .edit("removed", 0, "cause=accepted", "cause=consumed_failed")
        .edit("consume", 0, "join=held native=present deadline=ok check=ok classify=match accept=ok decline=none terminal=none entry=accepted", "join=failed native=none deadline=none check=none classify=none accept=none decline=none terminal=ok entry=finished")
        .before("consume", 0, &format!("terminal_requested {row} cseq=1 bid=1"));
    accepted("failed terminal", failed.run(2));
    let churn = good
        .clone()
        .without("removed", 0)
        .edit(
            "consume",
            0,
            "classify=match accept=ok decline=none",
            "classify=churn accept=none decline=ok",
        )
        .edit("consume", 0, "entry=accepted", "entry=ordinary")
        .before("consume", 0, &arm_release)
        .after(
            "consume",
            0,
            &format!("ordinary_entry {row} cseq=1 bid=1 row=lent"),
        );
    accepted("churn ordinary", churn.run(2));
    // Completeness: no terminal, no inventory, a declared row missing, a
    // mismatching terminal count, an inventory record before its phase.
    expect(
        "truncated",
        good.clone().without("fixture_end", 0).run(2),
        true,
        "incomplete",
    );
    expect(
        "no inventory",
        good.clone().without("inventory_begin", 0).run(2),
        true,
        "inventory_begin",
    );
    expect(
        "row missing",
        good.clone().without("retained_row", 1).run(2),
        true,
        "retained_row",
    );
    expect(
        "terminal slots",
        good.clone()
            .edit("fixture_end", 0, "slots=2", "slots=1")
            .run(2),
        true,
        "out of set",
    );
    expect(
        "pinned count",
        good.clone()
            .edit("inventory_begin", 0, "slots=2", "slots=1")
            .run(2),
        true,
        "out of set",
    );
    expect("own count", good.run(3), true, "out of set");
    // The registered-row domain is the caller's: three rows walked and
    // inventoried are accepted at three; a count above capacity is rejected
    // before the grammar even when the stream agrees with it; a declared
    // final row must be present and walked.
    let three = good
        .clone()
        .after(
            "walk_visit",
            1,
            &format!("walk_visit {at} slot=2 jobs=0 open=0 cancels=0"),
        )
        .after(
            "retained_row",
            1,
            "retained_row slot=2 jobs=0 open=0 cancels=0",
        )
        .edit("inventory_begin", 0, "slots=2", "slots=3")
        .edit("fixture_end", 0, "slots=2", "slots=3");
    let s = accepted("three rows", three.run(3));
    assert_eq!((s.expected_slots, s.retained.len()), (3, 0));
    expect(
        "above capacity",
        good.run(MAX + 1),
        true,
        "exceeds MAX_SHELL_COMPONENTS",
    );
    expect(
        "self-declared above capacity",
        three
            .clone()
            .edit(
                "inventory_begin",
                0,
                "slots=3",
                &format!("slots={}", MAX + 1),
            )
            .edit("fixture_end", 0, "slots=3", &format!("slots={}", MAX + 1))
            .run(MAX + 1),
        true,
        "exceeds MAX_SHELL_COMPONENTS",
    );
    expect(
        "last row missing",
        three.clone().without("retained_row", 2).run(3),
        true,
        "2 retained_row headers, 3 declared",
    );
    expect(
        "last row unwalked",
        three.clone().without("walk_visit", 2).run(3),
        false,
        "walk: slot 2",
    );
    expect(
        "early inventory",
        good.clone()
            .before(
                "inventory_begin",
                0,
                "retained_row slot=0 jobs=0 open=0 cancels=0",
            )
            .run(2),
        true,
        "inventory phase",
    );
    expect(
        "fixture count",
        good.clone()
            .edit(
                "fixture_end",
                0,
                "iteration=1 cpass=1",
                "iteration=2 cpass=2",
            )
            .run(2),
        true,
        "pass count",
    );
    // Inventory contents: a retained bid the live set lacks, twice; MW5.
    expect(
        "retained extra",
        good.clone()
            .edit("retained_row", 0, "jobs=0", "jobs=1")
            .after(
                "retained_row",
                0,
                "retained slot=0 bid=1 state=Held verdict=Open",
            )
            .run(2),
        false,
        "live-set mismatch",
    );
    expect(
        "retained twice",
        good.clone()
            .edit("retained_row", 0, "jobs=0", "jobs=2")
            .after(
                "retained_row",
                0,
                "retained slot=0 bid=1 state=Held verdict=Open",
            )
            .after(
                "retained_row",
                0,
                "retained slot=0 bid=1 state=Held verdict=Open",
            )
            .run(2),
        true,
        "retained twice",
    );
    expect(
        "retained other slot",
        good.clone()
            .edit("retained_row", 1, "jobs=0", "jobs=1")
            .after(
                "retained_row",
                1,
                "retained slot=1 bid=1 state=Held verdict=Open",
            )
            .run(2),
        true,
        "another slot",
    );
    expect(
        "MW5",
        good.clone().without("removed", 0).run(2),
        false,
        "live-set mismatch",
    );
    // Identity: duplicate bid, exhausted, (serial, job) repeated while live,
    // a row record from another slot, an unbound bid.
    let binding2 = format!("binding {row} bid=2 ticket=1 serial=1 job=1 grant=(1,1) head=(1,1,0)");
    expect(
        "duplicate bid",
        good.clone()
            .after("binding", 0, &good.0[good.at("binding", 0)])
            .run(2),
        true,
        "duplicate bid",
    );
    expect(
        "exhausted",
        good.clone()
            .edit("binding", 0, "bid=1", "bid=exhausted")
            .run(2),
        true,
        "exhausted",
    );
    expect(
        "serial job repeat",
        good.clone().after("binding", 0, &binding2).run(2),
        true,
        "(serial, job) repeats",
    );
    expect(
        "other slot",
        good.clone().edit("collect", 0, "slot=0", "slot=1").run(2),
        true,
        "another slot",
    );
    expect(
        "unbound",
        good.clone().edit("consume", 0, "bid=1", "bid=2").run(2),
        true,
        "never bound",
    );
    // Cancellation identities are preserved and reported, not rejected.
    let dup = good
        .clone()
        .edit("retained_row", 1, "cancels=0", "cancels=2")
        .after(
            "retained_row",
            1,
            "retained_cancel slot=1 head=(1,1,0) grant=(1,1)",
        )
        .after(
            "retained_row",
            1,
            "retained_cancel slot=1 head=(1,1,0) grant=(1,1)",
        );
    let s = accepted("duplicate cancels", dup.run(2));
    assert_eq!(s.retained_cancels, 2);
    assert!(s.notes.iter().any(|n| n.contains("duplicate cancellation")));
    // Omitted content: a violation with native present, nothing without.
    let absent_entry = "ordinary_entry cseq=none row=absent";
    let s = accepted(
        "native absent",
        good.clone()
            .edit("fixture_pass_begin", 0, "native=present", "native=absent")
            .after("images_visit", 0, absent_entry)
            .run(2),
    );
    assert!(s.insufficient.iter().any(|n| n.starts_with("PM1")));
    expect(
        "row absent",
        good.clone().after("images_visit", 0, absent_entry).run(2),
        false,
        "content served without the row",
    );
    // Hook order and omissions: MW2/PM3, PM1, the walk.
    let walk0 = good.0[good.at("walk_visit", 0)].clone();
    expect(
        "walk before content",
        good.clone()
            .without("walk_visit", 0)
            .before("images_visit", 0, &walk0)
            .run(2),
        false,
        "walk before content",
    );
    expect(
        "images omitted",
        good.clone().without("images_visit", 0).run(2),
        false,
        "images hook",
    );
    expect(
        "walk omitted",
        good.clone().without("walk_visit", 1).run(2),
        false,
        "walk: slot 1",
    );
    expect(
        "hooked false",
        good.clone()
            .edit("images_visit", 0, "hooked=true", "hooked=false")
            .run(2),
        true,
        "out of set",
    );
    expect(
        "head arity",
        good.clone()
            .edit("binding", 0, "head=(1,1,0)", "head=(1,bad,0)")
            .run(2),
        true,
        "out of set",
    );
    expect(
        "slot beyond",
        good.clone()
            .edit("walk_visit", 1, "slot=1", "slot=2")
            .run(2),
        true,
        "out of set",
    );
    // The consume table and its cseq-bound records, in exact numbers.
    expect(
        "decline without release",
        good.clone()
            .edit("consume", 0, "decline=none", "decline=ok")
            .run(2),
        true,
        "exactly 1 arm",
    );
    expect(
        "terminal without request",
        good.clone()
            .edit("consume", 0, "terminal=none", "terminal=ok")
            .run(2),
        true,
        "exactly 1 terminal",
    );
    expect(
        "ordinary without entry",
        good.clone()
            .edit("consume", 0, "entry=accepted", "entry=ordinary")
            .run(2),
        true,
        "exactly 1 ordinary_entry",
    );
    let lent = format!("ordinary_entry {row} cseq=1 bid=1 row=lent");
    expect(
        "entry forbidden",
        good.clone().after("consume", 0, &lent).run(2),
        true,
        "exactly 0 ordinary_entry",
    );
    expect(
        "two entries",
        good.clone()
            .after("consume", 0, &lent)
            .after("consume", 0, &lent)
            .run(2),
        true,
        "exactly 0 ordinary_entry",
    );
    expect(
        "two entries ordinary",
        churn.clone().after("consume", 0, &lent).run(2),
        true,
        "exactly 1 ordinary_entry",
    );
    expect(
        "release forbidden",
        good.clone().before("consume", 0, &arm_release).run(2),
        true,
        "exactly 0 arm",
    );
    expect(
        "two releases",
        declined.clone().before("consume", 0, &arm_release).run(2),
        true,
        "exactly 1 arm",
    );
    expect(
        "two terminals",
        failed
            .clone()
            .before(
                "consume",
                0,
                &format!("terminal_requested {row} cseq=1 bid=1"),
            )
            .run(2),
        true,
        "exactly 1 terminal",
    );
    expect(
        "release other slot",
        declined
            .clone()
            .edit("release_attempt", 0, "slot=0", "slot=1")
            .run(2),
        true,
        "another",
    );
    expect(
        "terminal other pass",
        failed
            .clone()
            .edit("terminal_requested", 0, "cpass=1", "cpass=2")
            .run(2),
        true,
        "another",
    );
    expect(
        "entry other bid",
        churn
            .clone()
            .edit("ordinary_entry", 0, "bid=1", "bid=none")
            .run(2),
        true,
        "another pass, slot or bid",
    );
    expect(
        "entry before consume",
        churn
            .clone()
            .without("ordinary_entry", 0)
            .before("consume", 0, &lent)
            .run(2),
        true,
        "before its consume",
    );
    expect(
        "terminal before release",
        declined
            .clone()
            .edit(
                "consume",
                0,
                "check=refused:Runtime",
                "check=refused:Native",
            )
            .edit(
                "consume",
                0,
                "terminal=none entry=error",
                "terminal=ok entry=finished",
            )
            .before(
                "release_attempt",
                0,
                &format!("terminal_requested {row} cseq=1 bid=1"),
            )
            .run(2),
        true,
        "precedes the arm release",
    );
    expect(
        "walk cseq",
        walked
            .clone()
            .edit("release_attempt", 0, "cseq=none", "cseq=1")
            .run(2),
        true,
        "cseq is numeric at the arm",
    );
    expect(
        "arm cseq none",
        declined
            .clone()
            .edit("release_attempt", 0, "cseq=1", "cseq=none")
            .run(2),
        true,
        "cseq is numeric at the arm",
    );
    expect(
        "keep disagrees",
        walked
            .clone()
            .edit("release_attempt", 0, "keep=false", "keep=true")
            .run(2),
        true,
        "keep and state_after",
    );
    expect(
        "churn accept",
        good.clone()
            .edit("consume", 0, "classify=match", "classify=churn")
            .run(2),
        true,
        "return table",
    );
    expect(
        "MW3 bid none",
        good.clone().edit("consume", 0, "bid=1", "bid=none").run(2),
        true,
        "return table",
    );
    expect(
        "ordinary join",
        good.clone()
            .edit("consume", 0, "join=held", "join=ordinary")
            .run(2),
        true,
        "return table",
    );
    expect(
        "stale cseq",
        good.clone()
            .before(
                "consume",
                0,
                &format!("terminal_requested {row} cseq=7 bid=1"),
            )
            .run(2),
        true,
        "names no consume",
    );
    // Envelopes and passes: mixed, unpaired, nested, unknown, out of set.
    expect(
        "mixed",
        good.clone()
            .after(
                "fixture_begin",
                0,
                &format!("cpass_begin {at} native=present paused=false slots=[0]"),
            )
            .run(2),
        true,
        "mixed",
    );
    expect(
        "pass unpaired",
        good.clone().without("fixture_pass_end", 0).run(2),
        true,
        "incomplete",
    );
    expect(
        "pass nested",
        good.clone().after("images_visit", 0, &good.0[1]).run(2),
        true,
        "inside a pass",
    );
    expect(
        "unknown kind",
        good.clone()
            .after("images_visit", 0, "unknown_kind a=1")
            .run(2),
        true,
        "unknown kind",
    );
    expect(
        "second begin",
        good.clone()
            .after("images_visit", 0, "fixture_begin owner=child")
            .run(2),
        true,
        "boundary",
    );
    // Production envelopes: components absent, then components present with
    // a walked pass, a native-absent pass and a pass that ended early.
    let absent = Stream(
        [
            "capture_begin iteration=1",
            "native_begin iteration=1 site=authority",
            "native_end iteration=1 site=authority result=ok",
            "inventory_begin slots=none",
            "capture_end iteration=1 cpass=none exit=normal slots=none",
        ]
        .iter()
        .map(|l| (*l).to_owned())
        .collect(),
    );
    let s = accepted("components absent", absent.run(2));
    assert!(s.passes.is_empty() && s.retention_demonstrated && s.insufficient.len() == 3);
    expect(
        "absent with rows",
        absent
            .clone()
            .after(
                "inventory_begin",
                0,
                "retained_row slot=0 jobs=0 open=0 cancels=0",
            )
            .run(2),
        true,
        "components absent",
    );
    expect(
        "absent cpass",
        absent
            .clone()
            .edit("capture_end", 0, "cpass=none", "cpass=0")
            .run(2),
        true,
        "imply each other",
    );
    // R8: an empty envelope orders itself; a malformed maximum counter is a
    // structured rejection, never an overflow.
    let empty = Stream(
        [
            "capture_begin iteration=1",
            "inventory_begin slots=none",
            "capture_end iteration=1 cpass=none exit=normal slots=none",
        ]
        .iter()
        .map(|l| (*l).to_owned())
        .collect(),
    );
    let s = accepted("empty envelope", empty.run(2));
    assert!(s.retention_demonstrated && s.passes.is_empty());
    expect(
        "empty backwards",
        empty
            .clone()
            .edit("capture_begin", 0, "iteration=1", "iteration=2")
            .run(2),
        true,
        "before the capture's begin",
    );
    expect(
        "absent with pass",
        absent
            .clone()
            .after(
                "native_end",
                0,
                &format!("cpass_end {at} exit=normal poll=ok slots_after=[]"),
            )
            .before(
                "cpass_end",
                0,
                &format!("cpass_begin {at} native=present paused=false slots=[]"),
            )
            .run(2),
        true,
        "component passes were recorded",
    );
    let production = Stream(
        [
            "capture_begin iteration=1".to_owned(),
            format!("cpass_begin {at} native=present paused=false slots=[0]"),
            format!("images_visit {row} hooked=true"),
            format!("walk_visit {row} jobs=0 open=0 cancels=0"),
            format!("walk_visit {at} slot=1 jobs=0 open=0 cancels=0"),
            format!("cpass_end {at} exit=normal poll=ok slots_after=[0]"),
            "native_begin iteration=1 site=lifecycle".to_owned(),
            "native_end iteration=1 site=lifecycle result=ok".to_owned(),
            "cpass_begin iteration=2 cpass=2 native=absent paused=false slots=[0]".to_owned(),
            "ordinary_entry cseq=none row=absent".to_owned(),
            "cpass_end iteration=2 cpass=2 exit=normal poll=ok slots_after=[0]".to_owned(),
            "cpass_begin iteration=3 cpass=3 native=present paused=false slots=[0]".to_owned(),
            "images_visit iteration=3 cpass=3 slot=0 hooked=true".to_owned(),
            "cpass_end iteration=3 cpass=3 exit=error:root_bounds poll=ok slots_after=[0]"
                .to_owned(),
            "inventory_begin slots=2".to_owned(),
            "retained_row slot=0 jobs=0 open=0 cancels=0".to_owned(),
            "retained_row slot=1 jobs=0 open=0 cancels=0".to_owned(),
            "capture_end iteration=3 cpass=3 exit=normal slots=2".to_owned(),
        ]
        .to_vec(),
    );
    let s = accepted("production", production.run(2));
    expect(
        "production own count",
        production.run(3),
        true,
        "out of set",
    );
    expect(
        "native none pass",
        production
            .clone()
            .edit("cpass_begin", 0, "native=present", "native=none")
            .without("images_visit", 0)
            .without("walk_visit", 0)
            .without("walk_visit", 0)
            .run(2),
        true,
        "out of set",
    );
    expect(
        "native none fixture pass",
        good.clone()
            .edit("fixture_pass_begin", 0, "native=present", "native=none")
            .run(2),
        true,
        "out of set",
    );
    let max = "18446744073709551615";
    expect(
        "counter maximum",
        production
            .clone()
            .edit("cpass_begin", 0, "cpass=1", &format!("cpass={max}"))
            .edit("cpass_end", 0, "cpass=1", &format!("cpass={max}"))
            .run(2),
        true,
        "advance by one",
    );
    assert_eq!(s.passes.len(), 3);
    assert!(s.notes.iter().any(|n| n.contains("ended before the walk")));
    assert!(
        s.insufficient.iter().all(|n| !n.starts_with("PM1")),
        "{:?}",
        s.insufficient
    );
    expect(
        "cpass gap",
        production
            .clone()
            .edit("cpass_begin", 2, "cpass=3", "cpass=4")
            .edit("cpass_end", 2, "cpass=3", "cpass=4")
            .edit("capture_end", 0, "cpass=3", "cpass=4")
            .run(2),
        true,
        "advance by one",
    );
    expect(
        "terminal cpass",
        production
            .clone()
            .edit("capture_end", 0, "cpass=3", "cpass=2")
            .run(2),
        true,
        "number of passes",
    );
    expect(
        "begin iteration",
        production
            .clone()
            .edit("capture_begin", 0, "iteration=1", "iteration=2")
            .run(2),
        true,
        "capture_begin iteration",
    );
    expect(
        "end iteration",
        production
            .clone()
            .edit("capture_end", 0, "iteration=3", "iteration=2")
            .run(2),
        true,
        "terminal iteration",
    );
    expect(
        "iteration backwards",
        production
            .clone()
            .edit("native_begin", 0, "iteration=1", "iteration=0")
            .run(2),
        true,
        "backwards",
    );
    expect(
        "native unpaired",
        production.clone().without("native_end", 0).run(2),
        true,
        "incomplete",
    );
    expect(
        "unlabelled",
        production
            .clone()
            .edit("cpass_end", 2, "error:root_bounds", "error:unlabelled")
            .run(2),
        true,
        "unlabelled",
    );
    expect(
        "unwinding",
        production
            .clone()
            .edit("cpass_end", 2, "error:root_bounds", "unwinding")
            .run(2),
        true,
        "unwound",
    );
    // A failed poll voids the images obligation of its pass, not the walk's.
    let s = accepted(
        "poll failed",
        production
            .clone()
            .edit("cpass_end", 0, "poll=ok", "poll=failed")
            .without("images_visit", 0)
            .run(2),
    );
    assert!(s.notes.iter().any(|n| n.contains("images unobserved")));
    // The paused walk: all slots walked, no content served.
    let paused = production
        .clone()
        .edit("cpass_begin", 0, "paused=false", "paused=true")
        .edit("cpass_end", 0, "exit=normal", "exit=paused");
    let s = accepted("paused", paused.run(2));
    assert!(s.insufficient.iter().all(|n| !n.starts_with("PM4")));
    expect(
        "PM4",
        paused.clone().without("walk_visit", 1).run(2),
        false,
        "walk: slot 1",
    );
    expect(
        "paused content",
        paused
            .clone()
            .after(
                "images_visit",
                0,
                &format!("binding {row} bid=1 ticket=0 serial=1 job=1 grant=(1,1) head=(1,1,0)"),
            )
            .after("binding", 0, &format!("removed {row} bid=1 cause=accepted"))
            .after("removed", 0, &good.0[good.at("consume", 0)])
            .run(2),
        false,
        "paused pass",
    );
    // A Held result that passed content unconsumed is insufficient, not a verdict.
    let s = accepted(
        "held unconsumed",
        good.clone()
            .without("removed", 0)
            .without("consume", 0)
            .edit("retained_row", 0, "jobs=0", "jobs=1")
            .after(
                "retained_row",
                0,
                "retained slot=0 bid=1 state=Held verdict=Open",
            )
            .run(2),
    );
    assert!(
        s.insufficient
            .iter()
            .any(|n| n.contains("held result not consumed"))
    );
    assert_eq!(s.retained.len(), 1);
}

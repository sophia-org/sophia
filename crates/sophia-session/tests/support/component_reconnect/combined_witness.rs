//! Gate 4 C3b: the diagnostic witness's isolated control (spec 03 section 5,
//! spec 04 R2). The child repeats the running-core accepted-result sequence
//! of `an_unchanged_scene_is_accepted_and_submitted_in_the_result_turn`
//! with that control's assertions, alone in its process, where it is the
//! only installer of `SessionOutput`; the parent spawns it, reads its
//! stdout and checks the witness stream with the one reader. The fixture
//! pass identity is the Lane's turn count; it proves nothing about the
//! production owner pass.
use super::*;
use crate::shell_component_connections::combined_row::witness;

#[path = "combined_witness_reader.rs"]
mod reader;

const CHILD_ENV: &str = "SOPHIA_WITNESS_CHILD";
const CAPTURE_ENV: &str = "SOPHIA_WITNESS_CAPTURE";
const EXPECTED_ENV: &str = "SOPHIA_WITNESS_EXPECTED_SLOTS";

/// The registered connection rows of the child's fixture, pinned here and
/// nowhere in the stream: `Lane::running()` builds `Harness::new()`, which is
/// `with_dock(false)`, registering the Bar and ApplicationLauncher rows and
/// leaving the Dock row unregistered. `MAX_SHELL_COMPONENTS` is the capacity
/// bound, not this count; the reader obligates exactly these rows.
const FIXTURE_ROWS: usize = 2;
const _: () = assert!(FIXTURE_ROWS <= sophia_config::MAX_SHELL_COMPONENTS);

/// The child's libtest name: this module's path without the crate.
fn child_name() -> String {
    let module = module_path!();
    let module = module.split_once("::").map_or(module, |(_, rest)| rest);
    format!("{module}::witness_child")
}

/// What the child observed through the existing recorder, for the parent.
struct Expectation {
    serial: u64,
    job: u64,
    ticket: u64,
    grant: String,
    head: String,
    passes: u64,
}

fn expectation(line: &str) -> Expectation {
    let field = |key: &str| {
        line.split(' ')
            .find_map(|t| t.strip_prefix(key).and_then(|t| t.strip_prefix('=')))
            .unwrap_or_else(|| panic!("witness_expect lacks {key}: {line}"))
    };
    Expectation {
        serial: field("serial").parse().unwrap(),
        job: field("job").parse().unwrap(),
        ticket: field("ticket").parse().unwrap(),
        grant: field("grant").to_owned(),
        head: field("head").to_owned(),
        passes: field("passes").parse().unwrap(),
    }
}

#[test]
fn the_witness_stream_of_one_accepted_admission_is_consistent() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &child_name(), "--ignored", "--nocapture"])
        .env(CHILD_ENV, "1")
        .stderr(std::process::Stdio::inherit())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&child.stdout);
    assert!(
        child.status.success(),
        "the child control failed ({:?}):\n{stdout}",
        child.status
    );
    let expect = stdout
        .lines()
        .find(|l| l.starts_with("witness_expect "))
        .map(expectation)
        .expect("the child printed its expectation");
    let records = reader::parse(stdout.lines()).unwrap_or_else(|f| panic!("{f:?}"));
    let of = |kind: &str| -> Vec<&reader::Record> {
        records.iter().filter(|r| r.kind == kind).collect()
    };
    let passes = of("fixture_pass_begin");
    assert_eq!(
        passes.len() as u64,
        expect.passes,
        "one pass per driver turn"
    );
    assert!(
        passes.iter().all(|p| p.get("slots") == "[0]"),
        "slot 0 is expected in every pass"
    );
    // 5.4: exactly one binding, the recorder's identity.
    let bindings = of("binding");
    assert_eq!(bindings.len(), 1, "exactly one binding");
    let b = bindings[0];
    assert_eq!(
        (b.get("slot"), b.get("bid"), b.get("ticket")),
        ("0", "1", expect.ticket.to_string().as_str())
    );
    assert_eq!(
        (b.get("serial"), b.get("job"), b.get("grant"), b.get("head")),
        (
            expect.serial.to_string().as_str(),
            expect.job.to_string().as_str(),
            expect.grant.as_str(),
            expect.head.as_str()
        ),
        "the binding names this validator, this job, this grant and this head"
    );
    // One hooked images visit per pass; the result collected in the last.
    let images = of("images_visit");
    assert_eq!(
        images.len() as u64,
        expect.passes,
        "one images visit per pass"
    );
    assert!(
        images
            .iter()
            .all(|r| r.get("slot") == "0" && r.get("hooked") == "true")
    );
    let collects = of("collect");
    let taken: Vec<&&reader::Record> = collects
        .iter()
        .filter(|r| r.get("result") == "taken:Rendered")
        .collect();
    assert_eq!(taken.len(), 1, "one fresh result: {collects:?}");
    assert_eq!(
        (
            taken[0].get("bid"),
            taken[0].get("state_after"),
            taken[0].get("cpass")
        ),
        ("1", "Held", expect.passes.to_string().as_str())
    );
    let bound_at: u64 = b.get("cpass").parse().unwrap();
    for pass in bound_at + 1..expect.passes {
        let uncollected = collects
            .iter()
            .filter(|r| r.get("cpass") == pass.to_string() && r.get("result") == "none")
            .count();
        assert_eq!(
            uncollected, 1,
            "pass {pass} visits the uncollected job once"
        );
    }
    // Exactly one consume, the accepted row of the return table.
    let consumes = of("consume");
    assert_eq!(consumes.len(), 1, "exactly one consume");
    let c = consumes[0];
    assert_eq!(
        c.fields,
        [
            ("iteration", expect.passes.to_string()),
            ("cpass", expect.passes.to_string()),
            ("slot", "0".into()),
            ("cseq", "1".into()),
            ("bid", "1".into()),
            ("join", "held".into()),
            ("native", "present".into()),
            ("deadline", "ok".into()),
            ("check", "ok".into()),
            ("classify", "match".into()),
            ("accept", "ok".into()),
            ("decline", "none".into()),
            ("terminal", "none".into()),
            ("entry", "accepted".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect::<std::collections::BTreeMap<String, String>>(),
        "the consume record is the accepted row"
    );
    let removed = of("removed");
    assert_eq!(removed.len(), 1);
    assert_eq!(
        (removed[0].get("bid"), removed[0].get("cause")),
        ("1", "accepted")
    );
    for kind in [
        "release_attempt",
        "terminal_requested",
        "ordinary_entry",
        "reserve_cancelled",
        "cancel_retry",
    ] {
        assert!(of(kind).is_empty(), "no {kind} record: {:?}", of(kind));
    }
    // MD4 in the stream: the last pass walks slot 0 after the consume.
    let walk = of("walk_visit")
        .into_iter()
        .find(|r| r.get("slot") == "0" && r.get("cpass") == expect.passes.to_string())
        .expect("the last pass walks slot 0");
    assert!(walk.index > c.index, "the walk follows the content step");
    // The reader's contract over the complete fixture stream, obligated to
    // the independently pinned registered rows, never to a count it read.
    let summary = reader::check(&records, FIXTURE_ROWS).unwrap_or_else(|f| panic!("{f:?}"));
    assert!(summary.fixture);
    assert_eq!(summary.passes.len() as u64, expect.passes);
    assert!(
        summary.live.is_empty(),
        "accepted removes the entry: {:?}",
        summary.live
    );
    assert!(
        summary.retained.is_empty() && summary.retained_open == 0,
        "{:?}",
        summary.retained
    );
    assert!(summary.retention_demonstrated);
}

/// The child: the running-core accepted-result sequence, witnessed.
#[test]
#[ignore = "witness child entry, invoked only by the parent test"]
fn witness_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        // Run outside its parent (an --include-ignored sweep): installing
        // the output here would not be isolated, so nothing is observed.
        return;
    }
    crate::output::install(crate::output::SessionOutput::new(
        |line| println!("{line}"),
        |line| eprintln!("{line}"),
    ))
    .unwrap();
    witness::fixture_begin();
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let serial = lane.validator().validator_serial();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.admitted(&mut a);
    let bindings = lane.row().bindings().to_vec();
    assert_eq!(bindings.len(), 1, "one admission, bound at submitted");
    assert_eq!(
        (bindings[0].serial, bindings[0].job, bindings[0].grant),
        (serial, job, a.key.grant),
        "the binding names this validator, this job and this grant"
    );
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the render result rings the owner"
    );
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "T0 left it uncollected"
    );
    let t1 = lane.result_turn();
    assert_eq!(
        t1.steps, TURN,
        "MD4: content accepts before the walk, native last"
    );
    assert_eq!(
        t1.status,
        Some(Submit::SubmittedWaitingForPageFlip),
        "MD3: after T1 the accepted frame is Submitted"
    );
    assert_eq!(t1.accepted, [1], "T1's content step accepted the result");
    assert_eq!(
        lane.native.device_counts(output().id),
        Some((1, 1, 0)),
        "one framebuffer, one commit, no test commit"
    );
    assert_eq!(lane.native.accepts(), 1);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    assert!(lane.states().is_empty(), "accepted removes the entry");
    lane.trace_end(&[], &[]);
    assert_eq!(
        lane.pending(&a),
        [(a.key.grant, OUTPUT, 1)],
        "one pending presentation"
    );
    assert!(
        lane.native.head_observation(output().id).2,
        "the head keeps the accepted content as submitted"
    );
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)]
    );
    assert_eq!(lane.row().bindings(), bindings, "no later binding");
    // The complete inventory, then the terminal record (spec 04 R2). The
    // inventory enumerates the registered rows; it must agree with the pin,
    // but the pin, not this return value, is what the parent checks against.
    let slots = lane.h.owner.processes.witness_inventory();
    assert_eq!(
        slots, FIXTURE_ROWS,
        "the inventory covers the registered rows"
    );
    witness::fixture_end(lane.witness_pass(), slots);
    let (head_owner, head, head_ticket) = bindings[0].head;
    println!(
        "witness_expect serial={serial} job={job} ticket={} grant=({},{}) head=({head_owner},{head},{head_ticket}) slot=0 passes={}",
        bindings[0].ticket,
        a.key.grant.connection_epoch,
        a.key.grant.content_grant_epoch,
        lane.witness_passes()
    );
}

/// The reader over a sealed capture, for a later native harness. That
/// harness must freeze `SOPHIA_WITNESS_EXPECTED_SLOTS` from the registered
/// component rows of the captured session (stable over the capture
/// interval) and bind the value to the capture receipt; the entry infers
/// nothing from the capture, and a successful read with an unverified value
/// claims no completeness. Missing, malformed or over-capacity input fails.
#[test]
#[ignore = "reader entry for sealed captures, invoked by a harness"]
fn witness_reader_over_file() {
    let path = std::env::var_os(CAPTURE_ENV).expect("SOPHIA_WITNESS_CAPTURE names the capture");
    let expected: usize = std::env::var(EXPECTED_ENV)
        .expect("SOPHIA_WITNESS_EXPECTED_SLOTS pins the registered rows")
        .parse()
        .expect("SOPHIA_WITNESS_EXPECTED_SLOTS is a count");
    let text = std::fs::read_to_string(&path).unwrap();
    let records = reader::parse(text.lines()).unwrap_or_else(|f| panic!("{f:?}"));
    match reader::check(&records, expected) {
        Ok(summary) => println!("{summary:#?}"),
        Err(findings) => panic!("{findings:#?}"),
    }
}

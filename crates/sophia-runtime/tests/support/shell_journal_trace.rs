//! Joins the comparator journal trace: commits, role identities, read
//! deliveries and acknowledgements, per journal. Diagnostic only.
//!
//! Classification follows the records' semantics. An outcome answers a
//! submission with its key: exactly, when both carry a transaction and they
//! match; by key alone only when no transaction is carried and one
//! submission holds the key. Anything else is listed unmatched with its
//! candidates, never joined. Commands without a key have no answer to join,
//! and a keyed command may legitimately get none. Keys are per journal; a
//! candidate generation key does not include its output, which suits the
//! single-output fixtures that use it. A record is
//! delivered when a read contiguous with every earlier delivered byte first
//! ends at or past its last byte, as the read itself reports; a read that
//! skips bytes credits nothing and is a violation. A read below the
//! delivered high-water offset is an explicit reread, not a delivery. Turns
//! are the driver's marks: an event belongs to the last mark before it.
//!
//! Read pairing: a joined outcome is `joined_read` when the read that
//! delivered its submission's receipt also delivered it, `split_read` when a
//! different read did, and `not_delivered` when no read did. Each receipt
//! and outcome carries its delivering read and its first covering
//! acknowledgement. The raw commit, read and acknowledgement tables are kept
//! so every derived join can be audited.
use sophia_9p::journal_trace::{AckResult, Event, ReadResult, RolePhase};
use std::collections::BTreeMap;

#[derive(Default)]
struct Journal {
    epoch: u64,
    /// sequence -> (commit ordinal, commit turn)
    records: BTreeMap<u64, (usize, u64)>,
    /// sequence -> appended record kind
    kinds: BTreeMap<u64, u16>,
    /// sequence -> (delivery ordinal, delivery turn, parked reads before it)
    delivered: BTreeMap<u64, (usize, u64, u64)>,
    high_water: u64,
    delivered_through: u64,
    parked: u64,
    reads: [u64; 6],
    acks: [u64; 4],
    largest_ack_lag: u64,
    /// Advanced acknowledgements in order: (ordinal, turn, sequence)
    advanced: Vec<(usize, u64, u64)>,
    /// records a read completed -> reads
    completed_per_read: BTreeMap<u64, u64>,
}

impl Journal {
    /// The read that delivered `sequence`, and the first acknowledgement
    /// covering it.
    fn custody(&self, sequence: u64) -> (serde_json::Value, serde_json::Value) {
        let delivery = self.delivered.get(&sequence).map_or(
            serde_json::Value::Null,
            |(ordinal, turn, _)| serde_json::json!({"read": ordinal, "turn": turn}),
        );
        let first = self.advanced.partition_point(|a| a.2 < sequence);
        let ack = self
            .advanced
            .get(first)
            .map_or(serde_json::Value::Null, |(ordinal, turn, requested)| {
                serde_json::json!({"ordinal": ordinal, "turn": turn, "sequence": requested})
            });
        (delivery, ack)
    }
}

const COMMIT_COLUMNS: [&str; 7] = [
    "ordinal", "turn", "journal", "first", "last", "start", "end",
];
const READ_COLUMNS: [&str; 13] = [
    "ordinal",
    "turn",
    "journal",
    "offset",
    "requested",
    "delivered",
    "first",
    "last",
    "starts_inside",
    "ends_inside",
    "result",
    "parked",
    "completed",
];
const ACK_COLUMNS: [&str; 9] = [
    "ordinal",
    "turn",
    "journal",
    "epoch",
    "requested",
    "result",
    "acknowledged",
    "floor",
    "last_committed",
];

const READS: [&str; 6] = [
    "advance",
    "continuation",
    "reread",
    "skip",
    "stale",
    "invalid",
];

const ACKS: [&str; 4] = ["advanced", "repeat", "stale", "invalid"];

fn turn_class(from: u64, to: u64) -> &'static str {
    if from == to {
        "same_turn"
    } else {
        "later_turn"
    }
}

/// The joined summary. `violations` lists broken invariants; it is empty
/// for any trace the journal semantics allow.
pub fn analyze(events: &[Event], dropped: u64) -> serde_json::Value {
    let mut journals = BTreeMap::<u64, Journal>::new();
    let mut violations = Vec::<String>::new();
    let mut submissions = Vec::<serde_json::Value>::new();
    // (journal, key class, id, generation) -> submissions holding it, in order
    let mut open_keys = BTreeMap::<(u64, &str, u64, u64), Vec<usize>>::new();
    // (journal, receipt sequence) -> index of the submission
    let mut receipts = BTreeMap::<(u64, u64), usize>::new();
    let mut unmatched = Vec::<serde_json::Value>::new();
    let mut turn = 0;
    let mut last_ack = BTreeMap::<u64, (u64, AckResult)>::new();
    let (mut commit_table, mut read_table, mut ack_table) = (Vec::new(), Vec::new(), Vec::new());
    for (ordinal, event) in events.iter().enumerate() {
        match *event {
            Event::Mark { label } => turn = label,
            Event::Opened {
                journal,
                epoch,
                next_sequence,
                tail,
            } => {
                let j = journals.entry(journal).or_default();
                j.epoch = epoch;
                j.high_water = tail;
                j.delivered_through = next_sequence.saturating_sub(1);
            }
            Event::Commit {
                journal,
                first,
                last,
                start,
                end,
            } => {
                commit_table.push(serde_json::json!([
                    ordinal, turn, journal, first, last, start, end
                ]));
                let j = journals.entry(journal).or_default();
                for sequence in first..=last {
                    j.records.insert(sequence, (ordinal, turn));
                }
            }
            Event::Read {
                journal,
                offset,
                requested,
                delivered,
                first,
                last,
                starts_inside,
                ends_inside,
                result,
                parked,
            } => {
                read_table.push(serde_json::json!([
                    ordinal,
                    turn,
                    journal,
                    offset,
                    requested,
                    delivered,
                    first,
                    last,
                    starts_inside,
                    ends_inside,
                    format!("{result:?}"),
                    parked,
                    0
                ]));
                let j = journals.entry(journal).or_default();
                j.parked += u64::from(parked);
                let class = match result {
                    ReadResult::Stale => 4,
                    ReadResult::Invalid => 5,
                    ReadResult::Ready if offset < j.high_water => 2,
                    ReadResult::Ready if offset > j.high_water => 3,
                    ReadResult::Ready if starts_inside => 1,
                    ReadResult::Ready => 0,
                };
                j.reads[class] += 1;
                if result != ReadResult::Ready {
                    continue;
                }
                // Bytes past the high-water offset were never delivered, so
                // a read beginning there credits nothing.
                if class == 3 {
                    violations.push(format!(
                        "journal {journal} read at {offset} skips from {}",
                        j.high_water
                    ));
                    continue;
                }
                j.high_water = j.high_water.max(offset + u64::from(delivered));
                let whole = if ends_inside {
                    last.saturating_sub(1)
                } else {
                    last
                };
                let mut completed = 0;
                for sequence in j.delivered_through + 1..=whole {
                    j.delivered.insert(sequence, (ordinal, turn, j.parked));
                    j.parked = 0;
                    completed += 1;
                }
                j.delivered_through = j.delivered_through.max(whole);
                *j.completed_per_read.entry(completed).or_default() += 1;
                read_table.last_mut().unwrap()[12] = completed.into();
            }
            Event::Ack {
                journal,
                epoch,
                requested,
                result,
                acknowledged,
                floor,
                last_committed,
            } => {
                ack_table.push(serde_json::json!([
                    ordinal,
                    turn,
                    journal,
                    epoch,
                    requested,
                    format!("{result:?}"),
                    acknowledged,
                    floor,
                    last_committed
                ]));
                let j = journals.entry(journal).or_default();
                j.acks[result as usize] += 1;
                if result == AckResult::Advanced {
                    j.advanced.push((ordinal, turn, requested));
                    if requested > j.delivered_through {
                        violations.push(format!(
                            "journal {journal} ack {requested} passes delivered {}",
                            j.delivered_through
                        ));
                    }
                    j.largest_ack_lag = j.largest_ack_lag.max(last_committed - requested);
                }
                last_ack.insert(journal, (requested, result));
            }
            Event::Role(role) => {
                let journal = role.journal;
                let key = (journal, role.key.class, role.key.id, role.key.generation);
                match role.phase {
                    RolePhase::Appended => {
                        journals
                            .entry(journal)
                            .or_default()
                            .kinds
                            .insert(role.sequence, role.kind);
                    }
                    RolePhase::Submitted => {
                        let index = submissions.len();
                        submissions.push(serde_json::json!({
                            "journal": journal, "sequence": role.sequence,
                            "submission_id": role.related, "kind": role.kind,
                            "transaction": role.transaction,
                            "key": if role.key.class.is_empty() { serde_json::Value::Null } else {
                                serde_json::json!({"class": role.key.class, "id": role.key.id,
                                    "generation": role.key.generation}) },
                            "ordinal": ordinal, "turn": turn, "outcomes": [],
                            "released": null, "superseded_by": null,
                        }));
                        if !role.key.class.is_empty() {
                            open_keys.entry(key).or_default().push(index);
                        }
                        receipts.insert((journal, role.sequence), index);
                    }
                    RolePhase::Outcome => {
                        let outcome = serde_json::json!({
                            "sequence": role.sequence, "kind": role.kind,
                            "key": {"class": role.key.class, "id": role.key.id,
                                "generation": role.key.generation},
                            "status": role.status, "transaction": role.transaction,
                            "ordinal": ordinal, "turn": turn,
                        });
                        let holders = open_keys.get(&key).map_or(&[][..], Vec::as_slice);
                        let joined = if role.transaction != 0 {
                            holders
                                .iter()
                                .rev()
                                .find(|&&index| {
                                    submissions[index]["transaction"] == role.transaction
                                })
                                .map(|&index| (index, "key_and_transaction"))
                        } else if let [index] = holders {
                            Some((*index, "key_only"))
                        } else {
                            None
                        };
                        let mut outcome = outcome;
                        match joined {
                            Some((index, join)) => {
                                let submission = &mut submissions[index];
                                outcome["join"] = join.into();
                                outcome["class"] =
                                    turn_class(submission["turn"].as_u64().unwrap(), turn).into();
                                submission["outcomes"].as_array_mut().unwrap().push(outcome);
                            }
                            None => {
                                outcome["reason"] = match (holders.len(), role.transaction) {
                                    (0, _) => "no_submission",
                                    (_, 0) => "ambiguous_key",
                                    _ => "transaction_mismatch",
                                }
                                .into();
                                outcome["candidates"] = holders
                                    .iter()
                                    .map(|&index| submissions[index]["sequence"].clone())
                                    .collect();
                                unmatched.push(outcome);
                            }
                        }
                    }
                    RolePhase::Released => {
                        if role.sequence > role.related {
                            violations.push(format!(
                                "journal {journal} released receipt {} under ack {}",
                                role.sequence, role.related
                            ));
                        }
                        match last_ack.get(&journal) {
                            Some(&(requested, AckResult::Advanced | AckResult::Repeat))
                                if requested == role.related => {}
                            _ => violations.push(format!(
                                "journal {journal} release of {} follows no ack {}",
                                role.sequence, role.related
                            )),
                        }
                        match receipts.get(&(journal, role.sequence)) {
                            Some(&index) => {
                                submissions[index]["released"] = serde_json::json!({
                                    "ack": role.related, "ordinal": ordinal, "turn": turn,
                                })
                            }
                            None => violations.push(format!(
                                "journal {journal} released unknown receipt {}",
                                role.sequence
                            )),
                        }
                    }
                }
            }
        }
    }
    if dropped != 0 {
        violations.push(format!("{dropped} trace events did not fit"));
    }
    // A keyed submission left unanswered when a later one took its key.
    for holders in open_keys.values() {
        for pair in holders.windows(2) {
            if submissions[pair[0]]["outcomes"]
                .as_array()
                .unwrap()
                .is_empty()
            {
                submissions[pair[0]]["superseded_by"] = submissions[pair[1]]["sequence"].clone();
            }
        }
    }

    // Read pairing: which reads delivered each receipt and its outcomes.
    let mut pairing_by_kind = BTreeMap::<u64, BTreeMap<String, u64>>::new();
    for submission in &mut submissions {
        let j = &journals[&submission["journal"].as_u64().unwrap()];
        let (receipt, receipt_ack) = j.custody(submission["sequence"].as_u64().unwrap());
        let receipt_read = receipt["read"].as_u64();
        submission["receipt_delivery"] = receipt;
        submission["receipt_ack"] = receipt_ack;
        let counts = pairing_by_kind
            .entry(submission["kind"].as_u64().unwrap())
            .or_default();
        let outcomes = submission["outcomes"].as_array_mut().unwrap();
        if outcomes.is_empty() {
            *counts.entry("no_outcome".into()).or_default() += 1;
        }
        for (position, outcome) in outcomes.iter_mut().enumerate() {
            let (delivery, ack) = j.custody(outcome["sequence"].as_u64().unwrap());
            let pairing = match (receipt_read, delivery["read"].as_u64()) {
                (Some(receipt), Some(read)) if receipt == read => "joined_read",
                (Some(_), Some(_)) => "split_read",
                _ => "not_delivered",
            };
            outcome["delivery"] = delivery;
            outcome["ack"] = ack;
            outcome["read_pairing"] = pairing.into();
            if position == 0 {
                *counts.entry(format!("first_{pairing}")).or_default() += 1;
            }
            *counts.entry(pairing.into()).or_default() += 1;
        }
    }

    // Per submitted kind: answered or not, and when.
    let mut by_kind = BTreeMap::<u64, BTreeMap<&str, u64>>::new();
    for submission in &submissions {
        let counts = by_kind
            .entry(submission["kind"].as_u64().unwrap())
            .or_default();
        *counts.entry("submitted").or_default() += 1;
        let outcomes = submission["outcomes"].as_array().unwrap();
        let class = if submission["key"].is_null() {
            "no_outcome_key"
        } else if outcomes.is_empty() && !submission["superseded_by"].is_null() {
            "superseded_without_outcome"
        } else if outcomes.is_empty() {
            "keyed_without_outcome"
        } else if outcomes[0]["class"] == "same_turn" {
            "first_outcome_same_turn"
        } else {
            "first_outcome_later_turn"
        };
        *counts.entry(class).or_default() += 1;
        *counts.entry("outcomes").or_default() += outcomes.len() as u64;
        if submission["released"].is_null() {
            *counts.entry("unreleased").or_default() += 1;
        }
    }

    let journals: Vec<_> = journals
        .iter()
        .map(|(id, j)| {
            let mut delivery = BTreeMap::<&str, u64>::new();
            let mut kinds = BTreeMap::<u16, u64>::new();
            for (sequence, (_, commit_turn)) in &j.records {
                *kinds
                    .entry(j.kinds.get(sequence).copied().unwrap_or(0))
                    .or_default() += 1;
                let class = match j.delivered.get(sequence) {
                    Some((_, delivered_turn, _)) => turn_class(*commit_turn, *delivered_turn),
                    None => "undelivered",
                };
                *delivery.entry(class).or_default() += 1;
            }
            let parked_before_delivery: u64 = j.delivered.values().map(|d| d.2).sum();
            serde_json::json!({
                "journal": id, "epoch": j.epoch, "records": j.records.len(),
                "kinds": kinds.iter().map(|(k, n)| (k.to_string(), *n)).collect::<BTreeMap<_, _>>(),
                "delivery": delivery, "delivered_through": j.delivered_through,
                "parked_before_delivery": parked_before_delivery,
                "parked_after_last_delivery": j.parked,
                "reads": READS.iter().zip(j.reads).map(|(k, n)| (*k, n)).collect::<BTreeMap<_, _>>(),
                "acks": ACKS.iter().zip(j.acks)
                    .map(|(k, n)| (*k, n)).collect::<BTreeMap<_, _>>(),
                "largest_ack_lag": j.largest_ack_lag,
                "completed_records_per_read": j.completed_per_read.iter()
                    .map(|(k, n)| (k.to_string(), *n)).collect::<BTreeMap<_, _>>(),
            })
        })
        .collect();
    serde_json::json!({
        "scope": "comparator journal identity trace; diagnostic only, never a timing run",
        "events": events.len(), "dropped": dropped,
        "journals": journals,
        "submitted_by_kind": by_kind.iter().map(|(k, c)| (k.to_string(), c)).collect::<BTreeMap<_, _>>(),
        "read_pairing_by_kind": pairing_by_kind.iter().map(|(k, c)| (k.to_string(), c)).collect::<BTreeMap<_, _>>(),
        "unmatched_outcomes": unmatched,
        "tables": {
            "commit_columns": COMMIT_COLUMNS, "commits": commit_table,
            "read_columns": READ_COLUMNS, "reads": read_table,
            "ack_columns": ACK_COLUMNS, "acks": ack_table,
        },
        "submissions": submissions,
        "violations": violations,
    })
}

/// This thread's trace, one traced fixture at a time. A fixture that panics
/// releases the trace for the next.
pub struct Active {
    _serial: std::sync::MutexGuard<'static, ()>,
    taken: bool,
}

pub fn begin(capacity: usize) -> Active {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    sophia_9p::journal_trace::begin(capacity);
    Active {
        _serial: serial,
        taken: false,
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        if !self.taken {
            let _ = sophia_9p::journal_trace::take();
        }
    }
}

/// End the trace, write its summary when a journal directory is set, and
/// assert the invariants.
pub fn finish(mut active: Active, stem: &str) -> serde_json::Value {
    let (events, dropped) = sophia_9p::journal_trace::take();
    active.taken = true;
    let summary = analyze(&events, dropped);
    if let Some(directory) = std::env::var_os("SOPHIA_TIMING_PEER_JOURNALS") {
        serde_json::to_writer_pretty(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::path::Path::new(&directory).join(format!("{stem}.journal-trace.json")))
                .unwrap(),
            &summary,
        )
        .unwrap();
    }
    assert_eq!(
        summary["violations"],
        serde_json::json!([]),
        "journal trace"
    );
    assert!(
        summary["submissions"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "trace saw the shell journal"
    );
    summary
}

/// Read pairing on a synthetic journal: a receipt and its outcome in one
/// read, an outcome committed later and read separately, a batch split
/// across two reads, an undelivered second outcome, and a command with no
/// outcome.
#[test]
fn journal_trace_pairs_receipts_and_outcomes_by_read() {
    use sophia_9p::journal_trace::{RoleKey, RoleRecord};
    const J: u64 = 3;
    let candidate = |id| RoleKey {
        class: "candidate_generation",
        id,
        generation: 0,
    };
    let role = |sequence, phase, kind, key, status| {
        Event::Role(RoleRecord {
            journal: J,
            sequence,
            phase,
            kind,
            transaction: 6,
            key,
            status,
            related: sequence + 100,
        })
    };
    let commit = |sequence: u64| Event::Commit {
        journal: J,
        first: sequence,
        last: sequence,
        start: (sequence - 1) * 10,
        end: sequence * 10,
    };
    let read = |offset, delivered, first, last, starts_inside, ends_inside| Event::Read {
        journal: J,
        offset,
        requested: 64,
        delivered,
        first,
        last,
        starts_inside,
        ends_inside,
        result: ReadResult::Ready,
        parked: 0,
    };
    let ack = |requested| Event::Ack {
        journal: J,
        epoch: 1,
        requested,
        result: AckResult::Advanced,
        acknowledged: requested,
        floor: requested * 10,
        last_committed: requested,
    };
    let events = vec![
        Event::Opened {
            journal: J,
            epoch: 1,
            next_sequence: 1,
            tail: 0,
        },
        Event::Mark { label: 1 },
        // 0..3: receipt 1 and outcome 2 committed, then one read takes both.
        commit(1),
        role(1, RolePhase::Submitted, 262, candidate(7), 0),
        commit(2),
        role(2, RolePhase::Outcome, 35, candidate(7), 1),
        read(0, 20, 1, 2, false, false),
        ack(2),
        // Receipt 3 is read alone; its outcome 4 commits a turn later.
        commit(3),
        role(3, RolePhase::Submitted, 262, candidate(8), 0),
        read(20, 10, 3, 3, false, false),
        Event::Mark { label: 2 },
        commit(4),
        role(4, RolePhase::Outcome, 35, candidate(8), 1),
        read(30, 10, 4, 4, false, false),
        ack(4),
        // A command with no outcome key.
        commit(5),
        role(5, RolePhase::Submitted, 263, RoleKey::NONE, 0),
        // Receipt 6 and outcome 7, split by a read that completes records
        // 5 and 6 and ends inside record 7.
        commit(6),
        role(6, RolePhase::Submitted, 262, candidate(9), 0),
        commit(7),
        role(7, RolePhase::Outcome, 35, candidate(9), 1),
        read(40, 25, 5, 7, false, true),
        read(65, 5, 7, 7, true, false),
        // A reread completes nothing.
        read(0, 10, 1, 1, false, false),
        ack(7),
        // The second outcome of candidate 7 is never read.
        commit(8),
        role(8, RolePhase::Outcome, 35, candidate(7), 2),
    ];
    let summary = analyze(&events, 0);
    assert_eq!(summary["violations"], serde_json::json!([]));
    let pairing = |submission: usize| -> Vec<_> {
        summary["submissions"][submission]["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o["sequence"].as_u64().unwrap(),
                    o["read_pairing"].as_str().unwrap().to_owned(),
                    o["delivery"]["read"].as_u64(),
                    o["ack"]["sequence"].as_u64(),
                )
            })
            .collect()
    };
    let read_of = |sequence: u64| {
        events
            .iter()
            .position(|e| matches!(e, Event::Read { first, last, .. } if *first <= sequence && sequence <= *last))
    };
    assert_eq!(
        pairing(0),
        [
            (
                2,
                "joined_read".to_owned(),
                read_of(2).map(|r| r as u64),
                Some(2)
            ),
            (8, "not_delivered".to_owned(), None, None),
        ]
    );
    assert_eq!(
        pairing(1),
        [(
            4,
            "split_read".to_owned(),
            read_of(4).map(|r| r as u64),
            Some(4)
        )]
    );
    // Record 7 completes in the continuation, not the read holding receipt 6.
    let continuation = events
        .iter()
        .position(|e| matches!(e, Event::Read { offset: 65, .. }))
        .unwrap() as u64;
    assert_eq!(
        pairing(3),
        [(7, "split_read".to_owned(), Some(continuation), Some(7))]
    );
    assert_eq!(summary["submissions"][0]["receipt_ack"]["sequence"], 2);
    assert_eq!(summary["submissions"][2]["outcomes"], serde_json::json!([]));
    assert_eq!(summary["submissions"][2]["receipt_ack"]["sequence"], 7);
    assert_eq!(
        summary["read_pairing_by_kind"],
        serde_json::json!({
            "262": {"first_joined_read": 1, "first_split_read": 2, "joined_read": 1,
                "split_read": 2, "not_delivered": 1},
            "263": {"no_outcome": 1},
        })
    );
    assert_eq!(
        summary["journals"][0]["completed_records_per_read"],
        serde_json::json!({"0": 1, "1": 3, "2": 2})
    );
    let tables = &summary["tables"];
    assert_eq!(tables["commits"].as_array().unwrap().len(), 8);
    assert_eq!(tables["acks"].as_array().unwrap().len(), 3);
    assert_eq!(
        tables["reads"][4],
        serde_json::json!([
            continuation,
            2,
            J,
            65,
            64,
            5,
            7,
            7,
            true,
            false,
            "Ready",
            0,
            1
        ])
    );
}

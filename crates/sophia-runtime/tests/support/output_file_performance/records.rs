use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    Sample {
        workload: String,
        index: u64,
        start: u64,
        end: u64,
        epoch: u64,
        transaction: u64,
    },
    Ready {
        workload: String,
        epoch: u64,
    },
    Done {
        workload: String,
        count: u64,
    },
    Failure {
        workload: String,
        index: u64,
        reason: String,
    },
}

pub fn parse(line: &str) -> Result<Record, String> {
    if line.len() > 192 || !line.is_ascii() {
        return Err("oversize or non-ASCII peer record".into());
    }
    let fields: Vec<_> = line.split_ascii_whitespace().collect();
    let number = |s: &str| -> Result<u64, String> {
        if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
            return Err("invalid peer integer".into());
        }
        s.parse().map_err(|_| "peer integer overflow".into())
    };
    match fields.as_slice() {
        ["S", workload, index, start, end, epoch, transaction] => {
            let start = number(start)?;
            let end = number(end)?;
            let epoch = number(epoch)?;
            if end < start || epoch == 0 {
                return Err("invalid sample clock or epoch".into());
            }
            Ok(Record::Sample {
                workload: workload.to_string(),
                index: number(index)?,
                start,
                end,
                epoch,
                transaction: number(transaction)?,
            })
        }
        ["READY", workload, epoch] => Ok(Record::Ready {
            workload: workload.to_string(),
            epoch: number(epoch)?,
        }),
        ["DONE", workload, count] => Ok(Record::Done {
            workload: workload.to_string(),
            count: number(count)?,
        }),
        ["F", workload, index, reason] => Ok(Record::Failure {
            workload: workload.to_string(),
            index: number(index)?,
            reason: reason.to_string(),
        }),
        _ => Err(format!("invalid peer record: {line:?}")),
    }
}

pub fn percentile(samples: &[u64], percent: usize) -> u64 {
    assert!(!samples.is_empty() && (1..=100).contains(&percent));
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[(percent * sorted.len()).div_ceil(100) - 1]
}

/// Both directions matter: an extra owner delivery and an unobserved terminal
/// fail just as a missing proposal does. The caller rejects duplicates before
/// inserting into these sets.
pub fn reconcile(
    deliveries: &BTreeSet<(u64, u64)>,
    outcomes: &BTreeSet<(u64, u64)>,
) -> Result<(), String> {
    if deliveries != outcomes {
        return Err(format!(
            "delivery/outcome mismatch: deliveries={}, outcomes={}",
            deliveries.len(),
            outcomes.len()
        ));
    }
    Ok(())
}

#[test]
fn records_refuse_overflow_truncation_and_reversed_clocks() {
    for bad in [
        "S connect 0 2 1 1 0",
        "S connect 0 1 2 0 0",
        "S connect 0 1 2 1",
        "DONE idle 18446744073709551616",
        "READY idle -1",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    assert!(parse(&"x".repeat(193)).is_err());
}

#[test]
fn nearest_rank_keeps_tail_samples_and_does_not_interpolate() {
    let hundred: Vec<_> = (1..=100).collect();
    let thousand: Vec<_> = (1..=1000).collect();
    assert_eq!(percentile(&hundred, 99), 99);
    assert_eq!(percentile(&thousand, 99), 990);
    assert_eq!(percentile(&[1, 2, 10_000], 99), 10_000);
}

#[test]
fn unmatched_owner_work_or_terminal_fails_reconciliation() {
    let one = BTreeSet::from([(1, 1)]);
    let two = BTreeSet::from([(1, 1), (1, 2)]);
    assert!(reconcile(&one, &two).is_err());
    assert!(reconcile(&two, &one).is_err());
    assert!(reconcile(&one, &BTreeSet::from([(2, 1)])).is_err());
    reconcile(&one, &one).unwrap();
}

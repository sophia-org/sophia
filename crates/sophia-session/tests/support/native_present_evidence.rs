use super::*;

fn record(id: u64) -> Record {
    Record::Mixed {
        transaction: TransactionId::from_raw(id),
        surface: SurfaceId::new(1, 1),
        source: Size {
            width: 16,
            height: 16,
        },
        target: Rect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        },
        clip: None,
        stable: true,
        nonzero_rgb_pixels: 256,
        ust: id,
        msc: id,
    }
}

#[test]
fn aggregation_counts_all_retirements_and_emits_a_bounded_exact_tail_once() {
    let mut evidence = NativePresentEvidence::new(true);
    for id in 1..=1000 {
        evidence.record(record(id), false);
    }
    assert_eq!(evidence.retired, 1000);
    assert_eq!(evidence.attempted, 2000);
    assert_eq!(evidence.emitted, 0);
    assert_eq!(evidence.coalesced, 2 * (1000 - CAPACITY as u64));
    let retained = (evidence.next..CAPACITY)
        .chain(0..evidence.next)
        .map(|i| match evidence.recent[i].unwrap() {
            Record::Mixed { transaction, .. } => transaction.raw(),
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        retained,
        (1001 - CAPACITY as u64..=1000).collect::<Vec<_>>()
    );
    evidence.flush();
    assert_eq!(evidence.emitted, 2 * CAPACITY as u64);
    assert_eq!(evidence.attempted, evidence.emitted + evidence.coalesced);
    evidence.flush();
    assert_eq!(evidence.emitted, 2 * CAPACITY as u64);
    assert!(evidence.recent.iter().all(Option::is_none));
}

#[test]
fn explicit_proof_witnesses_and_diagnostic_sessions_bypass_coalescing() {
    for (aggregate, exact) in [(true, true), (false, false)] {
        let mut evidence = NativePresentEvidence::new(aggregate);
        for id in 1..=100 {
            evidence.record(record(id), exact);
        }
        assert_eq!(evidence.emitted, 200);
        assert_eq!(evidence.coalesced, 0);
        assert!(evidence.recent.iter().all(Option::is_none));
        evidence.flush();
        assert_eq!(evidence.emitted, 200);
    }
}

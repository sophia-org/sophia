//! Schema 3 cost records carry cumulative process page faults. Fault counts
//! are monotonic; no particular touch pattern is required to raise them.
#![cfg(all(feature = "comparator-counters", not(feature = "comparator-progress")))]
use sophia_runtime::comparator_counters::{self as counters, Faults};

fn mode() -> &'static str {
    if cfg!(feature = "comparator-profiling") {
        "profile"
    } else if cfg!(feature = "comparator-allocation-counting") {
        "rust-global"
    } else {
        "cpu-only"
    }
}

#[test]
fn fault_counts_never_decrease_and_stay_inside_the_clock_bounds() {
    let first = counters::snapshot().unwrap();
    let pages = std::hint::black_box(vec![1u8; 1 << 20]);
    let second = counters::snapshot().unwrap();
    drop(pages);
    let third = counters::snapshot().unwrap();
    for (earlier, later) in [(first, second), (second, third)] {
        assert!(later.minor_faults >= earlier.minor_faults);
        assert!(later.major_faults >= earlier.major_faults);
        assert!(earlier.after_usec <= later.before_usec);
    }
    let direct = counters::process_faults().unwrap();
    assert!(direct.minor >= third.minor_faults);
    assert!(direct.major >= third.major_faults);
}

#[test]
fn a_failed_fault_read_fails_the_snapshot_without_a_substitute() {
    let error =
        counters::snapshot_with_faults(|| Err(std::io::Error::other("no rusage"))).unwrap_err();
    assert_eq!(error.to_string(), "no rusage");
    let supplied = counters::snapshot_with_faults(|| {
        Ok(Faults {
            minor: 11,
            major: 3,
        })
    })
    .unwrap();
    assert_eq!((supplied.minor_faults, supplied.major_faults), (11, 3));
}

#[test]
fn schema_three_names_its_mode_and_both_fault_fields() {
    assert_eq!(counters::MODE, mode());
    let snapshot = counters::snapshot_with_faults(|| {
        Ok(Faults {
            minor: 17,
            major: 5,
        })
    })
    .unwrap();
    let mut bytes = [0u8; 1024];
    let mut sink = std::io::Cursor::new(bytes.as_mut_slice());
    counters::write_snapshot(&mut sink, "peer", "frame_end", 9, snapshot).unwrap();
    let len = sink.position() as usize;
    let line = std::str::from_utf8(&bytes[..len]).unwrap();
    let fields: Vec<_> = line.split_whitespace().collect();
    assert_eq!(
        fields[..3],
        [
            "sophia_comparator_counters",
            "schema=3",
            format!("mode={}", mode()).as_str()
        ]
    );
    let names: Vec<_> = fields[3..]
        .iter()
        .map(|field| field.split_once('=').unwrap().0)
        .collect();
    assert_eq!(
        names,
        [
            "process",
            "pid",
            "stage",
            "sequence",
            "before_usec",
            "after_usec",
            "cpu_ns",
            "allocations",
            "reallocations",
            "deallocations",
            "bytes_allocated",
            "minor_faults",
            "major_faults"
        ]
    );
    assert!(line.ends_with(" minor_faults=17 major_faults=5\n"));
}

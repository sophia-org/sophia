#![cfg(feature = "comparator-allocation-counting")]
#[path = "../examples/shell_timing_peer/journal.rs"]
mod journal;
use journal::{Event, Upload};
use sophia_runtime::comparator_counters as counters;
use std::{
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[global_allocator]
static ALLOCATOR: counters::Allocator = counters::ALLOCATOR;
static SAMPLES: AtomicUsize = AtomicUsize::new(0);

struct SampleSink;
impl Write for SampleSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        SAMPLES.fetch_add(1, Ordering::SeqCst);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn events() -> [Event<'static>; 9] {
    [
        Event::Start {
            frames: 1,
            wire: "files",
            probe_bytes: 0,
        },
        Event::UploadOffer {
            upload: Upload {
                connection_epoch: 1,
                content_grant_epoch: 2,
                output: 3,
                candidate_generation: 4,
                resource_id: 5,
                resource_generation: 6,
                width: 1920,
                height: 24,
                total_bytes: 184320,
                begin_usec: 7,
            },
        },
        Event::UploadAccepted {
            resource_id: 5,
            resource_generation: 6,
            accepted_usec: 8,
        },
        Event::ProbeReleased {
            resource_id: 5,
            resource_generation: 6,
            released_usec: 9,
        },
        Event::Permit {
            generation: 4,
            demand_usec: 10,
            received_usec: 11,
        },
        Event::Offer {
            connection_epoch: 1,
            content_grant_epoch: 2,
            output: 3,
            candidate_generation: 4,
            submitted_monotonic_usec: 12,
        },
        Event::Terminal {
            connection_epoch: 1,
            content_grant_epoch: 2,
            output: 3,
            candidate_generation: 4,
            outcome: "presented",
            kind: 2,
            reason: 0,
            observed_monotonic_usec: 13,
        },
        Event::End {
            offered: 1,
            completed: 1,
        },
        Event::Failed {
            error: "quoted \" and newline\n",
        },
    ]
}

fn wait_samples(want: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while SAMPLES.load(Ordering::SeqCst) < want {
        assert!(Instant::now() < deadline, "sampler stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

// One test in its own executable: other tests must not allocate while measuring.
#[test]
fn measurement_paths_do_not_allocate_after_initialization() {
    let mut bytes = [0u8; 1024];
    let before = counters::snapshot();
    for sequence in 0..1000 {
        counters::write_snapshot(
            &mut std::io::Cursor::new(&mut bytes[..]),
            "session",
            "sample",
            sequence,
            counters::snapshot(),
        )
        .unwrap();
        for event in events() {
            journal::write(&mut std::io::Cursor::new(&mut bytes[..]), event).unwrap();
        }
    }
    let after = counters::snapshot();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.reallocations, before.reallocations);

    // The real timeout loop is measured after its thread/channel initialization.
    let sampler = counters::Sampler::start_with_output(SampleSink).unwrap();
    wait_samples(10);
    let before = counters::snapshot();
    wait_samples(SAMPLES.load(Ordering::SeqCst) + 30);
    let after = counters::snapshot();
    sampler.finish().unwrap();
    assert_eq!(after.allocations, before.allocations, "sampler allocated");
    assert_eq!(
        after.reallocations, before.reallocations,
        "sampler reallocated"
    );

    for event in events() {
        let mut out = std::io::Cursor::new(&mut bytes[..]);
        journal::write(&mut out, event).unwrap();
        let len = out.position() as usize;
        let value: serde_json::Value = serde_json::from_slice(&bytes[..len]).unwrap();
        assert_eq!(value["schema"], 2);
        assert!(value["event"].is_string());
    }
    let oversized = "x".repeat(2048);
    let mut out = Vec::new();
    assert!(journal::write(&mut out, Event::Failed { error: &oversized }).is_err());
    assert!(out.is_empty());
}

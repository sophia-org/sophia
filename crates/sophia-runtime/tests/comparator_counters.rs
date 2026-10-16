#![cfg(feature = "comparator-allocation-counting")]
use sophia_runtime::comparator_counters::{self as counters, Allocator};

#[global_allocator]
static ALLOCATOR: Allocator = counters::ALLOCATOR;

#[test]
fn instrumented_counts_include_real_allocations_with_ordered_clock_bounds() {
    let before = counters::snapshot();
    let bytes = std::hint::black_box(vec![7u8; 4096]);
    let after = counters::snapshot();
    assert!(before.before_usec <= before.after_usec);
    assert!(before.after_usec <= after.before_usec);
    assert!(after.before_usec <= after.after_usec);
    assert!(after.cpu_ns >= before.cpu_ns);
    assert!(after.allocations > before.allocations);
    assert!(after.bytes_allocated >= before.bytes_allocated + bytes.len());
    drop(bytes);
    assert!(counters::snapshot().deallocations > after.deallocations);
}

#[test]
fn sampler_stops_and_joins_without_waiting_for_its_sample_budget() {
    let sampler = counters::Sampler::start().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(8));
    sampler.finish().unwrap();
}

#[test]
fn counter_record_is_one_bounded_write_and_overflow_writes_nothing() {
    #[derive(Default)]
    struct Sink(Vec<Vec<u8>>);
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink::default();
    counters::write_snapshot(&mut sink, "session", "sample", 42, counters::snapshot()).unwrap();
    assert_eq!(sink.0.len(), 1);
    let line = std::str::from_utf8(&sink.0[0]).unwrap();
    assert!(line.starts_with("sophia_comparator_counters schema=1 process=session "));
    assert!(line.contains("stage=sample sequence=42 "));
    assert!(line.ends_with('\n'));
    assert!(line.len() <= 1024);
    assert!(
        counters::write_snapshot(
            &mut sink,
            &"x".repeat(1024),
            "sample",
            43,
            counters::snapshot()
        )
        .is_err()
    );
    assert_eq!(sink.0.len(), 1, "overflow must not emit a partial record");
}

#[test]
fn shared_pipe_keeps_concurrent_counter_and_json_records_separate() {
    use std::io::{Read, Write};
    let (read, write) = rustix::pipe::pipe().unwrap();
    let mut reader = std::fs::File::from(read);
    let mut counter_writer = std::fs::File::from(write);
    let mut json_writer = counter_writer.try_clone().unwrap();
    let counters = std::thread::spawn(move || {
        for sequence in 0..1000 {
            counters::write_snapshot(
                &mut counter_writer,
                "session",
                "sample",
                sequence,
                counters::snapshot(),
            )
            .unwrap();
        }
    });
    let json = std::thread::spawn(move || {
        for _ in 0..1000 {
            json_writer
                .write_all(b"{\"event\":\"upload_accepted\",\"schema\":2}\n")
                .unwrap();
        }
    });
    let mut text = String::new();
    reader.read_to_string(&mut text).unwrap();
    counters.join().unwrap();
    json.join().unwrap();
    let mut count = 0;
    let mut journals = 0;
    for line in text.lines() {
        if line.starts_with('{') {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(line).unwrap()["event"],
                "upload_accepted"
            );
            journals += 1;
        } else {
            assert!(line.starts_with("sophia_comparator_counters schema=1 "));
            assert!(!line.contains('{'));
            assert!(line.contains(&format!("sequence={count} ")));
            assert_eq!(line.split_whitespace().count(), 13);
            count += 1;
        }
    }
    assert_eq!((count, journals), (1000, 1000));
}

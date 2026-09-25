// The protocol watermark's own contract: a mark names what was queued, a
// wait ends when the writer has drained to it, and a wait past what will
// ever be drained ends at its bound (t229).

#[test]
fn a_wait_ends_when_the_writer_drains_to_the_mark() {
    let watermark = std::sync::Arc::new(X11ProtocolWatermark::default());
    watermark.queued();
    watermark.queued();
    let mark = watermark.mark();
    assert_eq!(mark, 2);
    let writer = {
        let watermark = watermark.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            watermark.drained();
            watermark.drained();
        })
    };
    let started = std::time::Instant::now();
    watermark.wait_drained(mark, std::time::Duration::from_secs(5));
    assert!(started.elapsed() >= std::time::Duration::from_millis(15), "the wait held until the drain");
    assert!(started.elapsed() < std::time::Duration::from_secs(4), "and ended on it, not on the bound");
    writer.join().unwrap();
    // Already drained: no wait at all.
    let started = std::time::Instant::now();
    watermark.wait_drained(mark, std::time::Duration::from_secs(5));
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
}

#[test]
fn a_wait_for_more_than_will_be_drained_ends_at_its_bound() {
    let watermark = X11ProtocolWatermark::default();
    watermark.queued();
    let started = std::time::Instant::now();
    watermark.wait_drained(watermark.mark(), std::time::Duration::from_millis(40));
    let elapsed = started.elapsed();
    assert!(elapsed >= std::time::Duration::from_millis(40), "the bound was served out: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(2), "and no longer: {elapsed:?}");
}

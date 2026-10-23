use super::*;
use std::time::Duration;

#[test]
fn texture_timing_is_opt_in_and_preserves_cache_counts() {
    let mut cache = NativeDmaBufImportCache::with_capacity_and_stats(
        1,
        NativeDmaBufImportCacheStats {
            imports: 3,
            hits: 2,
            ..Default::default()
        },
    );
    let before = cache.stats();
    cache.record_texture_timing(None);
    assert_eq!(cache.stats(), before);

    let started = super::super::RenderStageTimer::start();
    std::thread::sleep(Duration::from_millis(1));
    cache.record_texture_timing(Some(started));
    let first = cache.stats();
    assert_eq!(first.texture_calls, 1);
    assert!(first.texture_elapsed >= Duration::from_millis(1));
    assert_eq!(first.imports, 3);
    assert_eq!(first.hits, 2);
    cache.record_texture_timing(Some(super::super::RenderStageTimer::start()));
    let second = cache.stats();
    assert_eq!(second.texture_calls, 2);
    assert!(second.texture_cpu >= first.texture_cpu);
    assert!(second.texture_elapsed >= first.texture_elapsed);
}

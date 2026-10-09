#![cfg(test)]

use super::runtime_output_retry_delay;
use std::time::Duration;

#[test]
fn a_runtime_rescan_series_is_the_startup_series_and_then_ends() {
    let series = (0..5).map(runtime_output_retry_delay).collect::<Vec<_>>();
    assert_eq!(
        series,
        [
            Some(Duration::from_millis(250)),
            Some(Duration::from_millis(1_000)),
            Some(Duration::from_millis(4_000)),
            None,
            None,
        ]
    );
}

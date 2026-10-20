#![cfg(feature = "comparator-progress")]
use sophia_9p::progress::{
    self,
    exchanges::{self, Operation},
};

#[test]
fn operations_count_attempts_but_masks_count_passes_and_scope_ends_on_drop() {
    let before = exchanges::snapshot();
    exchanges::observe(Operation::Ack);
    {
        let _pass = progress::owner_pass();
        exchanges::observe(Operation::Record);
        exchanges::observe(Operation::Submit);
        exchanges::observe(Operation::Ack);
        exchanges::observe(Operation::Ack);
        exchanges::observe(Operation::EventReady);
        exchanges::observe(Operation::EventPending);
        exchanges::observe(Operation::EventError);
    }
    {
        let _empty = progress::owner_pass();
    }
    exchanges::observe(Operation::EventPending);
    let after = exchanges::snapshot();
    assert_eq!(
        std::array::from_fn::<_, 6, _>(|i| after.scoped[i] - before.scoped[i]),
        [1, 1, 2, 1, 1, 1]
    );
    assert_eq!(
        std::array::from_fn::<_, 6, _>(|i| after.unscoped[i] - before.unscoped[i]),
        [0, 0, 1, 0, 1, 0]
    );
    assert_eq!(after.masks[63] - before.masks[63], 1);
    assert_eq!(after.masks[0] - before.masks[0], 1);
    assert_eq!(
        after.masks.iter().sum::<u64>() - before.masks.iter().sum::<u64>(),
        2
    );
    let worst = exchanges::Snapshot {
        scoped: [u64::MAX; 6],
        unscoped: [u64::MAX; 6],
        masks: [u64::MAX; 64],
    };
    let line = format!(
        "sophia_shell_exchanges schema=1 mode=progress process=session pid={} sequence={} before_usec={} after_usec={}{worst}",
        u32::MAX,
        u64::MAX,
        u64::MAX,
        u64::MAX
    );
    let mut output = Vec::new();
    progress::wait_sources::write_line(&mut output, format_args!("{line}")).unwrap();
    assert!(output.len() <= 4096);
}

use sophia_protocol::OutputId;

/// Preserve physical head order and inspect current topology facts on each visit.
pub(super) fn enabled_head_indices(
    heads: impl Iterator<Item = (bool, OutputId)>,
    output: OutputId,
) -> impl Iterator<Item = usize> {
    heads
        .enumerate()
        .filter_map(move |(index, (enabled, id))| (enabled && id == output).then_some(index))
}

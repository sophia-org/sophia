//! Conservative retained-scene suppression after every mirror head has settled.
use super::*;

#[derive(Clone, Copy)]
pub(crate) struct SettledMirrorHead {
    pub head: sophia_engine::RenderHeadId,
    pub target_generation: u64,
    pub idle: bool,
    pub presented: Option<LiveProductionScanoutContent>,
    pub displayed: Option<crate::LiveNativeFrameIdentity>,
}

pub(crate) fn settled_mirror_checksum(
    owner: crate::NativeFrameOwner,
    output: OutputId,
    expected_heads: usize,
    group: Option<&LiveProductionMirrorGroupLifecycle>,
    heads: impl IntoIterator<Item = SettledMirrorHead>,
) -> Option<u64> {
    if !output.is_valid() || !(2..=sophia_engine::MAX_HEADS_PER_OUTPUT).contains(&expected_heads) {
        return None;
    }
    let group = group?;
    if group.output() != output
        || group.failed()
        || !group.converged()
        || group.heads().count() != expected_heads
    {
        return None;
    }
    let mut seen = [None; sophia_engine::MAX_HEADS_PER_OUTPUT];
    let mut count = 0;
    let mut common = None;
    for head in heads {
        if count >= expected_heads
            || !head.idle
            || !head.head.is_valid()
            || head.target_generation == 0
            || seen[..count].contains(&Some(head.head))
        {
            return None;
        }
        let content = head.presented?;
        if content.frame().raw() == 0 || group.displayed_frame(head.head) != Some(content.frame()) {
            return None;
        }
        let checksum = content.logical_checksum()?;
        let expected = owner.frame(
            output,
            head.head,
            head.target_generation,
            content.frame().raw(),
        );
        if head.displayed != Some(expected) {
            return None;
        }
        let identity = (content.frame(), checksum);
        if common.is_some_and(|common| common != identity) {
            return None;
        }
        common = Some(identity);
        seen[count] = Some(head.head);
        count += 1;
    }
    (count == expected_heads).then_some(common?.1)
}

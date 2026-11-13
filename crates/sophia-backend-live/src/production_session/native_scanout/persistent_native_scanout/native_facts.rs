//! Head and queue facts over the owner's parts. The owner delegates to these
//! rules, so a device-free fixture holding the same parts (GATE3-SCOPE G3-5a)
//! reads one native authority through the owner's own code.
use super::*;

type Heads = [LiveProductionNativeHead];

/// The enabled heads on `output`, in physical head order.
pub(crate) fn enabled_heads(heads: &Heads, output: OutputId) -> impl Iterator<Item = usize> + '_ {
    head_selection::enabled_head_indices(
        heads.iter().map(|head| (head.enabled, head.output.id)),
        output,
    )
}

pub(crate) fn head_render_targets(
    heads: &Heads,
    output: OutputId,
) -> Vec<sophia_engine::HeadRenderTarget> {
    enabled_heads(heads, output)
        .filter_map(|index| {
            reduce_live_production_head_render_target(
                LiveProductionNativeTopologyCurrentHead::new_with_target(
                    heads[index].head,
                    heads[index].enabled,
                    heads[index].group,
                    output,
                    heads[index].selection,
                    heads[index].target_generation,
                    heads[index].scale,
                    heads[index].refresh_millihz,
                    heads[index].transform,
                    heads[index].mapping,
                    heads[index].vrr,
                ),
            )
        })
        .collect()
}

/// Each render target with its head index, and the first head's contents
/// from pending to presented: what ordinary composition admission reads.
pub(crate) fn composition_targets(
    heads: &Heads,
    output: OutputId,
) -> (
    Vec<(usize, sophia_engine::HeadRenderTarget)>,
    [Option<LiveProductionScanoutContent>; 4],
) {
    let indices = enabled_heads(heads, output).collect::<Vec<_>>();
    let targets = head_render_targets(heads, output)
        .into_iter()
        .map(|target| {
            let index = *indices
                .iter()
                .find(|index| heads[**index].head == target.head)
                .expect("native head target");
            (index, target)
        })
        .collect();
    let newest = indices.first().map_or([None; 4], |index| {
        let head = &heads[*index];
        [
            head.pending_content,
            head.rendering_content,
            head.submitted_content,
            head.presented_content,
        ]
    });
    (targets, newest)
}

pub(crate) fn installed_retirement_protected(heads: &Heads, output: OutputId) -> bool {
    enabled_heads(heads, output).any(|index| heads[index].retirement_protected())
}

pub(crate) fn output_retirement_protected(
    heads: &Heads,
    deferred: &composition_queue::DeferredNativeCompositions,
    output: OutputId,
) -> bool {
    deferred.protected(output) || installed_retirement_protected(heads, output)
}

/// Owed native work on `output`, apart from preview recovery.
pub(crate) fn heads_pending_frame<R: RenderDeviceDiscoveryBackend>(
    heads: &Heads,
    exporters: &[NativeGbmRenderedScanoutBufferDiscoveryExporter<R>],
    deferred: &composition_queue::DeferredNativeCompositions,
    output: OutputId,
) -> bool {
    let mirror = enabled_heads(heads, output).nth(1).is_some();
    deferred.pending(output)
        || enabled_heads(heads, output).any(|index| {
            exporters[index].pending_frame()
                || heads[index].prepared_scanout.is_some()
                || heads[index].pending_content.is_some()
                || heads[index].rendering_content.is_some()
                || (!mirror && heads[index].scanout_custody.submitted().is_some())
        })
}

pub(crate) fn heads_in_flight(heads: &Heads, output: OutputId) -> bool {
    enabled_heads(heads, output).any(|index| heads[index].scanout_custody.submitted().is_some())
}

/// Resource cleanup owed by a head of `output`, apart from topology cleanup.
pub(crate) fn heads_cleanup_pending(heads: &Heads, output: OutputId) -> bool {
    enabled_heads(heads, output).any(|index| heads[index].scanout_custody.cleanup_pending())
}

/// The owner's one native frame counter: ordinary batches and combined
/// admissions both draw from it.
pub(crate) fn allocate_frame_id(next: &mut u64) -> LiveProductionNativeFrameId {
    let frame = LiveProductionNativeFrameId::from_raw(*next);
    *next = next
        .checked_add(1)
        .expect("native frame ID space exhausted");
    frame
}

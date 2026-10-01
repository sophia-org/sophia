//! Exercises the production handoff, with real runtime sets and fake DRM cleanup.
use super::*;
use crate::drm::framebuffer_test_device::Device;
use crate::{LiveProductionOutputRuntimeSet, NativeFrameOwner, PersistentScanoutCustody};
use sophia_engine::{HeadlessOutput, RenderHeadId};
use std::num::NonZeroU32;

fn runtimes(ids: &[u64]) -> LiveProductionOutputRuntimeSet {
    LiveProductionOutputRuntimeSet::new(
        &ids.iter()
            .map(|id| HeadlessOutput {
                id: OutputId::from_raw(*id),
                ..HeadlessOutput::deterministic()
            })
            .collect::<Vec<_>>(),
        &[],
        None,
    )
    .unwrap()
}

fn displayed(
    fb: u32,
    identity: Option<crate::LiveNativeFrameIdentity>,
) -> crate::BoxedRenderedPrimaryPlaneScanoutSubmission {
    crate::LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer: Box::new(()),
        correlation: Some(crate::LiveRendererFrameCorrelation {
            native: identity,
            request: None,
            trace: None,
            direct_scanout: None,
        }),
        primary_plane: crate::LibdrmNativePrimaryPlaneScanoutSubmission {
            resources: crate::LibdrmNativePrimaryPlaneResourceBundle::new(
                NonZeroU32::new(fb).unwrap().into(),
                None,
                HeadlessOutput::deterministic().size,
            ),
            completion_fence: None,
        },
        submitted_after_page_flip_serial: None,
        layout_witness: None,
    }
}

#[test]
fn topology_handoff_moves_singletons_keeps_mirrors_and_routes_disabled_old_heads() {
    let owner = NativeFrameOwner::new();
    let mut previous = runtimes(&[1, 2]);
    // Old output IDs no longer name the same groups. Head 4 is now disabled.
    for (runtime, (head, fb)) in previous.values_mut().zip([(4, 40), (1, 10)]) {
        let state = runtime.runtime.primary_output_state_mut();
        state
            .scanout_custody
            .adopt_displayed(displayed(
                fb,
                Some(owner.frame(state.output, RenderHeadId::from_raw(head), 1, 1)),
            ))
            .unwrap();
    }
    let mut next = runtimes(&[8, 9]);
    let device_a = Device::new();
    let device_b = Device::new();
    let mut custody: [PersistentScanoutCustody; 4] = std::array::from_fn(|_| Default::default());
    for (index, cell) in custody.iter_mut().take(3).enumerate() {
        cell.adopt_displayed(displayed(101 + index as u32, None))
            .unwrap();
    }
    let mut heads = custody
        .iter_mut()
        .enumerate()
        .map(|(index, cell)| TopologyCustodyHead {
            head: RenderHeadId::from_raw(index as u64 + 1),
            output: OutputId::from_raw(if index == 0 { 8 } else { 9 }),
            enabled: index < 3,
            custody: cell,
            device: if index == 3 { &device_b } else { &device_a },
        })
        .collect::<Vec<_>>();
    assert!(
        handoff_topology_custody(owner, &mut heads, &mut previous, &mut next)
            .unwrap()
            .is_empty()
    );
    assert_eq!(*device_a.destroyed.borrow(), [10]);
    assert_eq!(*device_b.destroyed.borrow(), [40]);
    assert!(
        previous
            .values()
            .all(|output| !output.runtime.rendered_primary_plane_scanout_displayed())
    );
    assert!(
        next.values()
            .next()
            .unwrap()
            .runtime
            .rendered_primary_plane_scanout_displayed()
    );
    assert!(
        !next
            .values()
            .nth(1)
            .unwrap()
            .runtime
            .rendered_primary_plane_scanout_displayed()
    );
    assert!(heads[0].custody.displayed().is_none());
    assert!(heads[1].custody.displayed().is_some());
    assert!(heads[2].custody.displayed().is_some());
    assert!(heads[3].custody.displayed().is_none());
}

#[test]
fn topology_handoff_preflights_every_owner_before_retiring_or_transferring_any() {
    for fault in [
        "foreign",
        "missing",
        "unknown",
        "submitted",
        "destination",
        "empty",
    ] {
        let owner = NativeFrameOwner::new();
        let device = Device::new();
        let mut previous = runtimes(&[1, 2]);
        for (index, output) in previous.values_mut().enumerate() {
            let identity = if index == 1 && fault == "missing" {
                None
            } else {
                Some(
                    (if index == 1 && fault == "foreign" {
                        NativeFrameOwner::new()
                    } else {
                        owner
                    })
                    .frame(
                        OutputId::from_raw(index as u64 + 1),
                        RenderHeadId::from_raw(if index == 1 && fault == "unknown" {
                            99
                        } else {
                            1
                        }),
                        1,
                        1,
                    ),
                )
            };
            let cell = &mut output.runtime.primary_output_state_mut().scanout_custody;
            if index == 1 && fault == "submitted" {
                cell.accept_submission(displayed(20, identity)).unwrap();
            } else {
                cell.adopt_displayed(displayed(10 + index as u32, identity))
                    .unwrap();
            }
        }
        let mut next = runtimes(&[3]);
        if fault == "destination" {
            next.values_mut()
                .next()
                .unwrap()
                .runtime
                .primary_output_state_mut()
                .scanout_custody
                .adopt_displayed(displayed(50, None))
                .unwrap();
        }
        let mut custody = PersistentScanoutCustody::default();
        if fault != "empty" {
            custody.adopt_displayed(displayed(30, None)).unwrap();
        }
        let mut heads = [TopologyCustodyHead {
            head: RenderHeadId::from_raw(1),
            output: OutputId::from_raw(3),
            enabled: true,
            custody: &mut custody,
            device: &device,
        }];
        let error =
            handoff_topology_custody(owner, &mut heads, &mut previous, &mut next).unwrap_err();
        let expected = match fault {
            "foreign" => "foreign displayed owner",
            "missing" => "old displayed head identity",
            "unknown" => "lost the old displayed head",
            "submitted" => "quiescent runtime custody",
            _ => "cannot adopt the displayed singleton owner",
        };
        assert!(error.contains(expected), "{fault}: {error}");
        assert!(
            device.destroyed.borrow().is_empty(),
            "{fault}: preflight must not retire an earlier valid output"
        );
        assert!(
            previous
                .values()
                .next()
                .unwrap()
                .runtime
                .rendered_primary_plane_scanout_displayed()
        );
        assert_eq!(heads[0].custody.displayed().is_some(), fault != "empty");
    }
}

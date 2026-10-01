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
        state.native_custody_scope = Some((owner, RenderHeadId::from_raw(head)));
        state
            .scanout_custody
            .adopt_displayed(displayed(
                fb,
                Some(owner.frame(state.output, RenderHeadId::from_raw(head), 1, 1)),
            ))
            .unwrap();
    }
    let mut next = runtimes(&[8, 9]);
    next.values_mut()
        .next()
        .unwrap()
        .runtime
        .primary_output_state_mut()
        .native_custody_scope = Some((owner, RenderHeadId::from_raw(1)));
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
        "frame_scope",
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
            let state = output.runtime.primary_output_state_mut();
            state.native_custody_scope = identity.map(|identity| {
                (
                    if index == 1 && fault == "foreign" {
                        NativeFrameOwner::new()
                    } else {
                        owner
                    },
                    if index == 1 && fault == "frame_scope" {
                        RenderHeadId::from_raw(9)
                    } else {
                        identity.head()
                    },
                )
            });
            let cell = &mut state.scanout_custody;
            if index == 1 && fault == "submitted" {
                cell.accept_submission(displayed(20, identity)).unwrap();
            } else {
                cell.adopt_displayed(displayed(10 + index as u32, identity))
                    .unwrap();
            }
        }
        let mut next = runtimes(&[3]);
        next.values_mut()
            .next()
            .unwrap()
            .runtime
            .primary_output_state_mut()
            .native_custody_scope = Some((owner, RenderHeadId::from_raw(1)));
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
            "missing" => "old displayed head scope",
            "unknown" => "lost the old displayed head",
            "submitted" => "quiescent runtime custody",
            "frame_scope" => "frame disagrees with its head scope",
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

#[test]
fn topology_handoff_can_roll_back_before_any_ordinary_frame_has_presented() {
    let owner = NativeFrameOwner::new();
    let head_id = RenderHeadId::from_raw(1);
    let device = Device::new();
    let mut previous = runtimes(&[1]);
    let mut applied = runtimes(&[2]);
    applied
        .values_mut()
        .next()
        .unwrap()
        .runtime
        .primary_output_state_mut()
        .native_custody_scope = Some((owner, head_id));
    let mut head = PersistentScanoutCustody::default();
    // Topology composition uses an unidentified mixed frame. No ordinary
    // submission or page-flip event has happened on either side of this test.
    head.adopt_displayed(displayed(60, None)).unwrap();
    assert!(
        handoff_topology_custody(
            owner,
            &mut [TopologyCustodyHead {
                head: head_id,
                output: OutputId::from_raw(2),
                enabled: true,
                custody: &mut head,
                device: &device,
            }],
            &mut previous,
            &mut applied
        )
        .unwrap()
        .is_empty()
    );
    assert!(device.destroyed.borrow().is_empty());
    let mut restored = runtimes(&[1]);
    restored
        .values_mut()
        .next()
        .unwrap()
        .runtime
        .primary_output_state_mut()
        .native_custody_scope = Some((owner, head_id));
    head.adopt_displayed(displayed(61, None)).unwrap();
    assert!(
        handoff_topology_custody(
            owner,
            &mut [TopologyCustodyHead {
                head: head_id,
                output: OutputId::from_raw(1),
                enabled: true,
                custody: &mut head,
                device: &device,
            }],
            &mut applied,
            &mut restored
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(*device.destroyed.borrow(), [60]);
    assert!(
        !applied
            .values()
            .next()
            .unwrap()
            .runtime
            .rendered_primary_plane_scanout_displayed()
    );
    assert!(
        restored
            .values()
            .next()
            .unwrap()
            .runtime
            .rendered_primary_plane_scanout_displayed()
    );
    assert!(head.displayed().is_none());
}

#[test]
fn topology_rollback_handoff_retires_candidate_flip_before_replacing_its_displayed_owner() {
    let owner = NativeFrameOwner::new();
    let head_id = RenderHeadId::from_raw(1);
    let output = OutputId::from_raw(1);
    let identity = owner.frame(output, head_id, 3, 11);
    let device = Device::new();
    let mut candidate = runtimes(&[1]);
    let state = candidate
        .values_mut()
        .next()
        .unwrap()
        .runtime
        .primary_output_state_mut();
    state.native_custody_scope = Some((owner, head_id));
    state
        .scanout_custody
        .adopt_displayed(displayed(70, None))
        .unwrap();
    state
        .scanout_custody
        .accept_submission(displayed(71, Some(identity)))
        .unwrap();
    assert!(candidate.native_scanout_in_flight());
    let state = candidate
        .values_mut()
        .next()
        .unwrap()
        .runtime
        .primary_output_state_mut();
    assert!(
        state
            .scanout_custody
            .retire_replaced_displayed(&device)
            .is_err()
    );
    assert!(device.destroyed.borrow().is_empty());
    let callback = crate::LivePageFlipCallbackReport {
        decision: crate::LivePageFlipCallbackDecision::Accepted,
        event: crate::LivePageFlipEvent {
            status: crate::LivePageFlipEventStatus::Presented,
            frame_serial: Some(1),
        },
    };
    assert!(matches!(
        state
            .scanout_custody
            .present(&device, &callback, Some(identity)),
        crate::PersistentFlipOutcome::Presented { .. }
    ));
    assert_eq!(*device.destroyed.borrow(), [70]);
    assert!(state.scanout_custody.displayed().is_some());
    assert!(!candidate.native_scanout_in_flight());

    // Only now supply the blocking restoration commit. The candidate's first
    // frame remains displayed until that event, then the production handoff
    // retires it and adopts the restored image.
    let mut head = PersistentScanoutCustody::default();
    head.adopt_displayed(displayed(72, None)).unwrap();
    let mut restored = runtimes(&[1]);
    restored
        .values_mut()
        .next()
        .unwrap()
        .runtime
        .primary_output_state_mut()
        .native_custody_scope = Some((owner, head_id));
    assert!(
        handoff_topology_custody(
            owner,
            &mut [TopologyCustodyHead {
                head: head_id,
                output,
                enabled: true,
                custody: &mut head,
                device: &device,
            }],
            &mut candidate,
            &mut restored
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(*device.destroyed.borrow(), [70, 71]);
    assert!(!candidate.native_scanout_in_flight());
    assert!(
        restored
            .values()
            .next()
            .unwrap()
            .runtime
            .rendered_primary_plane_scanout_displayed()
    );
    assert!(head.displayed().is_none());
}

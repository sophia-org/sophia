use super::topology::project_installed_output_topology;
use super::*;
use sophia_protocol::{OutputHeadMapping, OutputTransform, OutputVrrPolicy, Size};

fn mode(refresh: u32, clock: u32) -> ::drm::control::Mode {
    ::drm::control::Mode::from(drm_ffi::drm_mode_modeinfo {
        clock,
        hdisplay: 2560,
        hsync_start: 2608,
        hsync_end: 2640,
        htotal: 2720,
        vdisplay: 1440,
        vsync_start: 1443,
        vsync_end: 1448,
        vtotal: 1525,
        vrefresh: refresh,
        flags: 9,
        ..Default::default()
    })
}

fn selection(mode: Option<::drm::control::Mode>) -> crate::LibdrmNativePrimaryPlaneSelection {
    crate::LibdrmNativePrimaryPlaneSelection::new(
        ::drm::control::from_u32(94).unwrap(),
        ::drm::control::from_u32(80).unwrap(),
        ::drm::control::from_u32(59).unwrap(),
        Size {
            width: 2560,
            height: 1440,
        },
        mode,
    )
}

fn head(
    mode: Option<::drm::control::Mode>,
    refresh: u32,
    generation: u64,
) -> LiveProductionNativeTopologyCurrentHead {
    LiveProductionNativeTopologyCurrentHead::new_with_target(
        sophia_engine::RenderHeadId::from_raw(1),
        true,
        0,
        OutputId::from_raw(1),
        selection(mode),
        generation,
        1,
        refresh,
        OutputTransform::Normal,
        OutputHeadMapping::Fit,
        OutputVrrPolicy::Disabled,
    )
}

fn snapshot() -> (
    sophia_protocol::OutputAuthoritySnapshot,
    crate::LibdrmNativeOutputTiming,
) {
    let boot = crate::native_output_timing(mode(60, 248_875));
    let startup = crate::native_output_timing(mode(120, 497_750));
    let capability = crate::LibdrmNativeOutputCapability::new(
        OutputId::from_raw(1),
        94,
        "Display-1",
        [boot, startup],
        Some(boot),
        boot,
        crate::LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
    .bind_head(sophia_engine::RenderHeadId::from_raw(1))
    .unwrap();
    let snapshot = crate::project_live_output_authority_snapshot(
        &[capability],
        &[sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(1),
            size: selection(None).size(),
            scale: 1,
        }],
        1,
    )
    .unwrap();
    (snapshot, boot)
}

#[test]
fn rollback_projection_uses_installed_mode_after_startup_and_later_commits() {
    let (mut published, boot) = snapshot();
    // The discovery table stays anchored at 60 Hz while the committed state
    // advances to 120 Hz, then back to 60 Hz in the same native instance.
    for (epoch, refresh, clock) in [(2, 120, 497_750), (3, 60, 248_875)] {
        published.topology_epoch = epoch;
        published.heads[0].generation = epoch;
        published.heads[0].current_mode = Some(
            published.heads[0]
                .modes
                .iter()
                .find(|m| m.refresh_millihz == refresh * 1000)
                .unwrap()
                .mode,
        );
        let current = [head(Some(mode(refresh, clock)), refresh * 1000, epoch)];
        let rollback = project_installed_output_topology(&current, &published).unwrap();
        assert_eq!(
            rollback.targets[0].timing,
            crate::LibdrmNativeOutputTiming {
                width: 2560,
                height: 1440,
                refresh_millihz: refresh * 1000,
                mode: Some(sophia_protocol::OutputModeTiming {
                    clock_khz: clock,
                    hdisplay: 2560,
                    hsync_start: 2608,
                    hsync_end: 2640,
                    htotal: 2720,
                    hskew: 0,
                    vdisplay: 1440,
                    vsync_start: 1443,
                    vsync_end: 1448,
                    vtotal: 1525,
                    flags: 9,
                }),
            }
        );
        assert_eq!(rollback.targets[0].target_generation, epoch);
        if epoch == 2 {
            assert_eq!(
                project_live_production_published_topology(&current, &published, |_| Ok(boot)),
                Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)
            );
        }
    }
}

#[test]
fn rollback_projection_preserves_same_refresh_modeline_and_refuses_missing_modes() {
    let (published, _) = snapshot();
    // Same nominal mode as discovery, different clock and blanking. The
    // installed timing, including every field, is the restoration target.
    let mut raw: drm_ffi::drm_mode_modeinfo = mode(60, 249_000).into();
    raw.hsync_start = 2610;
    raw.hsync_end = 2644;
    raw.htotal = 2730;
    raw.hskew = 1;
    raw.vsync_start = 1444;
    raw.vsync_end = 1450;
    raw.vtotal = 1530;
    raw.flags = 5;
    let installed = ::drm::control::Mode::from(raw);
    let current = [head(Some(installed), 60_000, 1)];
    let rollback = project_installed_output_topology(&current, &published).unwrap();
    assert_eq!(
        rollback.targets[0].timing.mode,
        Some(sophia_protocol::OutputModeTiming {
            clock_khz: 249_000,
            hdisplay: 2560,
            hsync_start: 2610,
            hsync_end: 2644,
            htotal: 2730,
            hskew: 1,
            vdisplay: 1440,
            vsync_start: 1444,
            vsync_end: 1450,
            vtotal: 1530,
            flags: 5,
        })
    );
    for invalid in [None, Some(mode(0, 0))] {
        assert_eq!(
            project_installed_output_topology(&[head(invalid, 60_000, 1)], &published),
            Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)
        );
    }
}

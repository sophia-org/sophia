#![cfg(all(feature = "gbm-probe", feature = "libdrm-events"))]
//! Pure conversions behind the environment readback. No device is touched.
use sophia_backend_live::LiveProductionOutputEnvironmentReadback as Readback;
use std::path::Path;

/// A mode tuple in readback order with the fields these tests use.
fn mode(
    vrefresh: u32,
    clock_khz: u32,
    htotal: u32,
    vtotal: u32,
    vscan: u32,
    flags: u32,
) -> [u32; 13] {
    let mut mode = [0; 13];
    mode[Readback::MODE_VREFRESH] = vrefresh;
    mode[Readback::MODE_CLOCK_KHZ] = clock_khz;
    mode[Readback::MODE_HTOTAL] = htotal;
    mode[Readback::MODE_VTOTAL] = vtotal;
    mode[Readback::MODE_VSCAN] = vscan;
    mode[Readback::MODE_FLAGS] = flags;
    mode
}

const INTERLACE: u32 = drm::control::ModeFlags::INTERLACE.bits();
const DBLSCAN: u32 = drm::control::ModeFlags::DBLSCAN.bits();

#[test]
fn nominal_refresh_is_whole_hz_times_one_thousand() {
    assert_eq!(
        Readback::nominal_refresh_millihz(&mode(120, 0, 0, 0, 0, 0)),
        Some(120_000)
    );
    assert_eq!(
        Readback::nominal_refresh_millihz(&mode(0, 1, 1, 1, 0, 0)),
        None
    );
    assert_eq!(
        Readback::nominal_refresh_millihz(&mode(u32::MAX, 0, 0, 0, 0, 0)),
        None
    );
}

#[test]
fn timing_refresh_is_derived_from_programmed_timing_and_rounded() {
    // 2560x1440 at 120 Hz (CVT reduced blanking): 497750 kHz, 2640 x 1571.
    let cvt = mode(120, 497_750, 2_640, 1_571, 0, 0);
    assert_eq!(Readback::timing_refresh_millihz(&cvt), Some(120_014));
    assert_eq!(Readback::nominal_refresh_millihz(&cvt), Some(120_000));
    // A 119.98 Hz mode whose nominal rate rounds to 120 Hz.
    let near = mode(120, 593_000, 2_720, 1_817, 0, 0);
    assert_eq!(Readback::timing_refresh_millihz(&near), Some(119_986));
    assert_eq!(Readback::nominal_refresh_millihz(&near), Some(120_000));
    // Exact: 148500 kHz over 2200 x 1125 is 60 Hz.
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(60, 148_500, 2_200, 1_125, 0, 0)),
        Some(60_000)
    );
    // Round half up: 1 kHz over 3 x 1 is 333333.33 mHz; over 2 x 3 is 166666.67.
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(0, 1, 3, 1, 0, 0)),
        Some(333_333)
    );
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(0, 1, 2, 3, 0, 0)),
        Some(166_667)
    );
}

#[test]
fn interlace_doublescan_and_vscan_follow_the_kernel_factors() {
    let base = (148_500, 2_200, 1_125);
    let rate = |vscan, flags| {
        Readback::timing_refresh_millihz(&mode(0, base.0, base.1, base.2, vscan, flags))
    };
    assert_eq!(rate(0, INTERLACE), Some(120_000));
    assert_eq!(rate(0, DBLSCAN), Some(30_000));
    assert_eq!(rate(1, 0), Some(60_000));
    assert_eq!(rate(3, 0), Some(20_000));
    assert_eq!(rate(2, INTERLACE | DBLSCAN), Some(30_000));
}

#[test]
fn invalid_or_overflowing_timing_has_no_rate() {
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(60, 148_500, 0, 1_125, 0, 0)),
        None
    );
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(60, 148_500, 2_200, 0, 0, 0)),
        None
    );
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(60, 0, 2_200, 1_125, 0, 0)),
        None
    );
    // Rounds to zero mHz.
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(0, 1, u32::MAX, u32::MAX, 0, 0)),
        None
    );
    // Too large for u32 mHz.
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(0, u32::MAX, 1, 1, 0, INTERLACE)),
        None
    );
    // Extreme factors do not overflow the intermediate arithmetic.
    assert_eq!(
        Readback::timing_refresh_millihz(&mode(0, u32::MAX, u32::MAX, u32::MAX, u32::MAX, DBLSCAN)),
        None
    );
}

#[test]
fn scale_vrr_connector_and_card_conversions_are_exact() {
    assert_eq!(Readback::applied_scale_milli(1), Some(1_000));
    assert_eq!(Readback::applied_scale_milli(2), Some(2_000));
    assert_eq!(Readback::applied_scale_milli(0), None);
    assert_eq!(Readback::applied_scale_milli(u32::MAX), None);

    assert_eq!(Readback::vrr_enabled(None).unwrap(), None);
    assert_eq!(Readback::vrr_enabled(Some(0)).unwrap(), Some(false));
    assert_eq!(Readback::vrr_enabled(Some(1)).unwrap(), Some(true));
    assert!(Readback::vrr_enabled(Some(2)).is_err());

    assert_eq!(Readback::connector_name("DP", 2).as_deref(), Some("DP-2"));
    assert_eq!(
        Readback::connector_name("HDMI-A", 1).as_deref(),
        Some("HDMI-A-1")
    );
    assert_eq!(Readback::connector_name("Unknown", 1), None);
    assert_eq!(Readback::connector_name("", 1), None);

    let card = Path::new("/dev/dri/card1");
    assert_eq!(
        Readback::card_node_name(card, 7, Some(7)).as_deref(),
        Some("card1")
    );
    assert_eq!(Readback::card_node_name(card, 7, Some(8)), None);
    assert_eq!(Readback::card_node_name(card, 7, None), None);
    assert_eq!(
        Readback::card_node_name(Path::new("/dev/dri/renderD128"), 7, Some(7)),
        None
    );
    assert_eq!(
        Readback::card_node_name(Path::new("/dev/dri/card"), 7, Some(7)),
        None
    );
    assert_eq!(
        Readback::card_node_name(Path::new("/dev/dri/card1 (deleted)"), 7, Some(7)),
        None
    );
}

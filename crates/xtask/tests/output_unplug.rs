//! The output-unplug verdict on logs built from the emitters' own formats:
//! each passing run, and each way a run must be refused.

#[path = "../src/output_unplug.rs"]
mod output_unplug;

use output_unplug::{Mode, verify};

const START: &str = "\
sophia_qemu_unplug schema=1 status=starting isolation=headless control=none host_drm=none host_vt=none gpu=virtio-gpu mode=MODE single_card=1
sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=output-unplug
sophia_qemu_topology schema=1 status=observed requested_heads=2 connectors=2 connected=2
sophia_qemu_unplug schema=1 status=running mode=MODE
sophia_live_session_startup schema=2 status=ready elapsed_msec=900 surface=true visual_detail=true presented=true outputs_ready=2/2 recovery_attempts=0
";

const LOSS: &str = "\
sophia_qemu_unplug schema=1 status=sent action=off target=card0-Virtual-2
sophia_live_input_epoch schema=1 reason=output_topology transition=1 epoch=2 revoked_leases=0
sophia_live_output_topology schema=1 status=quiesced transition=1 outcome=drained abandoned_scanouts=0
sophia_live_output_topology schema=1 status=published transition=1 topology_epoch=2 generation=2 outputs=1 changed=true restored_images=1 policy_required=false input=quarantined
sophia_live_output_topology schema=1 status=settled transition=1 retirements=1 input=enabled
";

const RETURN: &str = "\
sophia_qemu_unplug schema=1 status=sent action=on target=card0-Virtual-2
sophia_live_output_topology schema=1 status=quiesced transition=2 outcome=drained abandoned_scanouts=0
sophia_live_output_topology schema=1 status=published transition=2 topology_epoch=3 generation=3 outputs=2 changed=true restored_images=1 policy_required=false input=quarantined
sophia_live_output_topology schema=1 status=settled transition=2 retirements=2 input=enabled
";

const END: &str = "\
sophia_live_session schema=7 status=bounded_complete display=:181 elapsed_msec=40000
sophia_qemu_guest schema=1 status=complete scenario=output-unplug
sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0
";

fn uevents(hotplug: u32, removed: u32, added: u32) -> String {
    format!(
        "sophia_qemu_unplug schema=1 status=uevents drm_hotplug={hotplug} input_remove={removed} input_add={added}\n"
    )
}

fn run(mode: &str, middle: &str, uevents: &str) -> String {
    format!("{}{middle}{uevents}{END}", START.replace("MODE", mode))
}

fn one() -> String {
    run("one", LOSS, &uevents(1, 0, 0))
}

fn one_return() -> String {
    run("one-return", &format!("{LOSS}{RETURN}"), &uevents(2, 0, 0))
}

fn all_return() -> String {
    let loss = "\
sophia_qemu_unplug schema=1 status=sent action=off target=card0-Virtual-1
sophia_qemu_unplug schema=1 status=sent action=off target=card0-Virtual-2
sophia_live_output_topology schema=1 status=unavailable transition=1 retry_msec=250 error=no_connected_outputs
";
    let back = RETURN.replace(
        "action=on target=card0-Virtual-2",
        "action=on target=card0-Virtual-1\nsophia_qemu_unplug schema=1 status=sent action=on target=card0-Virtual-2",
    );
    run("all-return", &format!("{loss}{back}"), &uevents(4, 0, 0))
}

fn input_return() -> String {
    let middle = "\
sophia_qemu_unplug schema=1 status=sent action=off target=virtio2
sophia_qemu_unplug schema=1 status=sent action=on target=virtio2
";
    run("input-return", middle, &uevents(0, 2, 2))
}

#[test]
fn each_mode_passes_on_a_run_that_lived_through_it() {
    for (mode, log, lines) in [
        (Mode::One, one(), 2),
        (Mode::OneReturn, one_return(), 3),
        (Mode::AllReturn, all_return(), 2),
        (Mode::InputReturn, input_return(), 1),
    ] {
        let summary = verify(&log, mode).unwrap_or_else(|error| panic!("{mode:?}: {error}"));
        assert_eq!(summary.len(), lines, "{mode:?}: {summary:?}");
        assert!(summary[0].contains("status=passed"), "{mode:?}");
    }
}

#[test]
fn a_run_the_fixture_never_reached_is_named_as_such() {
    for (mode, log) in [
        (Mode::One, one().replace("drm_hotplug=1", "drm_hotplug=0")),
        (
            Mode::OneReturn,
            one_return().replace("drm_hotplug=2", "drm_hotplug=1"),
        ),
        (
            Mode::InputReturn,
            input_return().replace("input_add=2", "input_add=0"),
        ),
    ] {
        let error = verify(&log, mode).unwrap_err();
        assert!(error.starts_with("fixture unreached"), "{mode:?}: {error}");
    }
}

#[test]
fn the_session_ending_on_the_loss_is_refused_with_its_line() {
    // The shape of the KVM exit (kdleagg3): the fatal, then no completion.
    let log = one_return()
        .replace(
            "sophia_live_output_topology schema=1 status=published transition=1",
            "sophia_live_session_runtime_fatal schema=1 status=detected source=owner_loop action=bounded_cleanup failure_code=handoff_head_coverage_changed error=\"x\"\nsophia_live_output_topology schema=1 status=published transition=1",
        );
    let error = verify(&log, Mode::OneReturn).unwrap_err();
    assert!(error.contains("handoff_head_coverage_changed"), "{error}");
}

#[test]
fn every_missing_obligation_is_refused() {
    let cases = [
        (Mode::One, one().replace("outputs=1 changed=true", "outputs=1 changed=false"), "never published 1"),
        (Mode::One, one().replace("retirements=1 input=enabled", "retirements=1 input=quarantined"), "never published 1"),
        (Mode::One, one().replace("status=settled transition=1", "status=settled transition=9"), "never published 1"),
        (Mode::OneReturn, one_return().replace("outputs=2 changed=true", "outputs=1 changed=true"), "never published 2"),
        (Mode::AllReturn, all_return().replace("sophia_live_output_topology schema=1 status=unavailable transition=1 retry_msec=250 error=no_connected_outputs\n", ""), "no topology transition"),
        (Mode::One, one().replace("sophia_live_session schema=7 status=bounded_complete", "sophia_live_session schema=7 status=stopped"), "bounded completion"),
        (Mode::One, one().replace("sophia_qemu_guest schema=1 status=complete scenario=output-unplug\n", ""), "sophia_qemu_guest"),
        (Mode::One, one().replace("status=guest_exited qemu_exit=0", "status=failed reason=host_timeout"), "failure marker"),
        (Mode::OneReturn, one().replace("mode=one", "mode=one-return"), "fixture unreached"),
        (Mode::One, one().replace("connected=2", "connected=1"), "needs 2"),
        (Mode::One, one().replace("action=off", "action=noop"), "no removal"),
        (Mode::One, format!("{}{}", one(), "sophia_qemu_unplug schema=1 status=running mode=one\n"), "more than one"),
    ];
    for (index, (mode, log, expected)) in cases.into_iter().enumerate() {
        let error = verify(&log, mode).unwrap_err();
        assert!(error.contains(expected), "case {index}: {error}");
    }
}

#[test]
fn returns_before_removals_and_a_return_in_a_lasting_mode_are_refused() {
    let early = one_return().replace(
        "sophia_qemu_unplug schema=1 status=sent action=off target=card0-Virtual-2\n",
        "",
    );
    let early = early.replacen(
        "sophia_qemu_unplug schema=1 status=sent action=on",
        "sophia_qemu_unplug schema=1 status=sent action=on target=card0-Virtual-2\nsophia_qemu_unplug schema=1 status=sent action=off",
        1,
    );
    assert!(
        verify(&early, Mode::OneReturn)
            .unwrap_err()
            .contains("out of order")
    );
    let lasting = format!("{}{RETURN}", one().replace(END, "")) + END;
    assert!(
        verify(&lasting, Mode::One)
            .unwrap_err()
            .contains("out of order")
    );
}

#[test]
fn records_written_through_tracing_are_read_past_their_prefix_and_colour() {
    let log = one()
        .replace(
            "sophia_live_output_topology schema=1 status=published",
            "2026-10-05T00:39:33.100000Z \u{1b}[32m INFO\u{1b}[0m \u{1b}[2msophia_session::live_session::owner_loop:\u{1b}[0m sophia_live_output_topology schema=1 status=published",
        )
        .replace(
            "sophia_live_output_topology schema=1 status=settled",
            "2026-10-05T00:39:33.200000Z  INFO sophia_session::live_session: sophia_live_output_topology schema=1 status=settled",
        );
    assert_eq!(verify(&log, Mode::One).unwrap().len(), 2);
}

#[test]
fn a_second_unchanged_rebuild_may_settle_in_the_first_ones_place() {
    // The kernel's uevent rebuilds, then udev's processed event rebuilds
    // again before the first transition presented.
    let log = one().replace(
        "sophia_live_output_topology schema=1 status=settled transition=1 retirements=1 input=enabled",
        "sophia_live_output_topology schema=1 status=quiesced transition=2 outcome=drained abandoned_scanouts=0
sophia_live_output_topology schema=1 status=published transition=2 topology_epoch=2 generation=2 outputs=1 changed=false restored_images=0 policy_required=false input=quarantined
sophia_live_output_topology schema=2 status=presentation_timed_out transition=2 retirements=0 presentation_baseline=0 timeout_msec=2000 input=enabled",
    );
    let summary = verify(&log, Mode::One).unwrap();
    assert!(
        summary[1].contains("transition=2 settle=presentation_timed_out"),
        "{summary:?}"
    );
    // The settled rebuild must show the same outputs as the loss.
    let regressed = log.replace(
        "transition=2 topology_epoch=2 generation=2 outputs=1",
        "transition=2 topology_epoch=2 generation=2 outputs=2",
    );
    assert!(verify(&regressed, Mode::One).is_err());
}

#[test]
fn a_removal_after_the_session_began_to_stop_is_an_unreached_fixture() {
    // The session's startup took its whole runtime, so the host's removal
    // reached a session that was already stopping.
    let log = one().replace(
        "sophia_qemu_unplug schema=1 status=sent action=off",
        "sophia_live_session_quiescence schema=3 status=started reason=runtime_deadline timeout_msec=2000\nsophia_qemu_unplug schema=1 status=sent action=off",
    );
    let error = verify(&log, Mode::One).unwrap_err();
    assert!(error.starts_with("fixture unreached"), "{error}");
}

#[test]
fn modes_parse_by_name_only() {
    assert_eq!(Mode::parse("all-return"), Ok(Mode::AllReturn));
    assert!(Mode::parse("all").is_err());
    let error = verify(&one(), Mode::AllReturn).unwrap_err();
    assert!(error.contains("another mode"), "{error}");
}

/// A static DMA-BUF client across a loss and return: one mixed Present
/// before the barrier, the window's region before the removal, and the same
/// region (same size and checksum, moved) queued and retired after the return.
fn static_client() -> String {
    let start = START
        .replace(
            "status=running mode=MODE",
            "status=running mode=MODE wm=true client=dri3",
        )
        .replace("MODE", "one-return");
    let before = "\
sophia_live_session_present schema=2 status=retired transaction=26 surface=2097153 source=400x300 target=400x300_100_100 clip=400x300_100_100 unit_scale=true ust=1 msc=1
sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=3 scene_generation=26 target_generation=1
sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=26 layer=0 source_stage=renderer_image target=400x300_100_100 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=777
sophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=2 frame=3
sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding
";
    let after = "\
sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=9 scene_generation=40 target_generation=1
sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=40 layer=0 source_stage=renderer_image target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=777
sophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=5 frame=9
";
    format!(
        "{start}{before}{LOSS}{RETURN}{after}{}{END}",
        uevents(2, 0, 0)
    )
}

#[test]
fn a_static_client_whose_content_survives_passes() {
    let summary = verify(&static_client(), Mode::OneReturn).unwrap();
    assert!(
        summary
            .last()
            .is_some_and(|line| line.contains("status=static_content_retained")
                && line.contains("target_after=400x300_0_0")),
        "{summary:?}"
    );
}

#[test]
fn every_way_the_static_content_can_fail_is_refused() {
    let log = static_client();
    let cases = [
        // A second Present: the client was not static.
        (log.replace("sophia_qemu_unplug schema=1 status=static_barrier", "sophia_live_session_present schema=2 status=retired transaction=30 surface=2097153 source=400x300 target=400x300_100_100 clip=x unit_scale=true ust=2 msc=2\nsophia_qemu_unplug schema=1 status=static_barrier"), "exactly one retired Present"),
        // A software Present is not the DMA-BUF path.
        (log.replace("sophia_live_session_present schema=2", "sophia_live_session_present schema=4"), "mixed (DMA-BUF) path"),
        (log.replace("sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding\n", ""), "no barrier"),
        (log.replace("sophia_qemu_unplug schema=1 status=sent action=off", "sophia_live_renderer_image_handoff schema=1 status=discarded reason=heads_changed captured_images=1 discarded_images=1\nsophia_qemu_unplug schema=1 status=sent action=off"), "discarded"),
        // Other content in the window's place after the return.
        (log.replace("target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=777", "target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=778"), "not drawn from a retained image"),
        // Rendered but never retired.
        (log.replace("submission=5 frame=9", "submission=5 frame=10"), "not drawn from a retained image"),
        // Drawn from the client buffer, not a retained image.
        (log.replace("scene_generation=40 layer=0 source_stage=renderer_image", "scene_generation=40 layer=0 source_stage=dmabuf"), "not drawn from a retained image"),
    ];
    for (index, (log, expected)) in cases.into_iter().enumerate() {
        let error = verify(&log, Mode::OneReturn).unwrap_err();
        assert!(error.contains(expected), "case {index}: {error}");
    }
}

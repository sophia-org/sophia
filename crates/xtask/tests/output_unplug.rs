//! The output-unplug verdict on logs built from the emitters' own formats:
//! each passing run, and each way a run must be refused.

#[path = "../src/output_unplug.rs"]
mod output_unplug;

use output_unplug::{Mode, probe_frame_checksum, verify};

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
sophia_qemu_unplug schema=1 status=connectors connectors=2 connected=0 sample=20
sophia_live_output_topology schema=1 status=unavailable transition=1 retry_msec=250 error=\"persistent native scanout could not open all KMS outputs: SelectionFailed\"
";
    let back = RETURN.replace(
        "action=on target=card0-Virtual-2",
        "action=on target=card0-Virtual-1\nsophia_qemu_unplug schema=1 status=sent action=on target=card0-Virtual-2",
    );
    run("all-return", &format!("{loss}{back}"), &uevents(4, 0, 0))
}

/// Session's input records before readiness: a non-virtual keyboard, the
/// virtio keyboard (bus virtual) and the virtio tablet, as in a real guest.
const INPUT_DEVICES: &str = "\
sophia_live_session_input_device schema=1 status=added device=256 keyboard=true pointer=false touch=false virtual=false source=udev
sophia_live_session_input_device schema=1 status=added device=257 keyboard=true pointer=false touch=false virtual=true source=udev
sophia_live_session_input_device schema=1 status=added device=258 keyboard=false pointer=true touch=false virtual=true source=udev
";

/// The input mode's chain, in the guest's, the host's, Session's and the
/// client's own formats. The host's completed line follows the guest's
/// baseline marker, as it may when QMP delivers before the helper returns,
/// and the client's return key precedes Session's record of it.
const INPUT_CHAIN: &str = "\
dri3_layout stage=focus state=in source=event mode=0 detail=3 synthetic=0
dri3_layout stage=holding hold_ms=35000
dri3_layout stage=focus state=in source=query
sophia_qemu_unplug schema=1 status=input_baseline_ready device=257
sophia_qemu_unplug schema=1 status=key_sending phase=baseline key=a
sophia_live_session_input_device schema=1 status=key_observed device=257
dri3_layout stage=key keycode=38 synthetic=0
sophia_qemu_unplug schema=1 status=input_baseline device=257 routed=yes
sophia_qemu_unplug schema=1 status=key_sent phase=baseline result=completed
sophia_qemu_unplug schema=1 status=sent action=off target=virtio2
sophia_live_session_input_device schema=1 status=removed device=257 released=0
sophia_qemu_unplug schema=1 status=input_removed device=257
sophia_qemu_unplug schema=1 status=sent action=on target=virtio2
sophia_live_session_input_device schema=1 status=added device=262 keyboard=true pointer=false touch=false virtual=true source=udev
sophia_qemu_unplug schema=1 status=input_return_ready device=262
sophia_qemu_unplug schema=1 status=key_sending phase=return key=b
dri3_layout stage=key keycode=56 synthetic=0
sophia_live_session_input_device schema=1 status=key_observed device=262
sophia_qemu_unplug schema=1 status=key_sent phase=return result=completed
sophia_qemu_unplug schema=1 status=input_return_routed device=262
";

fn input_return() -> String {
    let start = START
        .replace(
            "status=running mode=MODE",
            "status=running mode=MODE wm=true client=dri3",
        )
        .replace("MODE", "input-return")
        .replace(
            "sophia_live_session_startup schema=2",
            &format!("{INPUT_DEVICES}sophia_live_session_startup schema=2"),
        );
    format!("{start}{INPUT_CHAIN}{}{END}", uevents(0, 2, 2))
}

/// The log with `line` (a whole line) taken out and put back before `before`.
fn moved(log: &str, line: &str, before: &str) -> String {
    let line = format!("{line}\n");
    assert_eq!(log.matches(&line).count(), 1, "{line}");
    let without = log.replacen(&line, "", 1);
    assert_eq!(without.matches(before).count(), 1, "{before}");
    without.replacen(before, &format!("{line}{before}"), 1)
}

#[test]
fn input_return_is_routed_to_the_client_before_and_after_the_keyboard_returns() {
    let summary = verify(&input_return(), Mode::InputReturn).unwrap();
    assert_eq!(
        summary[1],
        "sophia_qemu_output_unplug_verdict schema=1 status=input_routed baseline_device=257 return_device=262 baseline_keycode=38 return_keycode=56"
    );
}

#[test]
fn every_break_in_the_input_return_chain_is_refused() {
    let log = input_return();
    let observed_k0 = "sophia_live_session_input_device schema=1 status=key_observed device=257";
    let observed_k1 = "sophia_live_session_input_device schema=1 status=key_observed device=262";
    let added_k1 = "sophia_live_session_input_device schema=1 status=added device=262 keyboard=true pointer=false touch=false virtual=true source=udev";
    let removed_k0 =
        "sophia_live_session_input_device schema=1 status=removed device=257 released=0";
    let return_ready = "sophia_qemu_unplug schema=1 status=input_return_ready device=262";
    let sending_return = "sophia_qemu_unplug schema=1 status=key_sending phase=return key=b";
    let sending_baseline = "sophia_qemu_unplug schema=1 status=key_sending phase=baseline key=a";
    let sent_baseline =
        "sophia_qemu_unplug schema=1 status=key_sent phase=baseline result=completed";
    let routed = "sophia_qemu_unplug schema=1 status=input_return_routed device=262";
    let sent_return = "sophia_qemu_unplug schema=1 status=key_sent phase=return result=completed";
    let bounded_end =
        "sophia_live_session schema=7 status=bounded_complete display=:181 elapsed_msec=40000\n";
    let cases = [
        // The keyboard's first key observed only in the return phase.
        (moved(&log, observed_k0, routed), "baseline key was not observed"),
        // The returned keyboard is the old identity.
        (log.replace("device=262", "device=257"), "not a new identity"),
        (log.replace("status=added device=262", "status=added device=257"), "not a new identity"),
        // Two keyboards admitted at the return.
        (log.replace(&format!("{added_k1}\n"), &format!("{added_k1}\n{}\n", added_k1.replace("262", "263"))), "found 3"),
        // No removal of K0.
        (log.replace(&format!("{removed_k0}\n"), ""), "removal of K0, found 0"),
        // The client's key missing at the baseline, or at the return.
        (log.replace("dri3_layout stage=key keycode=38 synthetic=0\n", ""), "two client key reports, found 1"),
        (log.replace("dri3_layout stage=key keycode=56 synthetic=0\n", ""), "two client key reports, found 1"),
        // The baseline's keycode again at the return: buffered data.
        (log.replace("keycode=56", "keycode=38"), "expected 38 then 56"),
        // A client key before the baseline key was sent, none inside it.
        (moved(&log, "dri3_layout stage=key keycode=38 synthetic=0", sending_baseline), "baseline key was not observed"),
        // The return key sent before the guest named the return ready.
        (moved(&log, sending_return, return_ready), "return ready does not precede return key sending"),
        // Session saw K1's first key before the return key was sent.
        (moved(&log, observed_k1, sending_return), "returned keyboard's key was not observed"),
        // Markers out of order.
        (moved(&log, "sophia_qemu_unplug schema=1 status=input_removed device=257", removed_k0), "K0 removed does not precede removal marked"),
        (moved(&log, added_k1, "sophia_qemu_unplug schema=1 status=sent action=on"), "keyboard on does not precede K1 admitted"),
        // The client lost the focus by its last report before readiness.
        (log.replace("state=in source=query", "state=out source=query"), "did not hold the focus"),
        // A failed send, another key, a completion outside its phase.
        (log.replace("phase=baseline result=completed", "phase=baseline result=failed exit=1"), "did not complete"),
        (log.replace("phase=baseline key=a", "phase=baseline key=c"), "another key"),
        (moved(&log, sent_baseline, "sophia_qemu_unplug schema=1 status=key_sent phase=return"), "completed outside its phase"),
        // Keys or removals on other devices, a second removal action.
        (log.replace(&format!("{observed_k0}\n"), &format!("{observed_k0}\n{}\n", observed_k0.replace("257", "256"))), "another device"),
        (log.replace(&format!("{removed_k0}\n"), &format!("{removed_k0}\n{}\n", removed_k0.replace("257", "258"))), "another input device was removed"),
        (log.replace("sophia_qemu_unplug schema=1 status=sent action=on", "sophia_qemu_unplug schema=1 status=sent action=off target=virtio2\nsophia_qemu_unplug schema=1 status=sent action=on"), "one keyboard removal and one return"),
        // Overflowed copies and reports.
        (log.replace(routed, &format!("dri3_layout stage=key_overflow reported=64\n{routed}")), "overflowed"),
        (log.replace(routed, &format!("sophia_qemu_unplug schema=1 status=records_overflow lines=40000\n{routed}")), "overflowed"),
        // Without the WM and the managed client there is no input witness.
        (log.replace("wm=true client=dri3", "wm=true client=none"), "managed DRI3 client"),
        // The routed marker missing: nothing after the return proves routing.
        (log.replace(&format!("{routed}\n"), ""), "input_return_routed, found 0"),
        // A SendEvent key is no routing witness, at either phase; a
        // synthetic focus report does not count as the focus at readiness.
        (log.replace("keycode=56 synthetic=0", "keycode=56 synthetic=1"), "synthetic key reached the client"),
        (log.replace("keycode=38 synthetic=0", "keycode=38 synthetic=1"), "synthetic key reached the client"),
        (log.replace("keycode=38 synthetic=0", "keycode=38"), "malformed client key report"),
        (
            log.replace("dri3_layout stage=focus state=in source=query\n", "")
                .replace("detail=3 synthetic=0", "detail=3 synthetic=1"),
            "did not hold the focus",
        ),
        // The client's own failure lines after an otherwise complete chain:
        // a hold poll, a request error, a geometry change, a lost connection,
        // a failed finish.
        (log.replace(routed, &format!("{routed}\ndri3_layout status=failed stage=hold_poll errno=4")), "client reported a failure"),
        (log.replace(routed, &format!("{routed}\ndri3_layout status=failed stage=request x_error=3 major=12 minor=0 sequence=40 resource=4194305")), "client reported a failure"),
        (log.replace(routed, &format!("{routed}\ndri3_layout status=failed stage=geometry_changed errno=22")), "client reported a failure"),
        (log.replace(routed, &format!("{routed}\ndri3_layout status=failed stage=connection errno=32")), "client reported a failure"),
        (log.replace(routed, &format!("{routed}\ndri3_layout event=finished result=fail window=4194305 submitted=1 completed=1 idle=0")), "client reported a failure"),
        // The return key's completion after the bounded completion, the
        // guest's completion or its exit.
        (moved(&log, sent_return, "sophia_qemu_guest schema=1 status=complete"), "completed outside its phase"),
        (log.replace(&format!("{sent_return}\n"), "").replace(bounded_end, &format!("{bounded_end}{sent_return}\n")), "completed outside its phase"),
        (log.replace(&format!("{sent_return}\n"), "") + sent_return + "\n", "completed outside its phase"),
        // Phase markers counted in the whole log before their device is bound.
        (log.replace(&format!("{return_ready}\n"), &format!("{return_ready}\n{}\n", return_ready.replace("262", "300"))), "expected exactly one input_return_ready, found 2"),
        (log.replace("status=input_baseline_ready device=257", "status=input_baseline_ready device=256"), "input_baseline_ready names another device than K0"),
        (log.replace("status=input_return_routed device=262", "status=input_return_routed device=257"), "input_return_routed names another device than K1"),
        // Input records with a repeated or contradictory field, another
        // schema, a missing or extra field, or a non-numeric device.
        (log.replace(observed_k0, &format!("{observed_k0} device=999")), "malformed sophia_live_session_input_device status=key_observed"),
        (log.replace(added_k1, &added_k1.replace("schema=1", "schema=2")), "malformed sophia_live_session_input_device status=added"),
        (log.replace(removed_k0, &removed_k0.replace(" released=0", "")), "malformed sophia_live_session_input_device status=removed"),
        (log.replace("status=input_return_ready device=262", "status=input_return_ready device=26x"), "malformed sophia_qemu_unplug status=input_return_ready"),
        (log.replace(sending_return, &format!("{sending_return} key=a")), "malformed sophia_qemu_unplug status=key_sending"),
        (log.replace("status=input_baseline device=257 routed=yes", "status=input_baseline device=257 routed=yes routed=no"), "malformed sophia_qemu_unplug status=input_baseline"),
    ];
    for (index, (changed, expected)) in cases.into_iter().enumerate() {
        assert_ne!(changed, log, "case {index} must change the chain");
        let error = verify(&changed, Mode::InputReturn).unwrap_err();
        assert!(error.contains(expected), "case {index}: {error}");
    }
}

#[test]
fn the_guest_exit_must_be_clean_and_follow_the_bounded_completion() {
    let complete = "sophia_qemu_guest schema=1 status=complete scenario=output-unplug";
    let exited = "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0";
    let bounded = "sophia_live_session schema=7 status=bounded_complete";
    for log in [one(), all_return(), input_return()] {
        let mode = if log.contains("mode=input-return") {
            Mode::InputReturn
        } else if log.contains("mode=all-return") {
            Mode::AllReturn
        } else {
            Mode::One
        };
        verify(&log, mode).unwrap();
        let cases = [
            (
                log.replace(complete, &complete.replace("output-unplug", "device-test")),
                "not this scenario's",
            ),
            (
                log.replace(complete, &format!("{complete} extra=1")),
                "not this scenario's",
            ),
            (
                log.replace("qemu_exit=0", "qemu_exit=1"),
                "not a clean QEMU exit",
            ),
            (
                log.replace("qemu_exit=0", "qemu_exit=0 qemu_exit=0"),
                "not a clean QEMU exit",
            ),
            (
                log.replace(exited, "sophia_qemu_unplug schema=1 status=guest_exited"),
                "not a clean QEMU exit",
            ),
            (
                moved(&log, complete, bounded),
                "do not follow the bounded completion",
            ),
            (
                moved(&log, exited, complete),
                "do not follow the bounded completion",
            ),
            (
                log.replace(&format!("{exited}\n"), ""),
                "is missing sophia_qemu_unplug status=guest_exited",
            ),
            (
                log.replace(&format!("{complete}\n"), ""),
                "is missing sophia_qemu_guest status=complete",
            ),
        ];
        for (index, (changed, expected)) in cases.into_iter().enumerate() {
            assert_ne!(
                changed, log,
                "{mode:?} case {index} must change the endpoint"
            );
            let error = verify(&changed, mode).unwrap_err();
            assert!(error.contains(expected), "{mode:?} case {index}: {error}");
        }
    }
}

#[test]
fn each_mode_passes_on_a_run_that_lived_through_it() {
    for (mode, log, lines) in [
        (Mode::One, one(), 2),
        (Mode::OneReturn, one_return(), 3),
        (Mode::AllReturn, all_return(), 2),
        (Mode::InputReturn, input_return(), 2),
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
        (
            Mode::One,
            one().replace("outputs=1 changed=true", "outputs=1 changed=false"),
            "never published 1",
        ),
        (
            Mode::One,
            one().replace(
                "retirements=1 input=enabled",
                "retirements=1 input=quarantined",
            ),
            "never published 1",
        ),
        (
            Mode::One,
            one().replace("status=settled transition=1", "status=settled transition=9"),
            "never published 1",
        ),
        (
            Mode::OneReturn,
            one_return().replace("outputs=2 changed=true", "outputs=1 changed=true"),
            "never published 2",
        ),
        (
            Mode::AllReturn,
            all_return().replace("status=unavailable", "status=quiesced"),
            "no unavailable topology",
        ),
        (
            Mode::One,
            one().replace(
                "sophia_live_session schema=7 status=bounded_complete",
                "sophia_live_session schema=7 status=stopped",
            ),
            "bounded completion",
        ),
        (
            Mode::One,
            one().replace(
                "sophia_qemu_guest schema=1 status=complete scenario=output-unplug\n",
                "",
            ),
            "sophia_qemu_guest",
        ),
        (
            Mode::One,
            one().replace(
                "status=guest_exited qemu_exit=0",
                "status=failed reason=host_timeout",
            ),
            "failure marker",
        ),
        (
            Mode::OneReturn,
            one().replace("mode=one", "mode=one-return"),
            "fixture unreached",
        ),
        (
            Mode::One,
            one().replace("connected=2", "connected=1"),
            "needs 2",
        ),
        (
            Mode::One,
            one().replace("action=off", "action=noop"),
            "no removal",
        ),
        (
            Mode::One,
            format!(
                "{}{}",
                one(),
                "sophia_qemu_unplug schema=1 status=running mode=one\n"
            ),
            "more than one",
        ),
    ];
    for (index, (mode, log, expected)) in cases.into_iter().enumerate() {
        let error = verify(&log, mode).unwrap_err();
        assert!(error.contains(expected), "case {index}: {error}");
    }
}

#[test]
fn all_heads_loss_requires_guest_and_session_witnesses_after_the_last_removal() {
    let zero = "sophia_qemu_unplug schema=1 status=connectors connectors=2 connected=0 sample=20\n";
    let unavailable = "sophia_live_output_topology schema=1 status=unavailable transition=1 retry_msec=250 error=\"persistent native scanout could not open all KMS outputs: SelectionFailed\"\n";
    let last_off = "sophia_qemu_unplug schema=1 status=sent action=off target=card0-Virtual-2\n";
    let first_on = "sophia_qemu_unplug schema=1 status=sent action=on target=card0-Virtual-1\n";
    let log = all_return();
    let cases = [
        (log.replace(zero, ""), "no zero-connected observation"),
        (
            log.replace(zero, "")
                .replace(last_off, &format!("{zero}{last_off}")),
            "no zero-connected observation",
        ),
        (
            log.replace(zero, "")
                .replace(first_on, &format!("{first_on}{zero}")),
            "no zero-connected observation",
        ),
        (
            log.replace(unavailable, "")
                .replace(last_off, &format!("{unavailable}{last_off}")),
            "no unavailable topology",
        ),
        (
            log.replace(unavailable, "")
                .replace(first_on, &format!("{first_on}{unavailable}")),
            "no unavailable topology",
        ),
        (
            log.replace("status=unavailable", "status=published"),
            "no unavailable topology",
        ),
        (
            log.replace("connected=0 sample", "connected=1 sample"),
            "no zero-connected observation",
        ),
        (
            log.replace("connectors=2 connected=0", "connectors=0 connected=0"),
            "inconsistent counts",
        ),
        (
            log.replace("connectors=2 connected=0", "connectors=1 connected=0"),
            "inconsistent counts",
        ),
        (
            log.replace("connectors=2 connected=0", "connectors=2 connected=3"),
            "inconsistent counts",
        ),
        (
            log.replace("connected=0 sample", "connected=unknown sample"),
            "no numeric connected",
        ),
        (
            log.replace("connectors=2 connected=0", "connectors=unknown connected=0"),
            "no numeric connectors",
        ),
        (
            log.replace("schema=1 status=connectors", "schema=2 status=connectors"),
            "unsupported schema",
        ),
        (
            log.replace("connected=0 sample", "connected=0 connected=1 sample"),
            "repeated fields",
        ),
        (
            log.replace(
                "connectors=2 connected=0",
                "connectors=2 connectors=1 connected=0",
            ),
            "repeated fields",
        ),
        (
            log.replace(
                "schema=1 status=connectors",
                "schema=1 schema=2 status=connectors",
            ),
            "repeated fields",
        ),
    ];
    for (index, (changed, expected)) in cases.into_iter().enumerate() {
        assert_ne!(changed, log, "case {index} must change its witness");
        let error = verify(&changed, Mode::AllReturn).unwrap_err();
        assert!(error.contains(expected), "case {index}: {error}");
    }
    // Enumeration includes disconnected connectors, and repeated samples can
    // straddle the host's logging of a completed removal.
    let repeated = log.replace(last_off, &format!("{zero}{last_off}"));
    verify(&repeated, Mode::AllReturn).unwrap();
    verify(
        &log.replace("connectors=2 connected=0", "connectors=3 connected=0"),
        Mode::AllReturn,
    )
    .unwrap();
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
sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=26 layer=0 source_stage=renderer_image target=400x300_100_100 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229
sophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=2 frame=3
sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding
";
    let after = "\
sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=9 scene_generation=40 target_generation=1
sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=40 layer=0 source_stage=renderer_image target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229
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
        (log.replace("target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229", "target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=778"), "not drawn from a retained image"),
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

/// REVIEW-CODEX-07: a replacement reuses frame and generation identities.
/// Before the loss frame 4 at generation 1 was queued, drawn and retired;
/// the replacement owner queues and draws its bootstrap frame 1 at
/// generation 1 again, then `post` says what presented it, if anything.
fn replacement_case(post: &str) -> String {
    let start = START
        .replace(
            "status=running mode=MODE",
            "status=running mode=MODE wm=false client=dri3",
        )
        .replace("MODE", "one");
    let before = "\
sophia_live_session_present schema=2 status=retired transaction=26 surface=2097153 source=400x300 target=400x300_100_100 clip=400x300_100_100 unit_scale=true ust=1 msc=1
sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=4 scene_generation=1 target_generation=1
sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=1 layer=0 source_stage=renderer_image target=400x300_100_100 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229
sophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=2 frame=4
sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding
";
    let loss = "\
sophia_qemu_unplug schema=1 status=sent action=off target=Console_1
sophia_live_output_topology schema=1 status=quiesced transition=1 outcome=drained abandoned_scanouts=0
sophia_live_native_owner schema=1 status=closed epoch=1 reason=topology_rebuild settled=true
sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=1 scene_generation=1 target_generation=1
sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=1 layer=0 source_stage=renderer_image target=400x300_100_100 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229
";
    let settled = "\
sophia_live_output_topology schema=1 status=published transition=1 topology_epoch=2 generation=2 outputs=1 changed=true restored_images=1 policy_required=false input=quarantined
sophia_live_output_topology schema=2 status=presentation_timed_out transition=1 retirements=0 presentation_baseline=0 timeout_msec=2000 input=enabled
";
    let post = post.replace("PUBLISHED", settled);
    format!("{start}{before}{loss}{post}{}{END}", uevents(1, 0, 0))
}

#[test]
fn only_the_replacement_owners_own_presentation_counts() {
    // The bootstrap frame's synchronous first modeset, then publication.
    let bootstrap = "sophia_live_head_bootstrap schema=1 status=worker_composed output=1 head=1 frame=1 scene_generation=1 target_generation=1 mapping=fit exports=1\nsophia_live_native_owner schema=1 status=opened epoch=2 reason=topology_rebuild\nPUBLISHED";
    let summary = verify(&replacement_case(bootstrap), Mode::One).unwrap();
    assert!(
        summary
            .last()
            .unwrap()
            .contains("status=static_content_retained"),
        "{summary:?}"
    );
    // A page flip of the same frame after the region, in the same owner.
    let flipped = "PUBLISHED\nsophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=1 frame=1\n";
    assert!(verify(&replacement_case(flipped), Mode::One).is_ok());
}

#[test]
fn a_borrowed_or_misordered_presentation_is_refused() {
    let cases = [
        // Only the old owner's frame 4 retired; it matches the region's
        // output, head and generation but belongs to the previous owner.
        "PUBLISHED",
        // Another frame of the replacement retired.
        "PUBLISHED\nsophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=3 frame=2\n",
        // A publication before the bootstrap composed says nothing about that
        // frame's modeset; only one after it does.
        "PUBLISHED\nsophia_live_head_bootstrap schema=1 status=worker_composed output=1 head=1 frame=1 scene_generation=1 target_generation=1 mapping=fit exports=1\n",
    ];
    for (index, post) in cases.into_iter().enumerate() {
        let error = verify(&replacement_case(post), Mode::One).unwrap_err();
        assert!(
            error.contains("not drawn from a retained image"),
            "case {index}: {error}"
        );
    }
    // A retirement of frame 1 before the region, in the same owner, is not
    // the region's presentation.
    let early = replacement_case("PUBLISHED").replace(
        "sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=1 scene_generation=1 target_generation=1\n",
        "sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=1 scene_generation=1 target_generation=1\nsophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=1 frame=1\n",
    );
    assert!(
        verify(&early, Mode::One)
            .unwrap_err()
            .contains("not drawn from a retained image")
    );
}

/// REVIEW-CODEX-14: the reference is the client's frame as first presented.
/// A region of the window before the removal that shows other pixels fails
/// the run as an unstable baseline, and says whether the content was then
/// seen preserved after the loss.
#[test]
fn a_baseline_that_changes_before_the_removal_fails_and_says_what_followed() {
    let log = static_client();
    let barrier = "sophia_qemu_unplug schema=1 status=static_barrier";
    let second = |checksum: &str, nonzero: &str| {
        format!(
            "sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=4 scene_generation=1 target_generation=1\n\
             sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=1 layer=0 source_stage=renderer_image target=400x300_100_100 region_pixels=120000 nonzero_rgb_pixels={nonzero} checksum={checksum}\n\
             sophia_live_native_head_page_flip schema=2 status=retired output=1 head=1 submission=3 frame=4\n{barrier}"
        )
    };
    let lost = "target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229";
    let cases = [
        // Good, then black, then good again after the loss.
        (
            log.replace(barrier, &second("555", "0")),
            "checksum=555 nonzero_rgb_pixels=0, not the first presented checksum=15913682524319544229; preserved_content=observed",
        ),
        // Good, then other non-black pixels.
        (
            log.replace(barrier, &second("779", "120000")),
            "checksum=779 nonzero_rgb_pixels=120000, not the first presented checksum=15913682524319544229; preserved_content=observed",
        ),
        // Good, then black, and nothing preserved after the loss.
        (
            log.replace(barrier, &second("555", "0")).replace(
                lost,
                "target=400x300_0_0 region_pixels=120000 nonzero_rgb_pixels=0 checksum=555",
            ),
            "checksum=555 nonzero_rgb_pixels=0, not the first presented checksum=15913682524319544229; preserved_content=not_observed",
        ),
    ];
    for (index, (log, expected)) in cases.into_iter().enumerate() {
        let error = verify(&log, Mode::OneReturn).unwrap_err();
        assert!(
            error.contains("unstable_baseline") && error.contains(expected),
            "case {index}: {error}"
        );
    }
}

#[test]
fn the_reference_is_the_first_presented_frame_and_must_be_the_clients() {
    let log = static_client();
    let frame = "region_pixels=120000 nonzero_rgb_pixels=120000 checksum=15913682524319544229";
    let refused = |log: &str| verify(log, Mode::OneReturn).unwrap_err();
    // Black from the first presented region on, before and after: no frame
    // of the client was ever shown, so nothing is preserved.
    assert!(refused(&log.replace(frame, "region_pixels=120000 nonzero_rgb_pixels=0 checksum=555")).contains(
        "not the client's frame: region_pixels=120000 checksum=555, expected region_pixels=120000 checksum=15913682524319544229"
    ));
    // Stable, fully non-black and still not the probe's pattern
    // (REVIEW-CODEX-15 control 1).
    assert!(
        refused(&log.replace(
            frame,
            "region_pixels=120000 nonzero_rgb_pixels=120000 checksum=999"
        ))
        .contains("not the client's frame: region_pixels=120000 checksum=999")
    );
    // A 400x300 target that read no pixels (REVIEW-CODEX-15 control 2).
    assert!(refused(&log.replace(frame, "region_pixels=0 nonzero_rgb_pixels=0 checksum=15913682524319544229")).contains(
        "not the client's frame: region_pixels=0 checksum=15913682524319544229, expected region_pixels=120000"
    ));
    // Only partly drawn.
    assert!(
        refused(&log.replacen(
            frame,
            "region_pixels=120000 nonzero_rgb_pixels=60000 checksum=556",
            1
        ))
        .contains("not the client's frame: region_pixels=120000 checksum=556")
    );
    // The only region before the removal belongs to a frame that never
    // retired: there is no presented reference.
    let unpresented = log.replacen("submission=2 frame=3", "submission=2 frame=8", 1);
    assert!(
        verify(&unpresented, Mode::OneReturn)
            .unwrap_err()
            .contains("no presented region of its window before the removal")
    );
    // An unpresented region with other pixels before the presented one is
    // still a contradiction: the reference is not chosen to pass.
    let earlier = log.replacen(
        "sophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=3",
        "sophia_native_composition_region_frame schema=1 status=read output=1 head=1 scene_generation=25 layer=0 source_stage=renderer_image target=400x300_100_100 region_pixels=120000 nonzero_rgb_pixels=0 checksum=555\nsophia_live_head_composition_queue schema=1 status=queued output=1 head=1 frame=3",
        1,
    );
    assert!(verify(&earlier, Mode::OneReturn).unwrap_err().contains(
        "unstable_baseline: a region of its window before the removal shows checksum=555"
    ));
}

/// The expected frame is derived from the probe's fill and the trace's
/// readback, never taken from a run. These values were computed separately
/// from the four quadrant colours, opaque alpha and bottom-up RGBA order; the
/// same pattern read top-down would hash to 8975981465688749989.
#[test]
fn the_expected_frame_is_the_probes_pattern_read_bottom_up() {
    assert_eq!(probe_frame_checksum(400, 300), 15913682524319544229);
    assert_ne!(probe_frame_checksum(400, 300), 8975981465688749989);
    assert_eq!(probe_frame_checksum(2, 2), 2471897560895143411);
    assert_eq!(probe_frame_checksum(3, 1), 8974295684228261135);
}

#[test]
fn out_of_probe_dimensions_are_refused_before_computing_a_reference() {
    for size in ["0x300", "400x0", "4097x1", "1x4097", "4294967295x1"] {
        let log = static_client().replace("source=400x300", &format!("source={size}"));
        let error = verify(&log, Mode::OneReturn).unwrap_err();
        assert!(
            error.contains("outside the probe's 1..=4096 dimensions"),
            "{size}: {error}"
        );
    }
}

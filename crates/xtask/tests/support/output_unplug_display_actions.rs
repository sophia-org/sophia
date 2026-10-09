use super::{Mode, all_return, moved, one_return, static_client, verify};

const OFF_START: &str = "sophia_qemu_unplug schema=1 status=sending action=off target=Console_1";
const OFF_END: &str = "sophia_qemu_unplug schema=1 status=sent action=off target=Console_1";
const ON_START: &str = "sophia_qemu_unplug schema=1 status=sending action=on target=Console_1";
const ON_END: &str = "sophia_qemu_unplug schema=1 status=sent action=on target=Console_1";
const BOUNDED: &str = "sophia_live_session schema=7 status=bounded_complete";

#[test]
fn topology_and_static_pixels_can_arrive_before_command_completion() {
    let log = moved(&static_client(), OFF_END, ON_START);
    let log = moved(&log, ON_END, BOUNDED);
    verify(&log, Mode::OneReturn).unwrap();
}

#[test]
fn zero_connected_and_unavailable_can_arrive_before_last_off_completes() {
    let log = moved(
        &all_return(),
        OFF_END,
        "sophia_qemu_unplug schema=1 status=sending action=on target=Console_0",
    );
    verify(&log, Mode::AllReturn).unwrap();
}

#[test]
fn display_attempts_require_matching_ordered_completions() {
    let log = one_return();
    let cases = [
        log.replace(&format!("{OFF_START}\n"), ""),
        log.replace(&format!("{OFF_END}\n"), ""),
        log.replace(&format!("{ON_START}\n"), ""),
        log.replace(&format!("{ON_END}\n"), ""),
        log.replace(OFF_START, &format!("{OFF_START}\n{OFF_START}")),
        log.replace(OFF_END, &format!("{OFF_END}\n{OFF_END}")),
        moved(&log, OFF_END, OFF_START),
        moved(&log, OFF_END, ON_END),
        log.replace(OFF_END, &OFF_END.replace("Console_1", "Console_0")),
        log.replace(ON_START, &ON_START.replace("action=on", "action=off")),
        log.replace("target=Console_1", "target=Console_2"),
        log.replace("target=Console_1", "target=Console_01"),
        log.replace(OFF_START, &format!("{OFF_START} target=Console_0")),
        log.replace(OFF_END, &format!("{OFF_END} result=failed")),
        log.replace(OFF_START, &OFF_START.replace("schema=1", "schema=2")),
        log.replace(
            ON_END,
            &format!("{ON_END}\nsophia_qemu_unplug schema=1 status=failed reason=head_enable"),
        ),
    ];
    for (index, changed) in cases.iter().enumerate() {
        assert_ne!(*changed, log, "case {index} must change evidence");
        assert!(verify(changed, Mode::OneReturn).is_err(), "case {index}");
    }
}

#[test]
fn completion_after_bounded_end_is_refused_even_with_returned_pixels() {
    let log = moved(
        &static_client(),
        ON_END,
        "sophia_qemu_guest schema=1 status=complete",
    );
    assert!(
        verify(&log, Mode::OneReturn)
            .unwrap_err()
            .contains("bounded completion")
    );
}

#[test]
fn all_return_requires_each_head_to_be_removed_and_returned_once() {
    let log = all_return();
    for changed in [
        log.replace("target=Console_0", "target=Console_1"),
        log.lines()
            .filter(|line| !line.contains("target=Console_0"))
            .collect::<Vec<_>>()
            .join("\n"),
        log.lines()
            .filter(|line| !line.contains("action=on target=Console_0"))
            .collect::<Vec<_>>()
            .join("\n"),
    ] {
        assert!(verify(&changed, Mode::AllReturn).is_err());
    }
}

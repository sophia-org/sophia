use super::{Mode, all_return, input_return, one, one_return, static_client, verify};

/// The harness's own endpoint and cleanup records (tools/qemu_unplug_endpoint.sh).
const BUS_ENDED: &str = "sophia_qemu_unplug schema=1 status=display_bus_ended display_bus_exit=143 display_bus_signal=TERM";
const BUS_UNREAPED: &str = "sophia_qemu_unplug schema=1 status=display_bus_ended display_bus_exit=unreaped display_bus_signal=KILL display_bus_pid=4242";
const QEMU_UNREAPED: &str = "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=unreaped logger_exit=0 qemu_signal=KILL logger_signal=none qemu_pid=4243";
const LOGGER_UNREAPED: &str = "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=0 logger_exit=unreaped qemu_signal=none logger_signal=KILL logger_pid=4244";
const STOPPED: &str = "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=143 logger_exit=0 qemu_signal=TERM logger_signal=none";
const EXITED: &str = "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0";

fn runs() -> [(String, Mode); 5] {
    [
        (one(), Mode::One),
        (one_return(), Mode::OneReturn),
        (all_return(), Mode::AllReturn),
        (input_return(), Mode::InputReturn),
        (static_client(), Mode::OneReturn),
    ]
}

#[test]
fn a_reaped_display_bus_after_a_clean_exit_still_passes() {
    for (log, mode) in runs() {
        verify(&log, mode).unwrap();
        verify(&format!("{log}{BUS_ENDED}\n"), mode).unwrap();
    }
}

#[test]
fn a_recorded_unreaped_process_or_stopped_guest_refuses_every_mode_wherever_it_stands() {
    for (log, mode) in runs() {
        let mut cases = Vec::new();
        for record in [BUS_UNREAPED, QEMU_UNREAPED, LOGGER_UNREAPED, STOPPED] {
            cases.push(format!("{log}{record}\n"));
            cases.push(format!("{record}\n{log}"));
        }
        // A conflicting duplicate on the clean exit itself, in either order.
        cases.push(log.replace(EXITED, &format!("{EXITED} qemu_exit=unreaped")));
        cases.push(log.replace(
            EXITED,
            &EXITED.replace("qemu_exit=0", "qemu_exit=unreaped qemu_exit=0"),
        ));
        // A pid kept for the runner, with an exit that reads as ordinary.
        cases.push(format!("{log}{BUS_ENDED} display_bus_pid=4242\n"));
        for (index, changed) in cases.into_iter().enumerate() {
            let error = verify(&changed, mode).unwrap_err();
            assert!(
                error.starts_with("host endpoint refused: ") && error.contains("the host "),
                "{mode:?} case {index}: {error}"
            );
        }
    }
}

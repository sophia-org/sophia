use super::*;

use crate::emergency_input::{EmergencyChordAction, EmergencyChordState};

const INPUT_GUARD_GRACE_MSEC: u64 = 250;

pub(crate) fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let automatic_arming = match arg_value(args, "--arming").as_deref() {
        None | Some("manual") => false,
        Some("automatic") => true,
        Some(_) => return Err("input guard --arming must be manual or automatic".into()),
    };
    let input_devices = arg_value(args, "--input-devices")
        .unwrap_or_default()
        .split(',')
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let input_seat = arg_value(args, "--input-seat");
    if input_devices.len() > 16 || input_devices.iter().any(|path| !path.is_absolute()) {
        return Err("sophia session input-guard accepts 1-16 absolute input device paths".into());
    }
    if input_devices.is_empty() == input_seat.is_none() {
        return Err(
            "sophia session input-guard requires exactly one of --input-seat or --input-devices"
                .into(),
        );
    }
    let armed_file = required_absolute_path(args, "--armed-file")?;
    let triggered_file = required_absolute_path(args, "--triggered-file")?;
    let owner_pid = arg_value(args, "--owner-pid")
        .ok_or("sophia session input-guard requires --owner-pid=PID")?
        .parse::<u32>()
        .map_err(|error| format!("invalid input guard owner PID: {error}"))?;
    if owner_pid == 0 {
        return Err("input guard owner PID must be nonzero".into());
    }

    let devices =
        sophia_backend_live::NativeLibinputDeviceMap::new(SeatId::from_raw(SESSION_SEAT_RAW))
            .with_keyboard_device(DeviceId::from_raw(SESSION_KEYBOARD_DEVICE_RAW));
    let mut poller = if let Some(seat_name) = input_seat.as_deref() {
        sophia_backend_live::open_native_libinput_udev_poller(seat_name, devices, 64)?
    } else {
        sophia_backend_live::open_native_libinput_path_poller(&input_devices, devices, 64)?
    };
    let policy = poller.reader().policy_report();
    // Path-based polling can open a pointer alone. It cannot protect a session
    // that is about to take over the display, even if no handshake is required.
    if policy.keyboards == 0 {
        return Err("input guard requires an opened keyboard".into());
    }
    let mut chord = if automatic_arming {
        EmergencyChordState::armed()
    } else {
        EmergencyChordState::awaiting_arm()
    };
    crate::session_println!(
        "sophia_session_input_guard schema=2 status=ready source={} seat={} devices={} keyboards={}",
        if policy.udev_managed { "udev" } else { "paths" },
        input_seat.as_deref().unwrap_or("explicit"),
        policy.devices_added,
        policy.keyboards,
    );
    std::io::stdout().flush()?;
    if automatic_arming {
        std::fs::write(&armed_file, b"armed\n")?;
        crate::session_println!("sophia_session_input_guard schema=1 status=armed");
        std::io::stdout().flush()?;
    }

    loop {
        if !std::path::Path::new(&format!("/proc/{owner_pid}")).exists() {
            return Ok(());
        }
        for event in poller.poll_ready()? {
            let action = match event.kind {
                sophia_protocol::InputEventKind::Key { keycode, pressed } => {
                    chord.observe_at_device(event.device, keycode, pressed)
                }
                // A keyboard that leaves takes its chord keys with it. That
                // can finish arming; it can never trigger.
                sophia_protocol::InputEventKind::DeviceRemoved => chord.forget_device(event.device),
                _ => continue,
            };
            match action {
                EmergencyChordAction::None => {}
                EmergencyChordAction::Armed => {
                    std::fs::write(&armed_file, b"armed\n")?;
                    crate::session_println!("sophia_session_input_guard schema=1 status=armed");
                    std::io::stdout().flush()?;
                }
                EmergencyChordAction::Triggered => {
                    std::fs::write(&triggered_file, b"triggered\n")?;
                    crate::session_println!("sophia_session_input_guard schema=1 status=triggered");
                    std::io::stdout().flush()?;
                    std::thread::sleep(Duration::from_millis(INPUT_GUARD_GRACE_MSEC));
                    return Ok(());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn required_absolute_path(
    args: &[String],
    name: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(
        arg_value(args, name)
            .ok_or_else(|| format!("sophia session input-guard requires {name}=PATH"))?,
    );
    if !path.is_absolute() {
        return Err(format!("{name} must be an absolute path").into());
    }
    Ok(path)
}

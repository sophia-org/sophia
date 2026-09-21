//! Optional real Qt consumer check. The broker ingress stands in for Engine
//! routing; this does not exercise physical input or compositor hit testing.
//! Compile the receiver using Qt6Widgets, xcb and xcb-xinput pkg-config flags:
//! `c++ -std=c++17 tools/probes/qt_wheel.cpp -o /tmp/qt-wheel $(pkg-config --cflags --libs Qt6Widgets xcb xcb-xinput)`
//! Then run this ignored test with `SOPHIA_QT_WHEEL_PROBE=/tmp/qt-wheel`.
#![cfg(unix)]

use sophia_protocol::{DeviceId, InputEventKind, NamespaceId, Point, RoutedInputRequest, SeatId};
use sophia_x_authority::*;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::num::NonZeroUsize;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct RunningFrontend {
    child: Option<Child>,
    stop: SyncSender<XServerFrontendServiceCommand>,
    thread: Option<thread::JoinHandle<()>>,
    socket: PathBuf,
}
impl Drop for RunningFrontend {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self
            .stop
            .try_send(XServerFrontendServiceCommand::StopAccepting);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}

#[test]
#[ignore = "requires SOPHIA_QT_WHEEL_PROBE pointing to compiled tools/probes/qt_wheel.cpp"]
fn real_qt_receives_scroll_and_single_click_through_the_frontend_writer() {
    let executable = std::env::var_os("SOPHIA_QT_WHEEL_PROBE")
        .expect("compile tools/probes/qt_wheel.cpp and set SOPHIA_QT_WHEEL_PROBE");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let evidence = std::env::temp_dir().join(format!(
        "sophia-qt-wheel-delivery-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&evidence).unwrap();
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
    println!("evidence={}", evidence.display());
    let display = 60000 + std::process::id() % 10000;
    let socket = PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
    assert!(!socket.exists(), "private test display already exists");
    let (observations, observed) = mpsc::sync_channel(256);
    let (acks, _acked) = mpsc::sync_channel(16);
    let (delivery, delivered) = mpsc::channel();
    let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(32).unwrap(),
        acks,
        delivery,
    );
    let input = broker.routed_input_sender();
    let (stop, stopped) = mpsc::sync_channel(1);
    let config = XServerFrontendConfig::new(&socket, NamespaceId::from_raw(995)).unwrap();
    let server = thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(config, observations, broker, stopped).unwrap();
    });
    let mut running = RunningFrontend {
        child: None,
        stop,
        thread: Some(server),
        socket,
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while !running.socket.exists() {
        assert!(Instant::now() < deadline, "frontend did not bind");
        thread::sleep(Duration::from_millis(5));
    }
    let mut command = Command::new(executable);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("SOPHIA_")
            || name.starts_with("QT_")
            || name.starts_with("QML_")
            || matches!(
                name.as_ref(),
                "DISPLAY" | "XAUTHORITY" | "WAYLAND_DISPLAY" | "LD_PRELOAD"
            )
        {
            command.env_remove(key);
        }
    }
    command
        .env("DISPLAY", format!(":{display}"))
        .env("QT_QPA_PLATFORM", "xcb")
        .env("QT_STYLE_OVERRIDE", "Fusion")
        .env("XDG_RUNTIME_DIR", &evidence)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(evidence.join("stderr.log")).unwrap());
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    running.child = Some(child);
    let (lines, received_lines) = mpsc::channel();
    let capture = evidence.join("stdout.log");
    let reader = thread::spawn(move || {
        let mut log = File::create(capture).unwrap();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            writeln!(log, "{line}").unwrap();
            let _ = lines.send(line);
        }
    });
    // Wait for real Qt MouseMove handling before scrolling, so XI Enter has
    // established Qt's valuator baseline. First-axis-on-entry is a separate
    // ordering boundary; this exercises normal motion followed by scrolling.
    let events = [
        InputEventKind::PointerMotion,
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: 120,
        },
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: -120,
        },
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: -120,
        },
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: 120,
        },
        InputEventKind::PointerButton {
            button: 0x110,
            pressed: true,
        },
        InputEventKind::PointerButton {
            button: 0x110,
            pressed: false,
        },
    ];
    let mut described = std::collections::BTreeMap::new();
    let mut ready = None;
    let mut send_next = false;
    let mut sent = 0;
    let mut flushed = 0;
    let mut complete = false;
    while Instant::now() < deadline {
        for batch in observed.try_iter() {
            for presentation in batch.surface_presentations {
                if presentation.mapped {
                    described.insert(presentation.surface.index(), presentation.surface);
                }
            }
            for transaction in batch.transactions {
                described.insert(transaction.surface.index(), transaction.surface);
            }
        }
        for delivery in delivered.try_iter() {
            assert_eq!(delivery.outcome, XAuthorityInputDeliveryOutcome::Flushed);
            flushed += 1;
        }
        while let Ok(line) = received_lines.try_recv() {
            println!("{line}");
            if let Some(tail) = line.strip_prefix("qt_wheel ready xid=") {
                let xid = tail
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap();
                ready = Some(xid);
                // XID alone is not treated as identity: wait for the authority's
                // described generation in the observations above.
                send_next = true;
            } else if line.starts_with("qt_wheel wheel=")
                || line.starts_with("qt_wheel press=")
                || line.starts_with("qt_wheel positioned=")
            {
                send_next = true;
            } else if line.starts_with("qt_wheel complete ") {
                assert!(line.contains("result=pass"), "{line}");
                complete = true;
            } else if line.starts_with("qt_wheel timeout ") {
                panic!("{line}");
            }
        }
        if send_next && let Some(surface) = ready.and_then(|index| described.get(&index).copied()) {
            assert!(
                sent < events.len(),
                "unexpected duplicate Qt input acknowledgement"
            );
            input
                .send(XAuthorityRoutedInput {
                    request: RoutedInputRequest {
                        serial: sent as u64 + 1,
                        seat: SeatId::from_raw(1),
                        device: DeviceId::from_raw(1),
                        time_msec: 1000 + sent as u64,
                        target_surface: surface,
                        global_position: Point { x: 100.0, y: 100.0 },
                        local_position: Point { x: 100.0, y: 100.0 },
                        kind: events[sent],
                    },
                    route_lease: None,
                    delivery: Some(XAuthorityInputDeliveryId::from_raw(sent as u64 + 1)),
                    mode: XAuthorityRoutedInputMode::Deliver,
                    origin: XAuthorityRoutedInputOrigin::Physical,
                })
                .unwrap();
            sent += 1;
            send_next = false;
        }
        if let Some(status) = running.child.as_mut().unwrap().try_wait().unwrap() {
            assert!(
                status.success(),
                "Qt client failed: {status}; evidence={}",
                evidence.display()
            );
            assert!(complete, "Qt exited without a successful completion");
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        complete,
        "Qt input did not complete; evidence={}",
        evidence.display()
    );
    assert_eq!(sent, 7);
    assert_eq!(flushed, 7);
    reader.join().unwrap();
}

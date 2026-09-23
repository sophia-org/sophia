//! xterm as an oracle for pointer delivery.
//!
//! WHAT IT PROVES. With SGR any-event mouse tracking on, xterm writes one
//! escape sequence to its pty for every pointer event its text widget
//! receives, naming the button and the cell: `CSI < Cb ; col ; row M` for a
//! press or a motion, `m` for a release, with Cb 0/1/2 for buttons 1/2/3,
//! +32 for motion with that button held, and 35 for motion with none. This
//! driver starts a real xterm whose command turns tracking on and copies the
//! pty's input to a file, injects XTEST motion, press, drag and release at
//! chosen cells, and reads xterm's reports back. What arrives in the file is
//! what the frontend delivered to the widget, on which window, at which
//! position, with which button state -- with no pixel or protocol tracing.
//!
//! It would have caught t155 (buttons at the origin: wrong cell), t156
//! (events to the focus: no report at all), t162 (no motion during a drag:
//! no `32;` reports) and t158 (a release past the widget's edge going to the
//! shell: no `m`), which is why it exists.
//!
//! Stdout is one fixed pass line or `status=fail reason=<token>`, always with
//! exit 0: a finding is a verdict, and a non-zero exit is left to mean the
//! driver itself broke. Diagnostics, including the raw reports, go to stderr.
//!
//! WHICH TRACKING MODE. Button-event tracking (`CSI ?1002h`) reports presses,
//! releases and motion with a button held, and for that motion xterm relies
//! on its `<Btn1Motion>` translation -- the same Button1Motion selection an
//! ordinary xterm drags a selection with. Any-event tracking (`?1003h`) makes
//! xterm select PointerMotion itself, so it hears drag motion even from a
//! frontend that only delivers to PointerMotion selectors, which is exactly
//! the t162 defect; a run in that mode cannot see it. The default is 1002.
//!
//! Arguments: `--any-event` uses mode 1003 and also checks plain motion;
//! `--no-tracking` leaves tracking off, so nothing is ever reported and the
//! run must fail (a self-test mutation); `--overshoot` releases past xterm's
//! right edge, the t158 probe, red until t158 lands.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

const COLUMNS: u16 = 60;
const ROWS: u16 = 8;
const MOTION: u8 = 6;
const PRESS: u8 = 4;
const RELEASE: u8 = 5;
/// How long a report may take to reach the file after its event.
const REPORT_DEADLINE: Duration = Duration::from_secs(3);

struct Failure(&'static str);

impl From<x11rb::errors::ConnectionError> for Failure {
    fn from(_: x11rb::errors::ConnectionError) -> Self {
        Failure("connection")
    }
}
impl From<x11rb::errors::ReplyError> for Failure {
    fn from(_: x11rb::errors::ReplyError) -> Self {
        Failure("reply")
    }
}
impl From<x11rb::errors::ReplyOrIdError> for Failure {
    fn from(_: x11rb::errors::ReplyOrIdError) -> Self {
        Failure("reply_or_id")
    }
}

/// The xterm dies with the driver, whichever way it ends.
struct Terminal(Child);
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Frame {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

/// Where xterm's cells are, from its own size hints: the resize increment is
/// the cell, the base size is the borders around the grid.
struct Grid {
    frame: Frame,
    cell_width: u16,
    cell_height: u16,
    left: u16,
    top: u16,
}

impl Grid {
    /// The centre of a zero-based cell, in root coordinates.
    fn centre(&self, column: u16, row: u16) -> (i16, i16) {
        (
            self.frame.x.saturating_add(
                (self.left + column * self.cell_width + self.cell_width / 2) as i16,
            ),
            self.frame
                .y
                .saturating_add((self.top + row * self.cell_height + self.cell_height / 2) as i16),
        )
    }
}

/// One expected SGR report: `CSI < code ; column ; row` then `M` or `m`,
/// columns and rows one-based as xterm writes them.
fn report(code: u8, column: u16, row: u16, released: bool) -> String {
    format!(
        "\x1b[<{code};{};{}{}",
        column + 1,
        row + 1,
        if released { 'm' } else { 'M' }
    )
}

/// Which mouse tracking xterm is asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tracking {
    Off,
    /// `?1002h`: buttons, and motion while one is held.
    ButtonEvent,
    /// `?1003h`: every motion as well.
    AnyEvent,
}

fn xterm(log: &std::path::Path, tracking: Tracking) -> Result<Child, Failure> {
    // Raw mode, or the line discipline holds the reports until a newline
    // that never comes; then tracking on, SGR-encoded; then everything the
    // pty gets goes to the file as it arrives. `/usr/bin/sh`: the QEMU
    // initramfs has no `/bin`.
    let enable = match tracking {
        Tracking::Off => "",
        Tracking::ButtonEvent => "printf '\\033[?1002h\\033[?1006h'; ",
        Tracking::AnyEvent => "printf '\\033[?1003h\\033[?1006h'; ",
    };
    let command = format!("stty raw -echo; {enable}exec cat > '{}'", log.display());
    Command::new("xterm")
        .args([
            "-name",
            "sophia-pointer-oracle",
            "-fa",
            "DejaVu Sans Mono",
            "-fs",
            "14",
            "-geometry",
            &format!("{COLUMNS}x{ROWS}+40+40"),
            "-e",
            "/usr/bin/sh",
            "-c",
            &command,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure("xterm_spawn"))
}

/// The toplevel whose WM_CLASS instance is `name`, waited for.
fn find_toplevel(conn: &RustConnection, root: Window, name: &str) -> Result<Window, Failure> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        for child in conn.query_tree(root)?.reply()?.children {
            let class = conn
                .get_property(false, child, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?
                .reply()?;
            if class.value.split(|byte| *byte == 0).next() == Some(name.as_bytes()) {
                return Ok(child);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(Failure("xterm_window_absent"))
}

fn frame(conn: &RustConnection, root: Window, window: Window) -> Result<Frame, Failure> {
    let geometry = conn.get_geometry(window)?.reply()?;
    let origin = conn.translate_coordinates(window, root, 0, 0)?.reply()?;
    Ok(Frame {
        x: origin.dst_x,
        y: origin.dst_y,
        width: geometry.width,
        height: geometry.height,
    })
}

fn within(conn: &RustConnection, mut window: Window, ancestor: Window) -> Result<bool, Failure> {
    while window != 0 {
        if window == ancestor {
            return Ok(true);
        }
        let tree = conn.query_tree(window)?.reply()?;
        if tree.parent == tree.root || tree.parent == 0 {
            return Ok(false);
        }
        window = tree.parent;
    }
    Ok(false)
}

fn fake(
    conn: &RustConnection,
    root: Window,
    kind: u8,
    detail: u8,
    x: i16,
    y: i16,
) -> Result<(), Failure> {
    conn.xtest_fake_input(kind, detail, 0, root, x, y, 0)?;
    // The FakeInput barrier: the next request is not read until this one is
    // processed, so the round trip orders the events.
    conn.get_input_focus()?.reply()?;
    Ok(())
}

/// Wait until the pointer resolves to the window, then read its grid.
fn settle(conn: &RustConnection, root: Window, window: Window) -> Result<Grid, Failure> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let placed = frame(conn, root, window)?;
        let (x, y) = (placed.x.saturating_add(8), placed.y.saturating_add(8));
        fake(conn, root, MOTION, 0, x, y)?;
        let child = conn.query_pointer(root)?.reply()?.child;
        if child != 0 && within(conn, child, window)? {
            break;
        }
        if Instant::now() >= deadline {
            return Err(Failure("window_never_routable"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // A window manager may still be placing it; take the frame it has now.
    let frame = frame(conn, root, window)?;
    let hints = x11rb::properties::WmSizeHints::get_normal_hints(conn, window)
        .ok()
        .and_then(|cookie| cookie.reply().ok().flatten())
        .and_then(|hints| {
            let (width, height) = hints.size_increment?;
            let (base_width, base_height) = hints.base_size.unwrap_or((0, 0));
            Some((
                u16::try_from(width).ok()?,
                u16::try_from(height).ok()?,
                u16::try_from(base_width / 2).unwrap_or(0),
                u16::try_from(base_height / 2).unwrap_or(0),
            ))
        });
    let Some((cell_width, cell_height, left, top)) = hints.filter(|(w, h, ..)| *w > 0 && *h > 0)
    else {
        return Err(Failure("no_size_hints"));
    };
    Ok(Grid {
        frame,
        cell_width,
        cell_height,
        left,
        top,
    })
}

/// xterm's reports so far, as bytes.
fn reports(log: &std::path::Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Ok(mut file) = std::fs::File::open(log) {
        let _ = file.read_to_end(&mut bytes);
    }
    bytes
}

/// Wait for `expected` to appear in the log after `from`, returning the
/// offset just past it, so reports are matched in order.
fn expect(log: &std::path::Path, from: usize, expected: &str) -> Option<usize> {
    let deadline = Instant::now() + REPORT_DEADLINE;
    loop {
        let bytes = reports(log);
        if bytes.len() > from
            && let Some(at) = bytes[from..]
                .windows(expected.len())
                .position(|window| window == expected.as_bytes())
        {
            return Some(from + at + expected.len());
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn escaped(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            0x1b => "\\e".to_owned(),
            0x20..=0x7e => (*byte as char).to_string(),
            other => format!("\\x{other:02x}"),
        })
        .collect()
}

fn run(tracking: Tracking, overshoot: bool) -> Result<String, Failure> {
    let (conn, screen_index) = x11rb::connect(None).map_err(|_| Failure("connect"))?;
    let root = conn.setup().roots[screen_index].root;
    let log = std::env::temp_dir().join(format!(
        "sophia-xterm-pointer-oracle-{}.log",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&log);
    let _terminal = Terminal(xterm(&log, tracking)?);
    let window = find_toplevel(&conn, root, "sophia-pointer-oracle")?;
    let grid = settle(&conn, root, window)?;
    eprintln!(
        "xterm_pointer_oracle: mode={tracking:?} frame={}x{}+{}+{} cell={}x{} margins={},{}",
        grid.frame.width,
        grid.frame.height,
        grid.frame.x,
        grid.frame.y,
        grid.cell_width,
        grid.cell_height,
        grid.left,
        grid.top
    );
    let row = 2;
    let dump = |reason: &'static str| {
        eprintln!(
            "xterm_pointer_oracle: reports={:?}",
            escaped(&reports(&log))
        );
        Failure(reason)
    };
    let mut at = 0;

    // The shell inside xterm turns tracking on asynchronously, so the first
    // report is retried for a few seconds before the run is called silent.
    // In any-event mode the probe is a motion (code 35); in button-event mode
    // it is button 3 down and up at a cell nothing else uses (codes 2, then
    // 2 with `m`).
    let (x, y) = grid.centre(10, row);
    let started = Instant::now();
    loop {
        let found = if tracking == Tracking::AnyEvent {
            fake(&conn, root, MOTION, 0, x, y)?;
            expect(&log, at, &report(35, 10, row, false))
        } else {
            let (px, py) = grid.centre(40, row);
            fake(&conn, root, MOTION, 0, px, py)?;
            fake(&conn, root, PRESS, 3, px, py)?;
            let pressed = expect(&log, at, &report(2, 40, row, false));
            fake(&conn, root, RELEASE, 3, px, py)?;
            pressed.and_then(|next| expect(&log, next, &report(2, 40, row, true)))
        };
        if let Some(next) = found {
            at = next;
            break;
        }
        if started.elapsed() > Duration::from_secs(8) {
            return Err(dump("tracking_never_reported"));
        }
        // Whatever half-reports arrived are not this attempt's.
        std::thread::sleep(Duration::from_millis(300));
        at = reports(&log).len();
        // Leave and return, so the next probe is a cell change xterm reports.
        let (ax, ay) = grid.centre(11, row);
        fake(&conn, root, MOTION, 0, ax, ay)?;
    }

    // Button 1 down at the drag's first cell: code 0.
    fake(&conn, root, MOTION, 0, x, y)?;
    if tracking == Tracking::AnyEvent {
        at = expect(&log, at, &report(35, 10, row, false))
            .ok_or_else(|| dump("no_motion_report"))?;
    }
    fake(&conn, root, PRESS, 1, x, y)?;
    at = expect(&log, at, &report(0, 10, row, false)).ok_or_else(|| dump("no_press_report"))?;

    // Drag: motion with button 1 held, code 32, one report per cell. This is
    // the report a frontend that delivers drag motion to the shell rather
    // than the widget never produces (t162).
    for column in [14u16, 18, 22] {
        let (dx, dy) = grid.centre(column, row);
        fake(&conn, root, MOTION, 0, dx, dy)?;
        at = expect(&log, at, &report(32, column, row, false))
            .ok_or_else(|| dump("no_drag_report"))?;
    }

    // Release: code 0 with `m`. With --overshoot it is past the right edge,
    // and xterm reports it, at its last column, only if the release reaches
    // the widget at all (t158).
    let (rx, ry) = if overshoot {
        (
            grid.frame
                .x
                .saturating_add(grid.frame.width as i16)
                .saturating_add(24),
            y,
        )
    } else {
        grid.centre(22, row)
    };
    if overshoot {
        // A button carries no position of its own: move there first, as a
        // hand does. Past the widget's edge the motion is not xterm's to
        // report, so nothing is expected of it.
        fake(&conn, root, MOTION, 0, rx, ry)?;
    }
    fake(&conn, root, RELEASE, 1, rx, ry)?;
    at = if overshoot {
        // Any release of button 1, wherever xterm clamped it.
        expect(&log, at, "\x1b[<0;").and_then(|next| {
            let bytes = reports(&log);
            let end = bytes[next..]
                .iter()
                .position(|byte| *byte == b'm' || *byte == b'M')?;
            (bytes[next + end] == b'm').then_some(next + end + 1)
        })
    } else {
        expect(&log, at, &report(0, 22, row, true))
    }
    .ok_or_else(|| dump("release_lost"))?;

    // Buttons 2 and 3, down and up, at a fresh cell: codes 1 and 2.
    let (bx, by) = grid.centre(30, row);
    fake(&conn, root, MOTION, 0, bx, by)?;
    if tracking == Tracking::AnyEvent {
        at = expect(&log, at, &report(35, 30, row, false))
            .ok_or_else(|| dump("no_motion_report_after_release"))?;
    }
    for (button, code) in [(2u8, 1u8), (3, 2)] {
        fake(&conn, root, PRESS, button, bx, by)?;
        at = expect(&log, at, &report(code, 30, row, false))
            .ok_or_else(|| dump("no_button_report"))?;
        fake(&conn, root, RELEASE, button, bx, by)?;
        at = expect(&log, at, &report(code, 30, row, true))
            .ok_or_else(|| dump("no_button_release_report"))?;
    }

    eprintln!(
        "xterm_pointer_oracle: reports={:?} bytes={at}",
        escaped(&reports(&log))
    );
    let _ = std::fs::remove_file(&log);
    Ok("sophia_xterm_pointer_oracle schema=1 status=pass motion=true press=true drag=true release=true buttons=3".to_owned())
}

fn main() {
    let mut tracking = Tracking::ButtonEvent;
    let mut overshoot = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--no-tracking" => tracking = Tracking::Off,
            "--any-event" => tracking = Tracking::AnyEvent,
            "--overshoot" => overshoot = true,
            _ => {}
        }
    }
    match run(tracking, overshoot) {
        Ok(record) => print!("{record}"),
        Err(Failure(reason)) => {
            eprintln!("xterm_pointer_oracle: status=fail reason={reason}");
            print!("sophia_xterm_pointer_oracle schema=1 status=fail reason={reason}");
        }
    }
}

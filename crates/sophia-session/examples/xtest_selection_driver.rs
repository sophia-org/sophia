//! Does a real xterm take PRIMARY when a drag is driven into it by XTEST, and
//! does a middle-click in a second xterm ask for it back?
//!
//! Run by the session as its `--client`, so `DISPLAY` and `XAUTHORITY` are
//! inherited and no cookie is hunted for. It spawns two xterms itself, so it
//! knows exactly what each shows, then drives the gesture with XTEST
//! `FakeInput` -- motion included, since `xdotool mousemove` is `WarpPointer`
//! and left every earlier attempt clicking on the root. Before any button the
//! pointer's position is read back and required to be over the intended
//! window; that check is what every prior attempt lacked, and without it a
//! silent selection cannot be told from a gesture that never landed.
//!
//! Output is one record with no trailing newline, because the session compares
//! the client's stdout byte for byte. The exit status is 0 for a pass and for a
//! finding alike, so the session completes and writes its records:
//!
//!     sophia_xtest_selection schema=1 status=pass owner_in_a=true matched=true pointer_in_a=true pointer_in_b=true
//!     sophia_xtest_selection schema=1 status=fail reason=<token>
//!
//! The session's own `sophia_live_selection` record carries the other half of
//! the proof: `owner_changes` is SetSelectionOwner arriving on the wire, and
//! `conversions` is xterm B's ConvertSelection after the middle-click, which
//! this client cannot observe and does not claim.
//!
//! Arguments: `--row=N` drags row N of xterm A instead of the text row, which
//! selects blank cells xterm trims to nothing and must fail; `--no-paste` skips
//! the middle-click so the session's `conversions` must stay zero. Both exist
//! so the gate can be shown to go red. `--overshoot` releases past A's right
//! edge: a probe for the frontend's implicit grab (t158), red until it lands.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, Window, WindowClass,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

const MARKER: &str = "SOPHIA_T124_ONE_TWO_THREE";
const ROWS: u16 = 8;
const MOTION: u8 = 6;
const PRESS: u8 = 4;
const RELEASE: u8 = 5;

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

/// The xterms die with the driver, whichever way it ends.
struct Terminals(Vec<Child>);
impl Drop for Terminals {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn xterm(name: &str, geometry: &str, command: &str) -> Result<Child, Failure> {
    // A diagnostic hook: `SOPHIA_T124_XTERM_A` names a program to run in place
    // of xterm for window A, with the same arguments -- a protocol tracer, so
    // what xterm receives and sends can be read. Absent, it is plain xterm.
    let program = match std::env::var("SOPHIA_T124_XTERM_A") {
        Ok(program) if name == "sophia-t124-a" => program,
        _ => "xterm".to_owned(),
    };
    // `/usr/bin/sh`, not `/bin/sh`: the QEMU initramfs and a sandbox may have
    // no `/bin`.
    Command::new(program)
        .args([
            "-name",
            name,
            "-fa",
            "DejaVu Sans Mono",
            "-fs",
            "14",
            "-geometry",
            geometry,
            "-e",
            "/usr/bin/sh",
            "-c",
            command,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure("xterm_spawn"))
}

/// The toplevel whose WM_CLASS instance is `name`, waited for. With no window
/// manager a toplevel is a direct child of the root.
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

struct Frame {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
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

/// Whether `window` is `ancestor` or lies beneath it.
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
    // A round trip after each event honours the FakeInput barrier rather than
    // racing it: the next request is not read until this one is processed.
    conn.get_input_focus()?.reply()?;
    Ok(())
}

/// Move the pointer with XTEST and confirm where it landed.
fn aim(conn: &RustConnection, root: Window, target: Window, x: i16, y: i16) -> Result<(), Failure> {
    fake(conn, root, MOTION, 0, x, y)?;
    let pointer = conn.query_pointer(root)?.reply()?;
    if pointer.child == 0 || !within(conn, pointer.child, target)? {
        // Which of the three it was: the motion did not move the pointer, it
        // moved and something else is under it, or the target is not where
        // the X tree says it is.
        let placed = frame(conn, root, target)?;
        eprintln!(
            "xtest_selection_driver: aimed=({x},{y}) pointer=({},{}) child=0x{:x} target=0x{target:x} target_frame={}x{}+{}+{}",
            pointer.root_x,
            pointer.root_y,
            pointer.child,
            placed.width,
            placed.height,
            placed.x,
            placed.y
        );
        // Then where the server does think the target is: sweep the root and
        // report the box in which the pointer resolves to it.
        let screen = conn.get_geometry(root)?.reply()?;
        let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
        let mut seen: Vec<(Window, i16, i16, i16, i16)> = Vec::new();
        let mut sy = 4i16;
        while (sy as u16) < screen.height {
            let mut sx = 4i16;
            while (sx as u16) < screen.width {
                fake(conn, root, MOTION, 0, sx, sy)?;
                let at = conn.query_pointer(root)?.reply()?;
                if at.child != 0 {
                    match seen.iter_mut().find(|(w, ..)| *w == at.child) {
                        Some((_, l, t, r, b)) => {
                            *l = (*l).min(sx);
                            *t = (*t).min(sy);
                            *r = (*r).max(sx);
                            *b = (*b).max(sy);
                        }
                        None => seen.push((at.child, sx, sy, sx, sy)),
                    }
                }
                if at.child != 0 && within(conn, at.child, target)? {
                    lo_x = lo_x.min(sx);
                    lo_y = lo_y.min(sy);
                    hi_x = hi_x.max(sx);
                    hi_y = hi_y.max(sy);
                }
                sx += 24;
            }
            sy += 24;
        }
        let seen: Vec<String> = seen
            .iter()
            .map(|(w, l, t, r, b)| format!("0x{w:x}@x={l}..{r},y={t}..{b}"))
            .collect();
        eprintln!(
            "xtest_selection_driver: sweep saw children [{}]",
            seen.join(",")
        );
        if lo_x == i16::MAX {
            eprintln!(
                "xtest_selection_driver: sweep found the target under the pointer nowhere on {}x{}",
                screen.width, screen.height
            );
        } else {
            eprintln!(
                "xtest_selection_driver: sweep found the target under the pointer at x={lo_x}..{hi_x} y={lo_y}..{hi_y}"
            );
        }
        return Err(Failure("pointer_not_over_target"));
    }
    Ok(())
}

/// Wait until the pointer, moved into `window` by XTEST, resolves to it --
/// the point at which the window is routable rather than merely mapped.
fn settle(conn: &RustConnection, root: Window, window: Window) -> Result<Frame, Failure> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let placed = frame(conn, root, window)?;
        let (x, y) = (placed.x.saturating_add(8), placed.y.saturating_add(8));
        fake(conn, root, MOTION, 0, x, y)?;
        let child = conn.query_pointer(root)?.reply()?.child;
        if child != 0 && within(conn, child, window)? {
            return Ok(placed);
        }
        if Instant::now() >= deadline {
            return Err(Failure("window_never_routable"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Select, on the deepest window under the pointer, what another client may
/// share with xterm: motion, release and crossings. ButtonPressMask is
/// exclusive and xterm holds it, so the press itself is inferred from the
/// motion state that follows it.
fn watch_deepest(conn: &RustConnection, top: Window) -> Result<Window, Failure> {
    let mut window = top;
    loop {
        let child = conn.query_pointer(window)?.reply()?.child;
        if child == 0 {
            break;
        }
        window = child;
    }
    let mask = EventMask::POINTER_MOTION
        | EventMask::BUTTON_RELEASE
        | EventMask::ENTER_WINDOW
        | EventMask::LEAVE_WINDOW;
    conn.change_window_attributes(
        window,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::new().event_mask(mask),
    )?;
    conn.get_input_focus()?.reply()?;
    eprintln!("xtest_selection_driver: watching 0x{window:x} (top 0x{top:x})");
    Ok(window)
}

/// Print every event the watched window received during the drag, with the
/// state it carried. A drag is motion with Button1Mask (0x100) set.
fn report_watched(conn: &RustConnection, watched: Window) -> Result<(), Failure> {
    let mut count = 0;
    while let Some(event) = conn.poll_for_event()? {
        count += 1;
        let line = match &event {
            Event::MotionNotify(e) => format!(
                "motion ev=0x{:x} child=0x{:x} at=({},{}) state=0x{:x}",
                e.event,
                e.child,
                e.event_x,
                e.event_y,
                u16::from(e.state)
            ),
            Event::ButtonRelease(e) => format!(
                "release ev=0x{:x} detail={} state=0x{:x}",
                e.event,
                e.detail,
                u16::from(e.state)
            ),
            Event::EnterNotify(e) => {
                format!("enter ev=0x{:x} state=0x{:x}", e.event, u16::from(e.state))
            }
            Event::LeaveNotify(e) => {
                format!("leave ev=0x{:x} state=0x{:x}", e.event, u16::from(e.state))
            }
            Event::Error(e) => format!("error {:?} major={}", e.error_kind, e.major_opcode),
            other => format!("other {other:?}"),
        };
        eprintln!("xtest_selection_driver: watched {line}");
    }
    eprintln!("xtest_selection_driver: watched 0x{watched:x} received {count} events");
    Ok(())
}

fn run(row: u16, paste: bool, overshoot: bool) -> Result<String, Failure> {
    let (conn, screen_index) = x11rb::connect(None).map_err(|_| Failure("connect"))?;
    let root = conn.setup().roots[screen_index].root;

    // One xterm at a time. In a session with no window manager the first
    // window to map is the one admitted and routed; spawning both at once made
    // which one that was a race, and the drag landed on A in only some runs.
    let mut terminals = Terminals(vec![xterm(
        "sophia-t124-a",
        "60x8+40+40",
        &format!("printf '%s\\n' {MARKER}; exec sleep 120"),
    )?]);
    let a = find_toplevel(&conn, root, "sophia-t124-a")?;
    let fa = settle(&conn, root, a)?;
    terminals
        .0
        .push(xterm("sophia-t124-b", "60x8+40+320", "exec sleep 120")?);
    let b = find_toplevel(&conn, root, "sophia-t124-b")?;
    // B is only needed for the paste half, and may never become routable; that
    // is reported there rather than failing the drag here.
    std::thread::sleep(Duration::from_millis(800));
    let fb = frame(&conn, root, b)?;
    // A window manager may have moved and resized A to make room for B, so
    // its frame is read again now rather than trusted from before B mapped.
    let settled_a = fa;
    let fa = frame(&conn, root, a)?;
    if fa.height < ROWS || fa.width < 40 || fb.height < ROWS {
        return Err(Failure("xterm_too_small"));
    }

    // Row centre: the requested row of an 8-row terminal. Row 0 holds the
    // marker; any other row holds nothing xterm will keep.
    // Row centre from xterm's own cell height, which it publishes as the
    // resize increment in WM_NORMAL_HINTS; the base size is its borders. A
    // window manager may have made A far taller than the eight rows asked
    // for, in which case height / ROWS lands on a blank row well below the
    // marker. Without hints the eight-row assumption stands.
    let (cell, top) = x11rb::properties::WmSizeHints::get_normal_hints(&conn, a)
        .ok()
        .and_then(|cookie| cookie.reply().ok().flatten())
        .and_then(|hints| {
            let (_, increment) = hints.size_increment?;
            let base = hints.base_size.map_or(0, |(_, height)| height);
            u16::try_from(increment)
                .ok()
                .filter(|increment| *increment > 0)
                .map(|increment| (increment, u16::try_from(base / 2).unwrap_or(0)))
        })
        .unwrap_or((fa.height / ROWS, 0));
    let y = fa.y.saturating_add((top + cell * row + cell / 2) as i16);
    let start = fa.x.saturating_add(8);
    // With --overshoot the release is past A's right edge, which only the
    // implicit grab keeps with A. The session holds the pressed surface; the
    // frontend still picks the window inside it by position, so the release
    // reaches xterm's shell rather than its text widget and xterm claims
    // nothing (t158). Until that lands the probe is red and the default drag
    // ends inside A.
    let end = if overshoot {
        fa.x.saturating_add(fa.width as i16).saturating_add(24)
    } else {
        fa.x.saturating_add((fa.width - 16) as i16)
    };

    eprintln!(
        "xtest_selection_driver: drag row={row} cell={cell} top={top} y={y} x={start}..{end} settled_a={},{} {}x{} a={},{} {}x{} b={},{} {}x{}",
        settled_a.x,
        settled_a.y,
        settled_a.width,
        settled_a.height,
        fa.x,
        fa.y,
        fa.width,
        fa.height,
        fb.x,
        fb.y,
        fb.width,
        fb.height
    );
    aim(&conn, root, a, start, y)?;
    let watched = watch_deepest(&conn, a)?;
    fake(&conn, root, PRESS, 1, start, y)?;
    let steps = 6;
    for step in 1..=steps {
        let x = start + ((end - start) as i32 * step / steps) as i16;
        fake(&conn, root, MOTION, 0, x, y)?;
    }
    fake(&conn, root, RELEASE, 1, end, y)?;
    std::thread::sleep(Duration::from_millis(400));
    report_watched(&conn, watched)?;

    let owner = conn
        .get_selection_owner(u32::from(AtomEnum::PRIMARY))?
        .reply()?
        .owner;
    if owner == 0 {
        return Err(Failure("primary_unowned"));
    }
    let owner_in_a = within(&conn, owner, a)?;
    if !owner_in_a {
        return Err(Failure("primary_owned_elsewhere"));
    }

    // Ask for the text the way a pasting client does, and read it back.
    let requestor = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        requestor,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    let utf8 = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let property = conn
        .intern_atom(false, b"SOPHIA_T124_SELECTION")?
        .reply()?
        .atom;
    conn.convert_selection(
        requestor,
        u32::from(AtomEnum::PRIMARY),
        utf8,
        property,
        x11rb::CURRENT_TIME,
    )?;
    conn.flush()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let bytes = loop {
        match conn.poll_for_event()? {
            Some(Event::SelectionNotify(notify)) if notify.requestor == requestor => {
                if notify.property == 0 {
                    return Err(Failure("convert_refused"));
                }
                break conn
                    .get_property(true, requestor, notify.property, utf8, 0, 4096)?
                    .reply()?
                    .value;
            }
            _ if Instant::now() >= deadline => return Err(Failure("selection_notify_timeout")),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let matched = bytes
        .windows(MARKER.len())
        .any(|window| window == MARKER.as_bytes());
    if !matched {
        return Err(Failure("selection_text_mismatch"));
    }

    // The paste half: a middle-click in B. What B does with it is B's own
    // ConvertSelection on the wire, counted by the session, not seen here.
    let pointer_in_b = if paste {
        let cell_b = fb.height / ROWS;
        let yb = fb.y.saturating_add((cell_b / 2) as i16);
        let xb = fb.x.saturating_add((fb.width / 2) as i16);
        aim(&conn, root, b, xb, yb)?;
        fake(&conn, root, PRESS, 2, xb, yb)?;
        fake(&conn, root, RELEASE, 2, xb, yb)?;
        std::thread::sleep(Duration::from_millis(600));
        true
    } else {
        false
    };

    drop(terminals);
    // The variable facts go to stderr, which the session log carries. Stdout
    // is the fixed verdict the session compares byte for byte.
    eprintln!(
        "xtest_selection_driver: owner=0x{owner:x} bytes={} row={row}",
        bytes.len()
    );
    Ok(format!(
        "sophia_xtest_selection schema=1 status=pass owner_in_a=true matched=true pointer_in_a=true pointer_in_b={pointer_in_b}"
    ))
}

fn main() {
    let mut row = 0u16;
    let mut paste = true;
    let mut overshoot = false;
    for argument in std::env::args().skip(1) {
        if let Some(value) = argument.strip_prefix("--row=") {
            row = value.parse().unwrap_or(0).min(ROWS - 1);
        } else if argument == "--no-paste" {
            paste = false;
        } else if argument == "--overshoot" {
            overshoot = true;
        }
    }
    match run(row, paste, overshoot) {
        Ok(record) => print!("{record}"),
        Err(Failure(reason)) => {
            // Stdout is captured and compared by the session; stderr reaches
            // the session log, which is where a failed run is read.
            eprintln!("xtest_selection_driver: status=fail reason={reason}");
            // Exit 0 on a finding. A failing client takes the session's
            // client-fatal path, which skips the completion records --
            // `sophia_live_selection` among them, the wire counter this run
            // exists to read. The verdict is in stdout, which the gate
            // compares; a non-zero exit is left to mean the driver crashed.
            print!("sophia_xtest_selection schema=1 status=fail reason={reason}");
        }
    }
}

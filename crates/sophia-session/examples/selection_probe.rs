//! Who owns a selection, and what a client gets when it asks for the text.
//!
//! Two roles in one binary, so a transfer can be proved with both ends under
//! test. `claim` becomes a real owner: it takes the selection and then answers
//! SelectionRequest with TARGETS and UTF8_STRING the way a toolkit does. The
//! default role reads GetSelectionOwner and runs a ConvertSelection against
//! whoever holds it.
//!
//! Serving, not merely claiming, is the point. An owner that claims and then
//! ignores SelectionRequest makes a working authority look broken -- the
//! requestor simply waits -- and a probe that cannot tell those apart is worse
//! than no probe.
//!
//! Run both against a private host, away from the operator display:
//!
//!     cargo build -p sophia-x-authority --example x11_conformance_host
//!     target/debug/examples/x11_conformance_host /tmp/.X11-unix/X88 &
//!     export DISPLAY=:88 XAUTHORITY=/tmp/unused-Xauthority
//!     selection_probe PRIMARY          # owner: 0x0, nobody has claimed it
//!     selection_probe PRIMARY claim &  # claims and serves
//!     selection_probe PRIMARY          # bytes: "SOPHIA-T124-PAYLOAD"
//!
//! t124 asked whether the selection path carries a real client's text. Run
//! that way it does, over the full round trip: SetSelectionOwner, then
//! GetSelectionOwner naming the owner, then ConvertSelection reaching the
//! owner as a SelectionRequest, the reply property, and the bytes back.

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

/// The bytes a requestor must get back for the transfer to count as proved.
const PAYLOAD: &[u8] = b"SOPHIA-T124-PAYLOAD";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let which = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "PRIMARY".to_owned());
    let (conn, screen_index) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_index];
    let selection = match which.as_str() {
        "PRIMARY" => u32::from(AtomEnum::PRIMARY),
        name => conn.intern_atom(false, name.as_bytes())?.reply()?.atom,
    };

    // `claim` makes this process a REAL owner: it holds the selection and
    // answers SelectionRequest the way any toolkit does. Serving matters --
    // an owner that claims but never replies makes a working authority look
    // broken, which is the same "the answer was not true" defect this probe
    // exists to find.
    if std::env::args().nth(2).as_deref() == Some("claim") {
        let window = conn.generate_id()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        conn.set_selection_owner(window, selection, x11rb::CURRENT_TIME)?;
        conn.flush()?;
        let held = conn.get_selection_owner(selection)?.reply()?.owner;
        println!("claimed {which} as {window:#x}; server reports owner {held:#x}");
        if held != window {
            println!("OWNERSHIP NOT RECORDED -- the authority did not accept the claim");
            return Ok(());
        }

        let utf8 = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
        let targets = conn.intern_atom(false, b"TARGETS")?.reply()?.atom;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            match conn.poll_for_event()? {
                Some(Event::SelectionRequest(request)) => {
                    // A property of None marks an obsolete requestor; the
                    // convention is to answer on the target atom instead.
                    let property = if request.property == 0 {
                        request.target
                    } else {
                        request.property
                    };
                    let served = if request.target == utf8
                        || request.target == u32::from(AtomEnum::STRING)
                    {
                        conn.change_property8(
                            PropMode::REPLACE,
                            request.requestor,
                            property,
                            request.target,
                            PAYLOAD,
                        )?;
                        true
                    } else if request.target == targets {
                        conn.change_property32(
                            PropMode::REPLACE,
                            request.requestor,
                            property,
                            AtomEnum::ATOM,
                            &[targets, utf8],
                        )?;
                        true
                    } else {
                        false
                    };
                    println!(
                        "SelectionRequest target={} requestor={:#x} served={served}",
                        request.target, request.requestor,
                    );
                    let notify = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: request.time,
                        requestor: request.requestor,
                        selection: request.selection,
                        target: request.target,
                        property: if served { property } else { 0 },
                    };
                    conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
                    conn.flush()?;
                }
                Some(_) => {}
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        println!("owner exiting after 20s");
        return Ok(());
    }

    let owner = conn.get_selection_owner(selection)?.reply()?.owner;
    println!(
        "{which} owner: {owner:#x}{}",
        if owner == 0 {
            "  (None -- nobody claimed it)"
        } else {
            ""
        }
    );
    if owner == 0 {
        return Ok(());
    }

    // Ask the owner for the bytes, through the authority, as any paste does.
    let requestor = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        requestor,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    let target = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let property = conn
        .intern_atom(false, b"SOPHIA_SELECTION_PROBE")?
        .reply()?
        .atom;
    conn.convert_selection(requestor, selection, target, property, x11rb::CURRENT_TIME)?;
    conn.flush()?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        match conn.poll_for_event()? {
            Some(Event::SelectionNotify(notify)) => {
                if notify.property == 0 {
                    println!("owner refused the conversion (property None)");
                    return Ok(());
                }
                let value = conn
                    .get_property(true, requestor, notify.property, target, 0, 4096)?
                    .reply()?;
                println!("bytes: {:?}", String::from_utf8_lossy(&value.value));
                return Ok(());
            }
            Some(_) => {}
            None => std::thread::sleep(std::time::Duration::from_millis(30)),
        }
    }
    println!("no SelectionNotify within 3s -- the transfer never completed");
    Ok(())
}

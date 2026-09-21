//! Type a line into the focused window through XTEST.
//!
//! A dev tool for a session started with `--admit-xtest`. `xdotool` cannot be
//! used there: libxdo asks the root for `_NET_ACTIVE_WINDOW` on startup, and
//! when the property is absent it reports the type by name -- `XGetAtomName`
//! of None -- which libX11 answers by exiting the process. This asks for
//! nothing but the keyboard mapping and the focus that FakeInput resolves
//! itself. ASCII only, unshifted keys only; a trailing newline is Return.
//!
//!     cargo run -p sophia-session --example xtest_type -- $'touch /tmp/marker\n'

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xtest::ConnectionExt as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = std::env::args()
        .nth(1)
        .ok_or("usage: xtest_type <ascii text>; a trailing newline is typed as Return")?;
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let version = conn.xtest_get_version(2, 1)?.reply()?;
    eprintln!(
        "xtest {}.{} on the display",
        version.major_version, version.minor_version
    );
    let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
    let mapping = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    let per = usize::from(mapping.keysyms_per_keycode);
    let keycode_for = |keysym: u32| -> Option<u8> {
        mapping
            .keysyms
            .chunks(per)
            .position(|column| column.first() == Some(&keysym))
            .and_then(|index| u8::try_from(index).ok())
            .map(|index| min + index)
    };
    let mut typed = 0usize;
    for ch in text.chars() {
        let keysym = match ch {
            '\n' => 0xff0d,
            c if c.is_ascii() && !c.is_ascii_control() => c as u32,
            other => return Err(format!("ascii only, cannot type {other:?}").into()),
        };
        let keycode = keycode_for(keysym)
            .ok_or_else(|| format!("no unshifted keycode for {ch:?} in the current mapping"))?;
        for (pressed, event_type) in [(true, 2u8), (false, 3u8)] {
            let _ = pressed;
            conn.xtest_fake_input(event_type, keycode, x11rb::CURRENT_TIME, root, 0, 0, 0)?;
            conn.flush()?;
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        typed += 1;
    }
    // A round trip: FakeInput has no reply, so this is what says every
    // request before it was processed.
    conn.get_input_focus()?.reply()?;
    eprintln!("typed {typed} keys");
    Ok(())
}

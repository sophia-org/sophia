//! Translating routed engine input events into synthetic file byte streams.

/// Formats a pointer event into standard Plan 9 `mouse` file format:
/// `'m' <x:11d> <y:11d> <buttons:11d> <msec:11d>\n`
pub fn format_plan9_mouse_event(x: i32, y: i32, buttons: u32, msec: u32) -> Vec<u8> {
    format!("m{:11}{:11}{:11}{:11}\n", x, y, buttons, msec).into_bytes()
}

/// Formats a keystroke event into UTF-8 bytes for `<id>/kbd`.
pub fn format_plan9_kbd_event(utf8_str: &str) -> Vec<u8> {
    utf8_str.as_bytes().to_vec()
}

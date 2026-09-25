//! Egress module for formatting engine input events for synthetic file consumption.

pub mod input;

pub use input::{format_plan9_kbd_event, format_plan9_mouse_event};

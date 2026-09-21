//! Sophia X Server Frontend implementation seed.
//!
//! This crate terminates a bounded, modern X11 subset and translates its
//! authority-owned resource state into Sophia transactions. It does not own
//! physical input, compositor policy, rendering, or DRM/KMS. The current crate
//! name remains `sophia-x-authority` while the source layout matures.

mod atom;
mod client_output;
mod clipboard;
mod close_target;
mod codec;
mod color;
mod connection_wait;
mod control_epoch;
mod device_bundle;
mod dispatch;
mod drawing;
mod event;
mod evidence;
mod explicit_pointer_grab;
mod focus_events;
mod font;
mod frontend_config;
mod frontend_types;
mod glx;
mod graphics_context;
mod image;
mod input_authority;
mod key_routing;
mod keyboard;
mod metadata;
mod observation;
mod packet;
mod pointer;
mod property;
mod ready_stream;
mod resource;
mod routing_types;
mod runtime;
mod selection;
mod setup;
mod shm;
mod socket;
mod software;
mod synthetic_executor;
mod transport;
mod window;
mod window_allocation;
mod wire;
mod x11_socket;

pub use atom::*;
pub use client_output::*;
pub use clipboard::*;
pub use close_target::*;
pub use codec::*;
pub use color::*;
pub use connection_wait::*;
pub use control_epoch::*;
pub use device_bundle::*;
pub use dispatch::*;
pub use drawing::*;
pub use event::*;
pub use explicit_pointer_grab::*;
pub use focus_events::*;
pub use font::*;
pub use frontend_config::*;
pub use frontend_types::*;
pub use ready_stream::*;
pub use software::geometry::arc::XArc;
pub use synthetic_executor::*;

/// Whether the operator asked to watch protocol traffic.
///
/// Opt-in, and reported at the level the session already runs at: requiring a
/// raised global level as well is what silenced a physical gate's telemetry
/// twice, because the filter replaces the default rather than adding to it.
pub(crate) fn x11_authority_trace_enabled() -> bool {
    std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some()
}
pub use glx::*;
pub use graphics_context::*;
pub use input_authority::*;
pub use keyboard::*;
pub use metadata::*;
pub use observation::*;
pub use packet::*;
pub use pointer::*;
pub use property::*;
pub use resource::*;
pub use routing_types::*;
pub use runtime::*;
pub use selection::*;
pub use setup::*;
pub use shm::*;
pub use socket::*;
pub use software::*;
pub use transport::*;
pub use window::*;
pub use window_allocation::*;
pub use wire::*;
pub use x11_socket::*;

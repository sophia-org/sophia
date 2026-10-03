//! The parts of the PAM package other crates use. The binary's libpam
//! binding stays in its own `ffi` module, so linking this library never links
//! libpam.
#![deny(unsafe_code)]

#[allow(unsafe_code)]
mod locked_page;

pub use locked_page::LockedPage;

//! Thin alias for the neutral cursor in `crate::byte_cursor`.
//!
//! The cursor itself has no IPC dependency; it moved to a crate-level
//! module so `crate::shell::encoding` can use it without depending on
//! `crate::ipc`. Every existing `super::cursor::...` / `crate::ipc::cursor::
//! ...` import in this module keeps resolving through this re-export, and
//! `crate::byte_cursor::CursorError`'s `From` impl into `IpcCodecError` keeps
//! every `?` in this module producing the identical error it does today.

pub(crate) use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

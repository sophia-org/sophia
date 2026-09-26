#![cfg(test)]

use super::*;

pub(in crate::live_session) fn startup(stream: UnixStream) -> (WmFileLimits, u64, u64, UnixStream) {
    super::startup::tests::selected_transport_startup(stream)
}

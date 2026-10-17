//! Lock sealed-image cancellation controls. The wire harness and its
//! source-path tests are shared with lock_image_transport; the
//! cancellation cases live in support/lock_image_cancel.rs.

#[path = "support/image_test_authority.rs"]
mod authority;
#[path = "support/shell_file_peer.rs"]
mod raw;
#[path = "support/lock_image_wire.rs"]
mod wire;

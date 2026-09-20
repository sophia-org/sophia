// Controls for the socket-directory layout (t141): the directory a client
// group reaches its listener through, and the promise a sandbox mounts it on.
//
// WHAT THESE CAN AND CANNOT PROVE. Mounting is the sandbox's half and needs a
// sandbox; these prove the half the session owes it, which is the half that
// makes mounting worth anything: a group directory that is owner-only, that
// holds its own socket and nothing else, and that contains no symbolic link
// leading back out to the trusted path. The exclusion a launcher achieves by
// mounting is only as good as that, so that is what is pinned here.

use crate::live_session::socket_directories::{
    ClientGroup, SocketDirectoryError, SocketDirectoryFault, SocketDirectoryLayout,
};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// A directory of this control's own, removed when it goes.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-socket-directory-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("a scratch root");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::symlink_metadata(path)
        .expect("a readable path")
        .permissions()
        .mode()
        & 0o777
}

/// The trusted path a confined client must not be able to reach. Named here
/// exactly as the host has it, because what the controls below refuse is a
/// way back to this.
const TRUSTED: &str = "/tmp/.X11-unix/X7";

#[test]
fn a_group_directory_is_owner_only_and_holds_only_its_socket() {
    let scratch = Scratch::new("holds-only");
    let layout = SocketDirectoryLayout::rooted_at(scratch.0.join("root"), 7).expect("a layout");
    let shared = layout.prepare_group(ClientGroup::Shared).expect("shared");
    let confined = layout
        .prepare_group(ClientGroup::Confined(1))
        .expect("confined");

    assert_ne!(
        shared.parent(),
        confined.parent(),
        "each group has a directory of its own"
    );
    assert_eq!(mode_of(shared.parent().unwrap()), 0o700);
    assert_eq!(mode_of(confined.parent().unwrap()), 0o700);

    // EVERY GROUP NAMES THE SAME SOCKET. A client whose directory is mounted
    // at the standard path asks for `:7` whichever group it is in, so nothing
    // about its launch changes when it becomes confined. The directory is
    // what differs, and the directory is the part a client cannot name.
    assert_eq!(shared.file_name(), confined.file_name());
    assert_eq!(shared.file_name().unwrap(), "X7");

    // A directory with nothing in it yet is already mountable; so is one
    // holding exactly its socket, which is the state after a listener binds.
    layout
        .verify_group(ClientGroup::Confined(1))
        .expect("an empty group directory is mountable");
    std::fs::write(&confined, b"").expect("the socket's place");
    layout
        .verify_group(ClientGroup::Confined(1))
        .expect("a group directory holding only its socket is mountable");
}

#[test]
fn a_symbolic_link_wearing_the_sockets_own_name_is_refused() {
    // THE ESCAPE THE LAYOUT EXISTS TO PREVENT. A launcher mounts the
    // directory, so a link inside it is mounted too, and a link named `X7` is
    // the escape wearing the one name this directory is allowed to hold: a
    // confined client would open what it believes is its own socket and reach
    // the trusted one. Refused by kind, never by where it points -- following
    // it to ask would be the first half of taking it.
    let scratch = Scratch::new("link-named-socket");
    let layout = SocketDirectoryLayout::rooted_at(scratch.0.join("root"), 7).expect("a layout");
    let socket = layout
        .prepare_group(ClientGroup::Confined(2))
        .expect("confined");
    std::os::unix::fs::symlink(TRUSTED, &socket).expect("the planted link");

    let refusal = layout
        .verify_group(ClientGroup::Confined(2))
        .expect_err("a link back to the trusted path must be refused");
    assert!(
        matches!(
            refusal,
            SocketDirectoryError::Unusable {
                because: SocketDirectoryFault::SymbolicLink,
                ..
            }
        ),
        "{refusal:?}"
    );
}

#[test]
fn anything_besides_the_socket_is_refused_because_the_mount_takes_it_too() {
    let scratch = Scratch::new("stray");
    let layout = SocketDirectoryLayout::rooted_at(scratch.0.join("root"), 7).expect("a layout");
    let socket = layout
        .prepare_group(ClientGroup::Confined(3))
        .expect("confined");
    std::fs::write(&socket, b"").expect("the socket's place");
    // Innocent on its own, and mounted into the sandbox all the same.
    let stray = socket.parent().unwrap().join("notes.txt");
    std::fs::write(&stray, b"anything").expect("the stray entry");

    let refusal = layout
        .verify_group(ClientGroup::Confined(3))
        .expect_err("a group directory holding anything else is not mountable");
    assert!(
        matches!(refusal, SocketDirectoryError::Occupied { ref entry, .. } if entry == "notes.txt"),
        "{refusal:?}"
    );
}

#[test]
fn a_directory_others_may_enter_is_refused_rather_than_repaired() {
    // REFUSED, NOT CHMODDED. A directory found in place with a loose mode may
    // have been prepared by somebody else, and the session cannot tell that
    // from its own leftovers. Tightening it would be acting on an attacker's
    // path and then vouching for it.
    let scratch = Scratch::new("reachable");
    // The layout makes its own root, owner-only; the loose directory is
    // planted inside it afterwards, which is the order an attacker has.
    let layout = SocketDirectoryLayout::rooted_at(scratch.0.join("root"), 7).expect("a layout");
    let group = scratch.0.join("root").join("confined-4");
    std::fs::create_dir_all(&group).expect("a group directory");
    std::fs::set_permissions(&group, std::fs::Permissions::from_mode(0o755)).expect("a loose mode");

    let refusal = layout
        .prepare_group(ClientGroup::Confined(4))
        .expect_err("a world-enterable group directory must be refused");
    assert!(
        matches!(
            refusal,
            SocketDirectoryError::Unusable {
                because: SocketDirectoryFault::Reachable(0o755),
                ..
            }
        ),
        "{refusal:?}"
    );
    assert_eq!(
        mode_of(&scratch.0.join("root").join("confined-4")),
        0o755,
        "and left exactly as it was found"
    );
}

#[test]
fn a_symbolic_link_in_place_of_a_group_directory_is_refused() {
    // The same escape one level up: the directory itself is the link, so the
    // socket bound in it is bound somewhere else entirely.
    let scratch = Scratch::new("link-directory");
    let layout = SocketDirectoryLayout::rooted_at(scratch.0.join("root"), 7).expect("a layout");
    let elsewhere = scratch.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("somewhere else");
    std::os::unix::fs::symlink(&elsewhere, scratch.0.join("root").join("confined-5"))
        .expect("the planted link");

    let refusal = layout
        .prepare_group(ClientGroup::Confined(5))
        .expect_err("a group directory that is a link must be refused");
    assert!(
        matches!(
            refusal,
            SocketDirectoryError::Unusable {
                because: SocketDirectoryFault::SymbolicLink,
                ..
            }
        ),
        "{refusal:?}"
    );
}

#[test]
fn a_relative_root_is_refused_because_the_sandbox_would_not_agree_on_it() {
    let refusal = SocketDirectoryLayout::rooted_at(PathBuf::from("relative/root"), 7)
        .expect_err("a relative root must be refused");
    assert!(
        matches!(refusal, SocketDirectoryError::RuntimeDirRelative(_)),
        "{refusal:?}"
    );
}

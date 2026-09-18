#![cfg(all(test, unix))]

//! The private bind refuses an occupied path instead of reclaiming it.
//!
//! WHAT THIS EXISTS TO CATCH. The reclaiming bind treats "this path is a
//! socket" as "this socket is stale", which is true of a crash leftover and
//! equally true of a live listener. A second service therefore unlinked the
//! first one's inode, bound a fresh one at the same path, and reported itself
//! ready, while the displaced service went on serving an inode no client could
//! reach. Both believed they owned the path, and nothing told either of them
//! otherwise. These controls assert the refusal and, more importantly, that the
//! original listener is still the one a client reaches afterwards.

use std::io::ErrorKind;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Barrier};

use crate::bind_x11_core_socket_server_exclusive;

fn unique_path(label: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "sophia-exclusive-bind-{label}-{}-{unique}.sock",
        std::process::id()
    ))
}

/// A second bind at a live listener's path is refused, and the first still owns
/// it.
///
/// THE OWNERSHIP ASSERTION IS THE POINT. A test that only checked for an error
/// would still pass if the refusal arrived after the inode had been replaced.
/// This connects afterwards and requires the *original* listener to accept, so
/// the path must still lead to it.
#[test]
fn exclusive_bind_refuses_an_occupied_path_and_keeps_the_first_owner() {
    let path = unique_path("occupied");
    let _ = std::fs::remove_file(&path);

    let first = bind_x11_core_socket_server_exclusive(&path).expect("first bind owns the path");

    let second = bind_x11_core_socket_server_exclusive(&path);
    assert!(
        second.is_err(),
        "a second exclusive bind must be refused while the first is listening"
    );

    // The path must still reach the first listener, not a replacement.
    let client = UnixStream::connect(&path).expect("the path still leads to a listener");
    let (accepted, _) = first.accept().expect("the original listener accepts it");
    drop(accepted);
    drop(client);

    drop(first);
    let _ = std::fs::remove_file(&path);
}

/// A path left behind by nothing is still refused.
///
/// NOT A RECLAIM, EVEN WHEN RECLAIMING WOULD HAVE WORKED. In this mode a stale
/// path is an explicit refusal: this bind cannot tell a leftover from a live
/// listener without a race, so it does not try. Deciding a path is genuinely
/// dead belongs to whoever can establish that, and it is deliberately not
/// offered here as a flag.
#[test]
fn exclusive_bind_refuses_a_stale_path_rather_than_reclaiming_it() {
    let path = unique_path("stale");
    let _ = std::fs::remove_file(&path);

    // A real socket file with nothing serving it.
    let leftover = bind_x11_core_socket_server_exclusive(&path).expect("bind to create the path");
    drop(leftover);
    assert!(path.exists(), "the socket file outlives its listener");

    let refused = bind_x11_core_socket_server_exclusive(&path);
    assert!(
        refused.is_err(),
        "a stale path is refused rather than reclaimed"
    );

    let _ = std::fs::remove_file(&path);
}

/// Many threads race one path; exactly one of them owns it.
///
/// THE RACE IS THE WHOLE TEST. Every preflight alternative -- a stat, a connect
/// probe, a lock file -- has a window between deciding the path is free and
/// binding it, and under a barrier that window is exactly what several threads
/// occupy at once. `bind(2)` has no such window: the kernel refuses an existing
/// path atomically, so the count of winners is one no matter how the threads
/// interleave.
#[test]
fn racing_exclusive_binds_leave_exactly_one_owner() {
    let path = unique_path("raced");
    let _ = std::fs::remove_file(&path);

    const RACERS: usize = 8;
    let barrier = Arc::new(Barrier::new(RACERS));
    let mut racers = Vec::new();
    for _ in 0..RACERS {
        let barrier = Arc::clone(&barrier);
        let path = path.clone();
        racers.push(std::thread::spawn(move || {
            barrier.wait();
            bind_x11_core_socket_server_exclusive(&path)
        }));
    }

    let mut owners = Vec::new();
    let mut refusals = 0usize;
    for racer in racers {
        match racer.join().expect("racer did not panic") {
            Ok(listener) => owners.push(listener),
            Err(_) => refusals += 1,
        }
    }

    assert_eq!(owners.len(), 1, "exactly one racer may own the path");
    assert_eq!(
        refusals,
        RACERS - 1,
        "every other racer is told it did not get it"
    );

    // And the winner is who the path actually leads to.
    let owner = owners.pop().expect("one owner");
    let client = UnixStream::connect(&path).expect("the path leads to the winner");
    let (accepted, _) = owner.accept().expect("the winning listener accepts");
    drop(accepted);
    drop(client);
    drop(owner);

    let _ = std::fs::remove_file(&path);
}

/// The refusal is the kernel's, not a guess.
///
/// Distinguishing this from any other bind failure matters: an operator reading
/// "already present" must not be told the same thing when, say, the directory
/// is missing.
#[test]
fn exclusive_bind_reports_a_missing_directory_differently() {
    let path = unique_path("absent-dir").join("nested").join("core.sock");
    let refused = bind_x11_core_socket_server_exclusive(&path)
        .expect_err("a path under a missing directory cannot be bound");
    let message = refused.to_string();
    assert!(
        !message.contains("already"),
        "a missing directory is not an occupied path: {message}"
    );
    assert_eq!(
        std::fs::metadata(&path).map(|_| ()).unwrap_err().kind(),
        ErrorKind::NotFound,
        "nothing was created"
    );
}

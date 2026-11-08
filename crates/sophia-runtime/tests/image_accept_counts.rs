//! Image listener accept attempts and connector process-alive polls count at
//! their executed sites, in the cell of the caller's context, whatever the
//! attempt returns: an idle listener, and a join refused after its poll.
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

use rustix::net::*;
use sophia_9p::ConnectionId;
use sophia_runtime::authenticated_files::AuthenticatedFiles;
use sophia_runtime::image_channel::ImageChannelListener;
use sophia_runtime::liveness_counts::{self, Boundary, Entry, Field};

fn cell(entry: Entry, boundary: Boundary, field: Field) -> u64 {
    liveness_counts::snapshot().get(entry, boundary).get(field)
}

#[test]
fn accept_attempts_and_alive_polls_count_where_they_run_even_when_refused() {
    let directory =
        std::env::temp_dir().join(format!("sophia-image-accept-counts-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = ImageChannelListener::bind(&directory).unwrap();
    let identity = sophia_runtime::PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let mut legacy = AuthenticatedFiles::legacy(());
    let now = Instant::now();

    // An idle listener: one attempt, no connector to poll.
    let outside = cell(Entry::Outside, Boundary::Other, Field::AcceptAttempts);
    let alive = liveness_counts::snapshot().total(Field::ImageAlivePolls);
    assert!(
        listener
            .accept_authenticated(identity, ConnectionId(7), &mut legacy, now)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cell(Entry::Outside, Boundary::Other, Field::AcceptAttempts),
        outside + 1
    );
    assert_eq!(
        liveness_counts::snapshot().total(Field::ImageAlivePolls),
        alive
    );

    // Inside a turn's accept boundary with a connector waiting: the attempt
    // and the connector's alive poll count there, although legacy admission
    // then refuses the join.
    let socket = socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )
    .unwrap();
    connect(
        &socket,
        &SocketAddrUnix::new(directory.join("image.sock")).unwrap(),
    )
    .unwrap();
    let attempts = cell(Entry::Turn, Boundary::ImageAccept, Field::AcceptAttempts);
    let polls = cell(Entry::Turn, Boundary::ImageAccept, Field::ImageAlivePolls);
    {
        let _turn = liveness_counts::enter(Entry::Turn);
        let _accept = liveness_counts::within(Boundary::ImageAccept);
        assert!(
            listener
                .accept_authenticated(identity, ConnectionId(7), &mut legacy, now)
                .is_err()
        );
    }
    assert_eq!(
        cell(Entry::Turn, Boundary::ImageAccept, Field::AcceptAttempts),
        attempts + 1
    );
    assert_eq!(
        cell(Entry::Turn, Boundary::ImageAccept, Field::ImageAlivePolls),
        polls + 1
    );
    assert_eq!(
        cell(Entry::Outside, Boundary::Other, Field::AcceptAttempts),
        outside + 1
    );
    drop(listener);
    std::fs::remove_dir_all(directory).unwrap();
}

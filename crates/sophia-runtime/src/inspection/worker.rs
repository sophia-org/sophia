use super::export::InspectionExport;
use super::publication::Shared;
use super::*;
use sophia_9p::records::Limits;
use sophia_9p::unix::Server;
use std::io::Read;
use std::time::Duration;

const MAX_PEERS: usize = 4;

/// One worker serves at most four admitted sockets. Per-socket core instances
/// keep authority and pinned handles independent without per-client threads.
/// The 20ms bound also rechecks quiet peers; owner wakeups interrupt it early.
pub(super) fn run(
    listener: UnixListener,
    mut wake: UnixStream,
    domain: Arc<HostDomain>,
    shared: Arc<Shared>,
) {
    let mut peers: Vec<Server<InspectionExport>> = Vec::new();
    let limits =
        Limits::new(64 * 1024, 512, 8, 16, 128 * 1024, 1).expect("fixed inspection limits");
    while !shared.stopped.load(Ordering::SeqCst) {
        let mut fds = [
            rustix::event::PollFd::new(&listener, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&wake, rustix::event::PollFlags::IN),
        ];
        if rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 20_000_000,
            }),
        )
        .is_err()
        {
            break;
        }
        let mut buffer = [0; 128];
        while matches!(wake.read(&mut buffer), Ok(n) if n > 0) {}
        discard_revoked(&mut peers);
        for _ in 0..MAX_PEERS {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    shared.stopped.store(true, Ordering::SeqCst);
                    break;
                }
            };
            if peers.len() == MAX_PEERS {
                continue;
            }
            let excluded = [shared.excluded.load(Ordering::SeqCst)];
            let Ok(peer) = domain.admit(&stream, &excluded) else {
                continue;
            };
            let Ok(export) = InspectionExport::new(shared.clone(), domain.clone(), peer) else {
                continue;
            };
            let Ok(mut server) = Server::new(export, limits) else {
                continue;
            };
            if server.adopt(stream).is_ok() {
                peers.push(server);
            }
        }
        service_peers(&mut peers);
    }
    shared.stopped.store(true, Ordering::SeqCst);
    for server in &mut peers {
        close(server);
    }
}

fn close(server: &mut Server<InspectionExport>) {
    // The core's stop path closes fids (including release callbacks) and drops
    // queued replies before servicing any other request or socket output.
    server.wake().stop();
    let _ = server.turn(Some(Duration::ZERO));
}

fn discard_revoked(peers: &mut Vec<Server<InspectionExport>>) {
    peers.retain_mut(|server| {
        if server.export().authorized() {
            true
        } else {
            close(server);
            false
        }
    });
}

pub(super) fn service_peers(peers: &mut Vec<Server<InspectionExport>>) {
    peers.retain_mut(|server| {
        if !server.export().authorized() {
            close(server);
            return false;
        }
        server.wake().wake();
        server.turn(Some(Duration::ZERO)).unwrap_or(false) && server.connection_count() != 0
    });
}

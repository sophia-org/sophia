use super::export::LaunchExport;
use super::*;
use sophia_9p::{records::Limits, unix::Server};
use std::sync::mpsc::SyncSender;

pub(super) fn run(
    listener: UnixListener,
    wake: sophia_wake::Wake,
    domain: Arc<HostDomain>,
    shared: Arc<Shared>,
    sender: SyncSender<DeveloperLaunchTicket>,
) {
    let mut peers: Vec<Server<LaunchExport>> = Vec::new();
    let mut next_qid = 1u64;
    let limits = Limits::new(64 * 1024, 512, 8, 16, 128 * 1024, 1).expect("fixed launch limits");
    while !shared.stopped.load(Ordering::Acquire) {
        if wake.clear().is_err() {
            break;
        }
        peers.retain_mut(|server| {
            if !server.export_mut().service() {
                close(server);
                return false;
            }
            server.wake().wake();
            server.turn(Some(Duration::ZERO)).unwrap_or(false) && server.connection_count() != 0
        });
        for _ in 0..DEVELOPER_LAUNCH_MAX_PEERS {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    shared.stopped.store(true, Ordering::Release);
                    break;
                }
            };
            if peers.len() == DEVELOPER_LAUNCH_MAX_PEERS {
                continue;
            }
            let Ok(peer) = domain.admit(&stream, &[]) else {
                continue;
            };
            let Some(next) = next_qid.checked_add(8) else {
                shared.stopped.store(true, Ordering::Release);
                break;
            };
            let export = LaunchExport::new(
                domain.clone(),
                peer,
                shared.clone(),
                sender.clone(),
                next_qid,
            );
            next_qid = next;
            let Ok(mut server) = Server::new(export, limits) else {
                break;
            };
            if server.adopt(stream).is_ok() {
                peers.push(server);
            }
        }
        // Bounded cadence services peer fds and deadlines without per-peer
        // threads. Owner claim/result publications interrupt this wait.
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
    }
    shared.stopped.store(true, Ordering::Release);
    for server in &mut peers {
        close(server);
    }
    shared.owner_wake.notify();
}
fn close(server: &mut Server<LaunchExport>) {
    server.wake().stop();
    let _ = server.turn(Some(Duration::ZERO));
}

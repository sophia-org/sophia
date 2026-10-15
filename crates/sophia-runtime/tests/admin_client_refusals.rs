use sophia_9p::unix::Server;
use sophia_9p::*;
use sophia_protocol::{ControlCommand, ControlOwner};
use sophia_runtime::AdminClient;
use std::os::unix::net::UnixListener;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Deliberately untrustworthy text over the real core. The client must validate
/// role records independently of the server's valid 9P framing.
struct Peer {
    info: Vec<u8>,
    status: Vec<u8>,
    short_write: bool,
}
impl Export for Peer {
    type Node = u8;
    type Handle = ();
    fn attach(&mut self, _: &AttachContext<'_>) -> Result<Attachment<u8>, Errno> {
        Ok(Attachment {
            root: 0,
            epoch: Epoch(1),
        })
    }
    fn check(&mut self, _: &Access<'_, u8>) -> Result<(), Errno> {
        Ok(())
    }
    fn lookup(&mut self, _: &u8, name: WalkName<'_>) -> Result<u8, Errno> {
        match name {
            WalkName::Child(b"info") => Ok(1),
            WalkName::Child(b"commands") => Ok(2),
            WalkName::Child(b"status") => Ok(3),
            WalkName::Child(b"ctl") => Ok(4),
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &u8, _: Option<&()>) -> Entry {
        Entry {
            kind: if *node == 0 {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: u64::from(*node),
            qid_version: 0,
            permissions: 0o700,
            size: 0,
        }
    }
    fn open(&mut self, _: &u8, _: OpenFlags) -> Result<(), Errno> {
        Ok(())
    }
    fn read(
        &mut self,
        node: &u8,
        _: &mut (),
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let bytes = match node {
            1 => self.info.as_slice(),
            2 => b"v1 catalog 7 1\nv1 command policy 10:focus next\n",
            3 => self.status.as_slice(),
            _ => return Err(Errno::EACCES),
        };
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        // Short reads must be assembled under one snapshot deadline.
        let end = (start + (count as usize).min(7)).min(bytes.len());
        Ok(ReadOutcome::Ready(bytes[start..end].to_vec()))
    }
    fn write(&mut self, _: &u8, _: &mut (), _: u64, bytes: &[u8]) -> Result<u32, Errno> {
        Ok(bytes.len() as u32 - u32::from(self.short_write))
    }
    fn release(&mut self, _: u8, _: Option<()>) {}
}

fn with_peer(info: &[u8], status: &[u8], short_write: bool, check: impl FnOnce(&std::path::Path)) {
    let path = std::env::temp_dir().join(format!(
        "admin-refusal-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stop = stopped.clone();
    let peer = Peer {
        info: info.to_vec(),
        status: status.to_vec(),
        short_write,
    };
    let worker = std::thread::spawn(move || {
        let mut server = Server::new(peer, Limits::default()).unwrap();
        server.listen(listener).unwrap();
        while !worker_stop.load(Ordering::Acquire) {
            server.turn(Some(Duration::from_millis(10))).unwrap();
        }
    });
    // Stop the bounded server even when an assertion fails.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&path)));
    stopped.store(true, Ordering::Release);
    worker.join().unwrap();
    std::fs::remove_file(path).unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn malformed_or_misattributed_results_are_uncertain_and_never_reused() {
    for status in [
        b"v1 outcome 8 1 7 committed 0:\n".as_slice(),
        b"v1 outcome 7 2 7 committed 0:\n",
        b"v1 outcome 7 1 8 committed 0:\n",
        b"v1 pending 7 2 7\n",
        b"v1 ready 7\n",
        b"v1 outcome 7 1 7 committed 1:\n",
        &[b'x'; 513],
    ] {
        with_peer(b"v1 info 1 2 7 10000 2000 60000\n", status, false, |path| {
            let mut client = AdminClient::connect(path).unwrap();
            assert!(
                client
                    .invoke(ControlCommand {
                        owner: ControlOwner::Policy,
                        name: "focus next".into()
                    })
                    .is_err()
            );
            assert!(client.invocation_pending());
            assert!(client.commands().is_err());
        });
    }
}

#[test]
fn partial_write_reply_is_not_a_terminal_command_result() {
    with_peer(
        b"v1 info 1 2 7 10000 2000 60000\n",
        b"v1 outcome 7 1 7 committed 0:\n",
        true,
        |path| {
            let mut client = AdminClient::connect(path).unwrap();
            assert!(
                client
                    .invoke(ControlCommand {
                        owner: ControlOwner::Policy,
                        name: "focus next".into()
                    })
                    .is_err()
            );
            assert!(client.invocation_pending());
        },
    );
}

#[test]
fn server_cannot_extend_deadlines_or_return_unbounded_discovery() {
    for info in [
        b"v1 info 1 2 7 4294967295 2000 60000\n".as_slice(),
        &[b'x'; 513],
    ] {
        with_peer(info, b"", false, |path| {
            assert!(AdminClient::connect(path).is_err());
        });
    }
}

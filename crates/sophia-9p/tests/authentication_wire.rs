mod support;
use sophia_9p::{
    authentication::{Attempt, Authentication, Identity},
    export::AuthAttachment,
    *,
};
use support::{Body, Frame, frame, frames, tattach, tclunk, tflush, tread, tversion, twrite};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Node {
    Root,
    Auth(Attempt),
}
struct Owner {
    state: Authentication,
    attempt: Option<Attempt>,
    live: bool,
    releases: usize,
    verified: bool,
}
impl Owner {
    fn new() -> Self {
        Self {
            state: Authentication::new(1, ConnectionId(2), 3, 4, 0).unwrap(),
            attempt: None,
            live: true,
            releases: 0,
            verified: false,
        }
    }
    fn verdict(&mut self) {
        self.state
            .verified(
                self.attempt.unwrap(),
                Identity {
                    principal: 5,
                    namespace: 6,
                    launch: 7,
                },
                0,
            )
            .unwrap();
        self.verified = true;
    }
}
impl Export for Owner {
    type Node = Node;
    type Handle = ();
    fn attach(&mut self, _: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        Err(Errno::EACCES)
    }
    fn auth(
        &mut self,
        c: &AttachContext<'_>,
        afid: Fid,
    ) -> Result<AuthAttachment<Node, ()>, Errno> {
        if !self.live || c.uname != b"user" || c.aname != b"view" {
            return Err(Errno::EACCES);
        }
        let a = self.state.begin(afid.0, 0).map_err(|_| Errno::EAGAIN)?;
        self.attempt = Some(a);
        Ok(AuthAttachment {
            node: Node::Auth(a),
            handle: (),
            epoch: Epoch(1),
            qid_path: 100 + a.attempt_generation,
        })
    }
    fn attach_authenticated(
        &mut self,
        c: &AttachContext<'_>,
        n: &Node,
        _: &mut (),
    ) -> Result<Attachment<Node>, Errno> {
        if !self.live || c.uname != b"user" || c.aname != b"view" {
            return Err(Errno::EACCES);
        }
        let Node::Auth(a) = n else {
            return Err(Errno::EBADF);
        };
        self.state.commit_attach(*a, 0).map_err(|_| Errno::EACCES)?;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(1),
        })
    }
    fn wire_reset(&mut self, _: ConnectionId) {
        let _ = self.state.reset(0);
        self.attempt = None;
        self.verified = false;
    }
    fn connection_closed(&mut self, _: ConnectionId) {
        self.state.close();
    }
    fn check(&mut self, _: &Access<'_, Node>) -> Result<(), Errno> {
        if self.live {
            Ok(())
        } else {
            Err(Errno::ESTALE)
        }
    }
    fn lookup(&mut self, _: &Node, _: WalkName<'_>) -> Result<Node, Errno> {
        Err(Errno::ENOENT)
    }
    fn describe(&self, n: &Node, _: Option<&()>) -> Entry {
        Entry {
            kind: if *n == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: 1,
            qid_version: 0,
            permissions: 0o400,
            size: 0,
        }
    }
    fn open(&mut self, _: &Node, _: OpenFlags) -> Result<(), Errno> {
        Ok(())
    }
    fn read(&mut self, n: &Node, _: &mut (), _: u64, count: u32) -> Result<ReadOutcome, Errno> {
        let Node::Auth(_) = n else {
            return Err(Errno::EISDIR);
        };
        if self.verified {
            Ok(ReadOutcome::Ready(
                b"verified"[..(count as usize).min(8)].to_vec(),
            ))
        } else {
            Ok(ReadOutcome::Pending)
        }
    }
    fn write(&mut self, _: &Node, _: &mut (), _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn release(&mut self, n: Node, _: Option<()>) {
        if let Node::Auth(a) = n {
            self.state.cancel(a);
            self.attempt = None;
            self.verified = false;
        }
        self.releases += 1;
    }
}

#[test]
fn rust_sdk_converses_on_the_afid_before_attaching_to_the_real_server() {
    use sophia_9p::unix::Server;
    use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let (client, stream) = UnixStream::pair().unwrap();
    let mut server = Server::new(Owner::new(), Limits::default()).unwrap();
    let connection = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
    server.export_mut().state = Authentication::new(1, connection, 3, 4, 0).unwrap();
    let thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while server.connection_count() != 0 {
            assert!(Instant::now() < deadline, "SDK exchange deadline");
            server.turn(Some(Duration::from_millis(5))).unwrap();
            let owner = server.export_mut();
            if owner.attempt.is_some() && !owner.verified {
                // Supplied owner verdict: this qualifies the wire join, not Factotum.
                owner.verdict();
            }
        }
        assert!(server.export_mut().state.pinned().is_some());
    });
    let mut client =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let deadline = || Instant::now() + Duration::from_secs(2);
    let (tag, afid) = client.auth(b"user", b"view").unwrap();
    assert!(
        matches!(client.wait(tag, deadline()).unwrap(), Reply::Auth(qid) if qid.kind == QidKind::Auth)
    );
    let tag = client.read(afid, 0, 32).unwrap();
    assert_eq!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Read(b"verified".to_vec())
    );
    let (tag, root) = client.attach_with_afid(afid, b"user", b"view").unwrap();
    assert_ne!(afid, root);
    assert!(matches!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    let tag = client.clunk(afid).unwrap();
    assert_eq!(client.wait(tag, deadline()).unwrap(), Reply::Clunk);
    assert_eq!(client.fid_count(), 1);
    drop(client);
    thread.join().unwrap();
}
fn send(c: &mut Connection<Owner>, o: &mut Owner, b: Vec<u8>) -> Vec<Frame> {
    assert_eq!(c.receive(o, &b).unwrap(), b.len());
    let r = frames(c.output());
    c.sent(c.output().len());
    r
}
fn auth(tag: u16) -> Vec<u8> {
    frame(
        102,
        tag,
        Body::default()
            .u32(10)
            .string(b"user")
            .string(b"view")
            .u32(u32::MAX),
    )
}
fn setup() -> (Connection<Owner>, Owner) {
    let mut o = Owner::new();
    let mut c = Connection::new(ConnectionId(2), None, Limits::default());
    assert_eq!(
        send(&mut c, &mut o, tversion(u16::MAX, 8192, b"9P2000.L"))[0].kind,
        101
    );
    (c, o)
}

#[test]
fn only_a_trusted_verdict_can_attach_and_reset_destroys_the_old_afid() {
    let (mut c, mut o) = setup();
    assert_eq!(
        send(&mut c, &mut o, tattach(1, 0, u32::MAX, b"user", b"view"))[0].errno(),
        Some(13)
    );
    let r = send(&mut c, &mut o, auth(2));
    assert_eq!(r[0].kind, 103);
    assert_eq!(
        send(&mut c, &mut o, twrite(3, 10, 0, b"success"))[0].errno(),
        Some(13)
    );
    assert_eq!(
        send(&mut c, &mut o, tattach(4, 0, 10, b"user", b"view"))[0].errno(),
        Some(13)
    );
    assert_eq!(c.fid_count(), 1);
    assert_eq!(o.state.pinned(), None);
    o.verdict();
    assert_eq!(
        send(&mut c, &mut o, tattach(5, 0, 10, b"forged", b"view"))[0].errno(),
        Some(13)
    );
    assert_eq!(
        send(&mut c, &mut o, tattach(6, 0, 10, b"user", b"view"))[0].kind,
        105
    );
    let identity = o.state.pinned();
    send(&mut c, &mut o, tversion(u16::MAX, 8192, b"9P2000.L"));
    assert_eq!(c.fid_count(), 0);
    assert_eq!(o.state.pinned(), identity);
    assert_eq!(
        send(&mut c, &mut o, tattach(7, 0, 10, b"user", b"view"))[0].errno(),
        Some(9)
    );
    assert_eq!(send(&mut c, &mut o, auth(8))[0].kind, 103);
    o.verdict();
    assert_eq!(
        send(&mut c, &mut o, tattach(9, 0, 10, b"user", b"view"))[0].kind,
        105
    );
}

#[test]
fn flush_only_cancels_the_waiting_read_and_revocation_rechecks_it() {
    let (mut c, mut o) = setup();
    send(&mut c, &mut o, auth(1));
    assert!(send(&mut c, &mut o, tread(2, 10, 0, 32)).is_empty());
    assert_eq!(send(&mut c, &mut o, tflush(3, 2))[0].kind, 109);
    assert_eq!(c.fid_count(), 1);
    assert!(send(&mut c, &mut o, tread(4, 10, 0, 32)).is_empty());
    o.live = false;
    c.retry_waiting(&mut o).unwrap();
    let r = frames(c.output());
    assert_eq!(r[0].errno(), Some(116));
    c.sent(c.output().len());
    send(&mut c, &mut o, tclunk(5, 10));
    assert_eq!(c.fid_count(), 0);
    assert_eq!(o.releases, 1);
    c.close(&mut o);
    c.close(&mut o);
    assert_eq!(o.releases, 1);
}

#[test]
fn a_full_fid_table_cannot_pin_an_identity_without_a_root() {
    let limits = Limits::new(8192, 512, 4, 1, 32768, 1).unwrap();
    let mut c = Connection::new(ConnectionId(2), None, limits);
    let mut o = Owner::new();
    send(&mut c, &mut o, tversion(u16::MAX, 8192, b"9P2000.L"));
    assert_eq!(send(&mut c, &mut o, auth(1))[0].kind, 103);
    o.verdict();
    assert_eq!(
        send(&mut c, &mut o, tattach(2, 0, 10, b"user", b"view"))[0].errno(),
        Some(24)
    );
    assert_eq!(
        o.state.pinned(),
        None,
        "root capacity must precede identity pinning"
    );
    assert_eq!(c.fid_count(), 1);
    assert_eq!(o.state.starts(), 1);
}

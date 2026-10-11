//! The production afid owner in the real core, with a minimal supplied role
//! view. The SDK reads the nonce from its afid; the protected C peer relays it.
use super::*;
use sophia_9p::{Errno, Fid, OpenFlags, ReadOutcome, export::*};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_session::launch_authentication::Authority;
use sophia_session::launch_authentication::role::{RoleAuthentication, RoleScope};

#[derive(Clone, PartialEq, Eq)]
enum Node {
    Auth(Attempt),
    Root,
    Api,
}
struct Role<'a> {
    auth: Option<RoleAuthentication>,
    book: &'a mut LaunchBook,
    supervisor: &'a mut ProcessSupervisor,
}
impl Role<'_> {
    fn auth(&mut self) -> &mut RoleAuthentication {
        self.auth.as_mut().unwrap()
    }
}
impl Export for Role<'_> {
    type Node = Node;
    type Handle = ();
    fn attach(&mut self, _: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        Err(Errno::EACCES)
    }
    fn auth(&mut self, c: &AttachContext<'_>, fid: Fid) -> Result<AuthAttachment<Node, ()>, Errno> {
        self.auth()
            .claims(c.connection, c.uname, c.aname, c.n_uname)?;
        let attempt = self.auth().begin(fid.0, Instant::now())?;
        Ok(AuthAttachment {
            node: Node::Auth(attempt),
            handle: (),
            epoch: Epoch(1),
            qid_path: 100 + attempt.attempt_generation,
        })
    }
    fn attach_authenticated(
        &mut self,
        c: &AttachContext<'_>,
        node: &Node,
        _: &mut (),
    ) -> Result<Attachment<Node>, Errno> {
        self.auth()
            .claims(c.connection, c.uname, c.aname, c.n_uname)?;
        let Node::Auth(attempt) = node else {
            return Err(Errno::EACCES);
        };
        // The role is fixed by fixture policy. No fallible publication follows
        // the identity commit; the core has already reserved root/reply space.
        self.auth.as_mut().unwrap().commit_attach(
            *attempt,
            Instant::now(),
            &mut Authority::Owner {
                book: self.book,
                supervisor: self.supervisor,
            },
        )?;
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(1),
        })
    }
    fn check(&mut self, a: &Access<'_, Node>) -> Result<(), Errno> {
        if matches!(a.node, Node::Auth(_)) {
            return self.auth().poll(Instant::now());
        }
        self.auth
            .as_mut()
            .unwrap()
            .check_admitted(
                Instant::now(),
                &mut Authority::Owner {
                    book: self.book,
                    supervisor: self.supervisor,
                },
            )
            .map(|_| ())
    }
    fn lookup(&mut self, node: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (node, name) {
            (Node::Root, WalkName::Child(b"api")) => Ok(Node::Api),
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &Node, _: Option<&()>) -> Entry {
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: if *node == Node::Root { 1 } else { 2 },
            qid_version: 0,
            permissions: 0o400,
            size: 4,
        }
    }
    fn open(&mut self, _: &Node, _: OpenFlags) -> Result<(), Errno> {
        Ok(())
    }
    fn read(
        &mut self,
        node: &Node,
        _: &mut (),
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        match node {
            Node::Auth(attempt) => self.auth.as_mut().unwrap().read(
                *attempt,
                offset,
                count,
                Instant::now(),
                &mut Authority::Owner {
                    book: self.book,
                    supervisor: self.supervisor,
                },
            ),
            Node::Api => Ok(ReadOutcome::Ready(
                b"role"[..(count as usize).min(4)].to_vec(),
            )),
            Node::Root => Err(Errno::EISDIR),
        }
    }
    fn write(&mut self, _: &Node, _: &mut (), _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn release(&mut self, node: Node, _: Option<()>) {
        if let Node::Auth(attempt) = node {
            self.auth().release(attempt, Instant::now());
        }
    }
    fn wire_reset(&mut self, _: ConnectionId) {
        self.auth().reset(Instant::now()).unwrap();
    }
    fn connection_closed(&mut self, _: ConnectionId) {
        self.auth().close(Instant::now());
    }
}

#[test]
fn afid_requires_real_verification_and_retained_role_handle_rechecks_custody() {
    joined(false);
}

#[test]
fn revocation_between_verified_status_and_attach_cannot_pin_identity() {
    joined(true);
}

fn joined(revoke_before_attach: bool) {
    let mut f = Fixture::new();
    let id = f.reserve();
    let probe = Probe::build();
    let mut supervisor = probe.supervisor(Nonce([0; 32]));
    let (client, stream) = UnixStream::pair().unwrap();
    let role = Role {
        auth: None,
        book: &mut f.book,
        supervisor: &mut supervisor,
    };
    let mut server = Server::new(role, Limits::default()).unwrap();
    let connection = server
        .adopt(stream)
        .unwrap_or_else(|_| panic!("adopt role"));
    server.export_mut().auth = Some(
        RoleAuthentication::new(
            RoleScope {
                server: 1,
                connection,
                session: 3,
                audience: 4,
                uname: b"role".to_vec(),
                aname: b"view".to_vec(),
            },
            f.ingress.clone(),
            Instant::now(),
        )
        .unwrap(),
    );
    let (nonce_sender, nonce_receiver) = mpsc::sync_channel(1);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (resume_sender, resume_receiver) = mpsc::sync_channel(1);
    let client = std::thread::spawn(move || {
        let mut client =
            Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(5)).unwrap();
        let (tag, _) = client.attach(b"role", b"view").unwrap();
        assert_eq!(
            client.wait(tag, deadline()),
            Ok(Reply::Error(Errno::EACCES)),
            "a role root requires a verified afid"
        );
        let (tag, _) = client.auth(b"forged", b"view").unwrap();
        assert_eq!(
            client.wait(tag, deadline()),
            Ok(Reply::Error(Errno::EACCES)),
            "claimed names must select no authority"
        );
        let (tag, afid) = client.auth(b"role", b"view").unwrap();
        assert!(matches!(
            client.wait(tag, deadline()).unwrap(),
            Reply::Auth(_)
        ));
        let (tag, _) = client.attach_with_afid(afid, b"role", b"view").unwrap();
        assert_eq!(
            client.wait(tag, deadline()),
            Ok(Reply::Error(Errno::EACCES)),
            "an unverified afid grants no root"
        );
        let tag = client.write(afid, 0, b"verified").unwrap();
        assert_eq!(
            client.wait(tag, deadline()),
            Ok(Reply::Error(Errno::EACCES)),
            "a peer cannot assert successful verification"
        );
        let tag = client.read(afid, 0, 40).unwrap();
        let Reply::Read(bytes) = client.wait(tag, deadline()).unwrap() else {
            panic!("challenge read");
        };
        nonce_sender
            .send(sophia_factotum::launch::relay::decode_challenge(&bytes).unwrap())
            .unwrap();
        let tag = client.read(afid, 40, 8).unwrap();
        assert_eq!(
            client.wait(tag, deadline()).unwrap(),
            Reply::Read(vec![1, 0, 0, 0, 0, 0, 0, 0])
        );
        if revoke_before_attach {
            ready_sender.send(()).unwrap();
            resume_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let (tag, _) = client.attach_with_afid(afid, b"role", b"view").unwrap();
            assert_eq!(
                client.wait(tag, deadline()),
                Ok(Reply::Error(Errno::ESTALE)),
                "verified status cannot bypass revocation before attach"
            );
            return;
        }
        let (tag, root) = client.attach_with_afid(afid, b"role", b"view").unwrap();
        assert!(matches!(
            client.wait(tag, deadline()).unwrap(),
            Reply::Attach(_)
        ));
        let (tag, api) = client.walk(root, &[b"api"]).unwrap();
        client.wait(tag, deadline()).unwrap();
        let tag = client.lopen(api, 0).unwrap();
        client.wait(tag, deadline()).unwrap();
        let tag = client.read(api, 0, 4).unwrap();
        assert_eq!(
            client.wait(tag, deadline()).unwrap(),
            Reply::Read(b"role".to_vec())
        );
        ready_sender.send(()).unwrap();
        resume_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let tag = client.read(api, 0, 4).unwrap();
        assert_eq!(
            client.wait(tag, deadline()),
            Ok(Reply::Error(Errno::ESTALE)),
            "a retained role fid must lose authority when its launch is revoked"
        );
    });
    let end = deadline();
    let mut spawned = false;
    let mut active = false;
    let mut revoked = false;
    while !client.is_finished() {
        assert!(Instant::now() < end, "joined role authentication deadline");
        f.owner.poll(Instant::now()).unwrap();
        let role = server.export_mut();
        role.book.poll(Instant::now()).unwrap();
        let _ = role.auth().poll(Instant::now());
        if let Ok(nonce) = nonce_receiver.try_recv() {
            *role.supervisor = probe.supervisor(nonce);
            let channel = role.book.take_channel(id, Instant::now()).unwrap();
            role.supervisor.start_with_launch_channel(channel).unwrap();
            spawned = true;
        }
        if spawned && !active {
            active = role
                .book
                .activate(id, role.supervisor, Instant::now())
                .unwrap();
        }
        if ready_receiver.try_recv().is_ok() {
            role.book.revoke(id, Instant::now()).unwrap();
            revoked = true;
            resume_sender.send(()).unwrap();
        }
        server.turn(Some(Duration::ZERO)).unwrap();
        server.flush().unwrap();
        std::thread::yield_now();
    }
    client.join().unwrap();
    assert!(active && revoked);
    if revoke_before_attach {
        assert!(
            server.export_mut().auth().pinned().is_none(),
            "refused attach must not pin identity"
        );
    } else {
        assert_eq!(
            server.export_mut().auth().pinned().unwrap().launch,
            id.generation()
        );
    }
    server.export_mut().supervisor.terminate().unwrap();
}

fn scope() -> RoleScope {
    RoleScope {
        server: 1,
        connection: ConnectionId(2),
        session: 3,
        audience: 4,
        uname: b"role".to_vec(),
        aname: b"view".to_vec(),
    }
}

#[test]
fn resetting_wire_state_does_not_renew_first_attach_deadline() {
    let (ingress, _owner) = queue::channel();
    let now = Instant::now();
    let mut auth = RoleAuthentication::new(scope(), ingress, now).unwrap();
    auth.reset(now + Duration::from_secs(9)).unwrap();
    assert_eq!(
        auth.poll(now + Duration::from_secs(10)),
        Err(Errno::ESTALE),
        "Tversion must not renew the original first-attach deadline"
    );
    assert_eq!(
        auth.begin(3, now + Duration::from_secs(10)),
        Err(Errno::ESTALE)
    );
    assert!(auth.drained());
}

#[test]
fn reread_budget_cancels_the_exact_factotum_attempt() {
    let mut f = Fixture::new();
    let mut auth = RoleAuthentication::new(scope(), f.ingress.clone(), Instant::now()).unwrap();
    let attempt = auth.begin(7, Instant::now()).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/usr/bin/true"),
    );
    let end = deadline();
    let mut reads = 0;
    loop {
        f.tick(end);
        match auth.read(
            attempt,
            0,
            40,
            Instant::now(),
            &mut Authority::Owner {
                book: &mut f.book,
                supervisor: &mut supervisor,
            },
        ) {
            Ok(ReadOutcome::Pending) => {}
            Ok(ReadOutcome::Ready(bytes)) => {
                assert_eq!(bytes.len(), 40);
                reads += 1;
            }
            Err(Errno::ESTALE) => break,
            _ => panic!("unexpected challenge result"),
        }
        assert!(
            reads < 103,
            "rereading the challenge must charge the transcript budget"
        );
    }
    assert_eq!(reads, 101, "the budget includes both fixed relay records");
    while !auth.drained() {
        f.tick(end);
        auth.poll(Instant::now()).unwrap();
    }
    let mut ticket = f
        .ingress
        .submit(Command::Take { attempt }, Instant::now(), end)
        .unwrap();
    loop {
        f.tick(end);
        match ticket.take(Instant::now()) {
            Ok(None) => {}
            Err(sophia_factotum::launch::queue::Error::Remote(Errno::ESTALE)) => break,
            _ => panic!("oversized conversation must cancel its exact Factotum attempt"),
        }
    }
    let next = auth.begin(7, Instant::now()).unwrap();
    assert!(next.attempt_generation > attempt.attempt_generation);
    auth.close(Instant::now());
    while !auth.drained() {
        f.tick(end);
        assert_eq!(auth.poll(Instant::now()), Err(Errno::ESTALE));
    }
}

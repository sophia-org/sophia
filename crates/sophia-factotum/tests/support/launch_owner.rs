//! Actual 9P relay/core/private Factotum exchange. Launch activation and the
//! Session class are supplied; no protected process or final role attach here.
use super::*;
use sophia_9p::Errno;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_factotum::launch::{
    Nonce,
    queue::{self, ConnectionOwner, Error, Ingress, Ticket},
    relay::{self, Relay},
};

struct Fixture {
    _served: Served,
    ingress: Ingress,
    owner: ConnectionOwner,
}
impl Fixture {
    fn new() -> Self {
        let (served, client) = Served::start(None);
        let (ingress, owner) = queue::channel();
        Self {
            _served: served,
            ingress,
            owner: owner.connect(client),
        }
    }
    fn step(&mut self, end: Instant) {
        assert!(Instant::now() < end, "verification owner progress deadline");
        self.owner.poll(Instant::now()).unwrap();
        std::thread::yield_now();
    }
    fn submit(&self, command: Command, end: Instant) -> Ticket {
        self.ingress.submit(command, Instant::now(), end).unwrap()
    }
    fn answer(&mut self, mut ticket: Ticket, end: Instant) -> Result<Response, Error> {
        loop {
            self.step(end);
            match ticket.take(Instant::now()) {
                Ok(None) => {}
                Ok(Some(response)) => return Ok(response),
                Err(error) => return Err(error),
            }
        }
    }
    fn exchange(&mut self, command: Command) -> Result<Response, Error> {
        let end = deadline();
        let ticket = self.submit(command, end);
        self.answer(ticket, end)
    }
    fn initialize(&mut self) {
        assert!(matches!(
            self.exchange(Command::Initialize { session: 3 }),
            Ok(Response::Done)
        ));
        self.step(deadline());
    }
}
fn attempt(n: u64) -> Attempt {
    Attempt {
        server: 1,
        connection: ConnectionId(n),
        session: 3,
        audience: 4,
        wire_generation: 1,
        attempt_generation: 1,
        afid: 7,
    }
}
fn delegation() -> Delegation {
    Delegation {
        identity: Identity {
            principal: 8,
            namespace: 9,
            launch: 10,
        },
        audience: 4,
    }
}

#[test]
fn unread_results_hold_all_verification_slots_until_consumed() {
    let mut f = Fixture::new();
    f.initialize();
    let end = deadline();
    let mut tickets = (1..=queue::CAPACITY)
        .map(|n| {
            f.submit(
                Command::Cancel {
                    attempt: attempt(n as u64),
                },
                end,
            )
        })
        .collect::<Vec<_>>();
    // Complete the last request without consuming earlier results. FIFO
    // execution proves those earlier commands have already received replies.
    let last = tickets.pop().unwrap();
    assert!(matches!(f.answer(last, end), Ok(Response::Done)));
    // Its consumed reservation is reclaimed only on an owner turn.
    f.step(end);
    let extra = f.submit(
        Command::Cancel {
            attempt: attempt(99),
        },
        end,
    );
    for _ in 0..100 {
        f.step(end);
    }
    assert!(
        matches!(
            f.ingress.submit(
                Command::Cancel {
                    attempt: attempt(100)
                },
                Instant::now(),
                end
            ),
            Err(Error::Capacity)
        ),
        "unread verification results must retain every admission slot"
    );
    assert!(matches!(
        tickets[0].take(Instant::now()),
        Ok(Some(Response::Done))
    ));
    f.step(end);
    assert!(
        f.ingress
            .submit(
                Command::Cancel {
                    attempt: attempt(100)
                },
                Instant::now(),
                end
            )
            .is_ok()
    );
    drop(extra);
}

#[test]
fn dropped_challenge_is_cancelled_before_its_slot_is_reused() {
    let mut f = Fixture::new();
    f.initialize();
    let end = deadline();
    let ticket = f.submit(
        Command::Begin {
            attempt: attempt(1),
            remaining_msec: 2000,
        },
        end,
    );
    // A following response proves Begin was executed, while its Challenge is
    // still unread in the first ticket.
    assert!(matches!(
        f.exchange(Command::Cancel {
            attempt: attempt(2)
        }),
        Ok(Response::Done)
    ));
    drop(ticket);
    assert!(
        matches!(
            f.exchange(Command::Take {
                attempt: attempt(1)
            }),
            Err(Error::Remote(Errno::ESTALE))
        ),
        "abandoned challenge must be cancelled in Factotum before reuse"
    );
    assert!(matches!(
        f.exchange(Command::Begin {
            attempt: attempt(1),
            remaining_msec: 1000
        }),
        Ok(Response::Challenge(_))
    ));
}

#[test]
fn dropped_reservation_is_retired_and_cannot_activate() {
    let mut f = Fixture::new();
    f.initialize();
    let ticket = f.submit(Command::Reserve(delegation()), deadline());
    assert!(matches!(
        f.exchange(Command::Cancel {
            attempt: attempt(2)
        }),
        Ok(Response::Done)
    ));
    drop(ticket);
    assert!(
        matches!(
            f.exchange(Command::Activate { launch: 10 }),
            Err(Error::Remote(Errno::ESTALE))
        ),
        "abandoned launch reservation must not become active"
    );
}

#[test]
fn cancelled_queued_work_never_reaches_factotum() {
    let mut f = Fixture::new();
    f.initialize();
    drop(f.submit(Command::Reserve(delegation()), deadline()));
    assert!(matches!(
        f.exchange(Command::Activate { launch: 10 }),
        Err(Error::Remote(Errno::ESTALE))
    ));
    assert!(
        matches!(
            f.exchange(Command::Reserve(delegation())),
            Ok(Response::Done)
        ),
        "queued cancellation must not spend a remote launch generation"
    );
}

#[test]
fn expired_queue_entry_never_gets_a_fresh_attempt_deadline() {
    let mut f = Fixture::new();
    f.initialize();
    let now = Instant::now();
    let mut ticket = f
        .ingress
        .submit(
            Command::Begin {
                attempt: attempt(1),
                remaining_msec: 5000,
            },
            now,
            now + Duration::from_millis(1),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(2));
    assert!(matches!(ticket.take(Instant::now()), Err(Error::Deadline)));
    assert!(matches!(
        f.exchange(Command::Take {
            attempt: attempt(1)
        }),
        Err(Error::Remote(Errno::ESTALE))
    ));
}

#[test]
fn full_verification_ingress_does_not_block_an_admitted_pam_login() {
    let mut f = Fixture::new();
    f.initialize();
    let end = deadline();
    let job = begin_pam(&f._served, f.owner.client_mut(), end);
    let tickets = (1..=queue::CAPACITY)
        .map(|n| {
            f.submit(
                Command::Cancel {
                    attempt: attempt(n as u64),
                },
                end,
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        f.ingress.submit(
            Command::Cancel {
                attempt: attempt(100)
            },
            Instant::now(),
            end
        ),
        Err(Error::Capacity)
    ));
    f._served.finish(job, PamVerdict::Accepted);
    loop {
        f.step(end);
        if let Some(verdict) = f.owner.client_mut().take_login(Instant::now()).unwrap() {
            assert_eq!(
                verdict,
                LoginVerdict::Accepted,
                "admitted PAM must progress with all verification slots occupied"
            );
            break;
        }
    }
    drop(tickets);
}

#[test]
fn owner_loss_refuses_all_existing_and_future_requests() {
    let (ingress, owner) = queue::channel();
    let mut ticket = ingress
        .submit(
            Command::Initialize { session: 3 },
            Instant::now(),
            deadline(),
        )
        .unwrap();
    drop(owner);
    assert!(matches!(ticket.take(Instant::now()), Err(Error::Closed)));
    assert!(matches!(
        ingress.submit(
            Command::Initialize { session: 3 },
            Instant::now(),
            deadline()
        ),
        Err(Error::Closed)
    ));
}

fn relay_child(stream: UnixStream, nonce: Nonce) {
    let mut client =
        Pipeline::over(stream, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let end = deadline();
    let (tag, root) = client.attach(b"", relay::ATTACH).unwrap();
    assert!(matches!(client.wait(tag, end).unwrap(), Reply::Attach(_)));
    for name in [
        b"ctl".as_slice(),
        b"keys",
        b"log",
        b"confirm",
        b"needkey",
        b"pam",
    ] {
        let (tag, _) = client.walk(root, &[name]).unwrap();
        assert!(
            matches!(client.wait(tag, end).unwrap(), Reply::Error(Errno::ENOENT)),
            "provisioned child must not reach Factotum administration"
        );
    }
    let (tag, rpc) = client.walk(root, &[b"rpc"]).unwrap();
    assert!(matches!(client.wait(tag, end).unwrap(), Reply::Walk(_)));
    let tag = client.lopen(rpc, 2).unwrap();
    assert!(matches!(
        client.wait(tag, end).unwrap(),
        Reply::Lopen { .. }
    ));
    let bytes = relay::encode_challenge(nonce);
    let tag = client.write(rpc, 0, &bytes).unwrap();
    assert!(matches!(client.wait(tag, end).unwrap(), Reply::Write(40)));
    let tag = client.read(rpc, 0, 8).unwrap();
    assert!(
        matches!(client.wait(tag, end).unwrap(), Reply::Read(b) if b == [1,0,0,0,0,0,0,0]),
        "registered launch relay must return correlated success"
    );
}

#[test]
fn provisioned_relay_uses_9p_and_owner_registration_not_child_claims() {
    let mut f = Fixture::new();
    f.initialize();
    assert!(matches!(
        f.exchange(Command::Reserve(delegation())),
        Ok(Response::Done)
    ));
    assert!(matches!(
        f.exchange(Command::Activate { launch: 10 }),
        Ok(Response::Done)
    ));
    let Ok(Response::Challenge(nonce)) = f.exchange(Command::Begin {
        attempt: attempt(1),
        remaining_msec: 2000,
    }) else {
        panic!("challenge")
    };
    let (child, stream) = UnixStream::pair().unwrap();
    let mut server = Relay::over(stream, 10, f.ingress.clone()).unwrap();
    assert_eq!(server.export_mut().activate(11), Err(Errno::ESTALE));
    server.export_mut().activate(10).unwrap();
    let thread = std::thread::spawn(move || relay_child(child, nonce));
    let end = deadline();
    while !thread.is_finished() {
        server.turn(Some(Duration::ZERO)).unwrap();
        f.step(end);
    }
    thread.join().unwrap();
    let Ok(Response::Verified(verdict)) = f.exchange(Command::Take {
        attempt: attempt(1),
    }) else {
        panic!("verdict")
    };
    assert_eq!(verdict.identity, delegation().identity);
    assert_eq!(verdict.attempt, attempt(1));
    assert!(
        matches!(
            f.exchange(Command::Relay { launch: 10, nonce }),
            Err(Error::Remote(Errno::ESTALE))
        ),
        "relayed nonce must be single use"
    );
}

#[test]
fn relay_records_refuse_truncation_tail_and_owner_fields() {
    let bytes = relay::encode_challenge(Nonce([7; 32]));
    for n in 0..bytes.len() {
        assert!(relay::decode_challenge(&bytes[..n]).is_err());
    }
    let mut extended = bytes.to_vec();
    extended.extend(10_u64.to_le_bytes());
    assert!(
        relay::decode_challenge(&extended).is_err(),
        "child cannot append a launch identity"
    );
    for index in [0, 4] {
        let mut malformed = bytes;
        malformed[index] = 2;
        assert!(relay::decode_challenge(&malformed).is_err());
    }
}

#[test]
fn reserved_relay_cannot_forward_or_activate_a_foreign_launch() {
    use sophia_9p::{
        OpenFlags,
        export::{Export, ReadOutcome},
    };
    let mut f = Fixture::new();
    f.initialize();
    assert!(matches!(
        f.exchange(Command::Reserve(delegation())),
        Ok(Response::Done)
    ));
    // Factotum's activation answer may arrive before Session publishes its
    // own custody transition. Both checks are required independently.
    assert!(matches!(
        f.exchange(Command::Activate { launch: 10 }),
        Ok(Response::Done)
    ));
    let Ok(Response::Challenge(nonce)) = f.exchange(Command::Begin {
        attempt: attempt(1),
        remaining_msec: 2000,
    }) else {
        panic!("challenge")
    };
    let (_child, stream) = UnixStream::pair().unwrap();
    let mut server = Relay::over(stream, 10, f.ingress.clone()).unwrap();
    let export = server.export_mut();
    let mut handle = export.open(&relay::Node::Rpc, OpenFlags(2)).unwrap();
    export
        .write(
            &relay::Node::Rpc,
            &mut handle,
            0,
            &relay::encode_challenge(nonce),
        )
        .unwrap();
    assert_eq!(
        export.read(&relay::Node::Rpc, &mut handle, 0, 8),
        Ok(ReadOutcome::Pending),
        "Reserved relay must not forward a child's challenge"
    );
    assert!(
        matches!(
            f.exchange(Command::Take {
                attempt: attempt(1)
            }),
            Ok(Response::Pending)
        ),
        "Factotum activation alone must not bypass Reserved Session custody"
    );
    assert_eq!(
        export.activate(11),
        Err(Errno::ESTALE),
        "another spawn cannot activate this relay"
    );
    export.activate(10).unwrap();
    let end = deadline();
    loop {
        f.step(end);
        match export.read(&relay::Node::Rpc, &mut handle, 0, 8).unwrap() {
            ReadOutcome::Pending => {}
            ReadOutcome::Ready(bytes) => {
                assert_eq!(bytes, [1, 0, 0, 0, 0, 0, 0, 0]);
                break;
            }
        }
    }
    assert!(matches!(
        f.exchange(Command::Take {
            attempt: attempt(1)
        }),
        Ok(Response::Verified(_))
    ));
    export.release(relay::Node::Rpc, Some(handle));
    export.revoke();
    assert_eq!(export.activate(10), Err(Errno::ESTALE));
}

#[test]
fn dropping_connected_owner_closes_transport_and_invalidates_buffered_results() {
    let mut f = Fixture::new();
    f.initialize();
    let mut ticket = f.submit(
        Command::Cancel {
            attempt: attempt(1),
        },
        deadline(),
    );
    assert!(matches!(
        f.exchange(Command::Cancel {
            attempt: attempt(2)
        }),
        Ok(Response::Done)
    ));
    drop(f.owner);
    f._served
        .closed
        .recv_timeout(Duration::from_secs(2))
        .expect("dropping verification owner must close its actual transport");
    assert!(
        matches!(ticket.take(Instant::now()), Err(Error::Closed)),
        "connection loss invalidates even an already buffered result"
    );
}

#[test]
fn reserved_relay_deadline_progresses_without_factotum_or_peer_activity() {
    let (ingress, _owner) = queue::channel();
    let (child, stream) = UnixStream::pair().unwrap();
    let mut server = Relay::over(stream, 10, ingress).unwrap();
    let thread = std::thread::spawn(move || {
        let mut client =
            Pipeline::over(child, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
        let end = Instant::now() + Duration::from_secs(8);
        let (tag, root) = client.attach(b"", relay::ATTACH).unwrap();
        assert!(matches!(client.wait(tag, end).unwrap(), Reply::Attach(_)));
        let (tag, rpc) = client.walk(root, &[b"rpc"]).unwrap();
        assert!(matches!(client.wait(tag, end).unwrap(), Reply::Walk(_)));
        let tag = client.lopen(rpc, 2).unwrap();
        assert!(matches!(
            client.wait(tag, end).unwrap(),
            Reply::Lopen { .. }
        ));
        let tag = client
            .write(rpc, 0, &relay::encode_challenge(Nonce([8; 32])))
            .unwrap();
        assert!(matches!(client.wait(tag, end).unwrap(), Reply::Write(40)));
        let tag = client.read(rpc, 0, 8).unwrap();
        assert!(
            matches!(client.wait(tag, end).unwrap(), Reply::Error(Errno::ESTALE)),
            "silent Reserved relay must expire without peer or Factotum progress"
        );
    });
    let end = Instant::now() + Duration::from_secs(9);
    while !thread.is_finished() {
        assert!(Instant::now() < end, "Reserved relay expiry deadline");
        server.turn(Some(Duration::from_millis(100))).unwrap();
    }
    thread.join().unwrap();
}

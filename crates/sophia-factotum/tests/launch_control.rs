//! The private owner exchange uses the actual 9P server and Rust pipeline.
//! Session class and delegation facts are supplied; no protected spawn is claimed.

use sophia_9p::{
    ConnectionId, Errno, Fid, Limits,
    authentication::{Attempt, Identity},
    unix::{Server, Wake},
};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_factotum::{
    export::FactotumExport,
    launch::{
        Delegation, Nonce, Verdict,
        control::LaunchControl,
        wire::{self, Command, Response},
    },
    proto::{ChannelClass, Settings},
};
use std::{
    os::unix::net::UnixStream,
    thread::JoinHandle,
    time::{Duration, Instant},
};

fn attempt() -> Attempt {
    Attempt {
        server: 1,
        connection: ConnectionId(2),
        session: 3,
        audience: 4,
        wire_generation: 5,
        attempt_generation: 6,
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
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

struct Served {
    wake: Wake,
    thread: Option<JoinHandle<()>>,
}
impl Served {
    fn start(class: ChannelClass) -> (Self, Pipeline) {
        let (client, stream) = UnixStream::pair().unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let thread = Some(std::thread::spawn(move || {
            let export = FactotumExport::new(Settings {
                owner: "fixture".into(),
                pam_service: "unused".into(),
            });
            let mut server = Server::new(export, Limits::default()).unwrap();
            let id = server.adopt(stream).unwrap_or_else(|_| panic!("adopt"));
            server.export_mut().admit(id, class);
            sender.send(server.wake()).unwrap();
            server.run().unwrap();
        }));
        let wake = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let served = Self { wake, thread };
        let client =
            Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
        (served, client)
    }
}
impl Drop for Served {
    fn drop(&mut self) {
        self.wake.stop();
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}
fn attach(client: &mut Pipeline) -> Fid {
    let (tag, root) = client.attach(b"", wire::ATTACH).unwrap();
    assert!(matches!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    root
}
fn open(client: &mut Pipeline, root: Fid) -> Fid {
    let (tag, rpc) = client.walk(root, &[b"rpc"]).unwrap();
    assert!(matches!(client.wait(tag, deadline()).unwrap(), Reply::Walk(ref v) if v.len() == 1));
    let tag = client.lopen(rpc, 2).unwrap();
    assert!(matches!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Lopen { .. }
    ));
    rpc
}
fn exchange(client: &mut Pipeline, rpc: Fid, command: Command) -> Result<Response, Errno> {
    let bytes = command.encode();
    let tag = client.write(rpc, 0, &bytes).unwrap();
    match client.wait(tag, deadline()).unwrap() {
        Reply::Write(n) => assert_eq!(n as usize, bytes.len()),
        Reply::Error(e) => return Err(e),
        other => panic!("write: {other:?}"),
    }
    let tag = client.read(rpc, 0, wire::MAX_RECORD_BYTES as u32).unwrap();
    let Reply::Read(bytes) = client.wait(tag, deadline()).unwrap() else {
        panic!("read")
    };
    Response::decode(&bytes)
}
fn done(client: &mut Pipeline, rpc: Fid, command: Command) {
    assert!(matches!(exchange(client, rpc, command), Ok(Response::Done)));
}

#[test]
fn real_control_exchange_uses_registered_delegation_and_single_use_nonce() {
    let (_served, mut client) = Served::start(ChannelClass::Session);
    let root = attach(&mut client);
    let rpc = open(&mut client, root);
    let a = attempt();
    let d = delegation();
    let tag = client
        .write(rpc, 1, &Command::Initialize { session: a.session }.encode())
        .unwrap();
    assert_eq!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno::EINVAL)
    );
    done(&mut client, rpc, Command::Initialize { session: a.session });
    done(&mut client, rpc, Command::Reserve(d));
    let nonce = match exchange(
        &mut client,
        rpc,
        Command::Begin {
            attempt: a,
            remaining_msec: 5000,
        },
    ) {
        Ok(Response::Challenge(n)) => n,
        _ => panic!("challenge"),
    };
    assert_eq!(
        exchange(
            &mut client,
            rpc,
            Command::Relay {
                launch: d.identity.launch,
                nonce
            }
        )
        .err(),
        Some(Errno::EAGAIN),
        "the private exchange must not verify a Reserved launch"
    );
    assert!(matches!(
        exchange(&mut client, rpc, Command::Take { attempt: a }),
        Ok(Response::Pending)
    ));
    done(
        &mut client,
        rpc,
        Command::Activate {
            launch: d.identity.launch,
        },
    );
    done(
        &mut client,
        rpc,
        Command::Relay {
            launch: d.identity.launch,
            nonce,
        },
    );
    assert_eq!(
        exchange(
            &mut client,
            rpc,
            Command::Relay {
                launch: d.identity.launch,
                nonce
            }
        )
        .err(),
        Some(Errno::ESTALE)
    );
    match exchange(&mut client, rpc, Command::Take { attempt: a }) {
        Ok(Response::Verified(v)) => assert_eq!(
            v,
            Verdict {
                attempt: a,
                identity: d.identity
            }
        ),
        _ => panic!("only Factotum's correlated verdict completes the exchange"),
    }
    assert_eq!(
        exchange(&mut client, rpc, Command::Take { attempt: a }).err(),
        Some(Errno::ESTALE)
    );
    done(
        &mut client,
        rpc,
        Command::Retire {
            launch: d.identity.launch,
        },
    );
    assert_eq!(
        exchange(&mut client, rpc, Command::Reserve(d)).err(),
        Some(Errno::ESTALE)
    );
}

#[test]
fn control_view_is_session_only_exclusive_and_has_no_key_or_pam_path() {
    let (_served, mut user) = Served::start(ChannelClass::User);
    let (tag, _) = user.attach(b"", wire::ATTACH).unwrap();
    assert_eq!(
        user.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno::EACCES)
    );
    let (_served, mut client) = Served::start(ChannelClass::Session);
    let root = attach(&mut client);
    for name in [
        b"ctl".as_slice(),
        b"keys",
        b"log",
        b"confirm",
        b"needkey",
        b"proto",
        b"factotum",
    ] {
        let (tag, _) = client.walk(root, &[name]).unwrap();
        assert_eq!(
            client.wait(tag, deadline()).unwrap(),
            Reply::Error(Errno::ENOENT)
        );
    }
    let rpc = open(&mut client, root);
    let (tag, second) = client.walk(root, &[b"rpc"]).unwrap();
    assert!(matches!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));
    let tag = client.lopen(second, 2).unwrap();
    assert_eq!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno(16))
    );
    let tag = client.write(rpc, 0, b"start proto=pam role=login").unwrap();
    assert_eq!(
        client.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno::EINVAL)
    );
    done(&mut client, rpc, Command::Initialize { session: 3 });
    done(&mut client, rpc, Command::Reserve(delegation()));
    let tag = client.clunk(rpc).unwrap();
    assert_eq!(client.wait(tag, deadline()).unwrap(), Reply::Clunk);
    let rpc = open(&mut client, root);
    done(&mut client, rpc, Command::Initialize { session: 3 });
    assert_eq!(
        exchange(&mut client, rpc, Command::Activate { launch: 10 }).err(),
        Some(Errno::ESTALE),
        "clunk must drop the previous control owner's delegations"
    );
}

#[test]
fn unread_response_refuses_new_mutation_and_short_read_preserves_response() {
    let mut c = LaunchControl::default();
    c.write(&Command::Initialize { session: 3 }.encode())
        .unwrap();
    assert_eq!(
        c.write(&Command::Reserve(delegation()).encode()),
        Err(Errno::EAGAIN)
    );
    assert_eq!(c.read(7), Err(Errno::EINVAL));
    assert!(matches!(
        Response::decode(&c.read(84).unwrap()),
        Ok(Response::Done)
    ));
    assert_eq!(
        c.write(&Command::Activate { launch: 10 }.encode()),
        Err(Errno::ESTALE)
    );
    c.write(&Command::Reserve(delegation()).encode()).unwrap();
    c.read(84).unwrap();
    c.write(&Command::Activate { launch: 10 }.encode()).unwrap();
}

#[test]
fn every_control_record_refuses_truncation_extra_bytes_and_unknown_version() {
    let a = attempt();
    let nonce = Nonce([42; 32]);
    let commands = [
        Command::Initialize { session: 3 },
        Command::Reserve(delegation()),
        Command::Activate { launch: 10 },
        Command::Retire { launch: 10 },
        Command::Begin {
            attempt: a,
            remaining_msec: 5000,
        },
        Command::Relay { launch: 10, nonce },
        Command::Take { attempt: a },
        Command::Cancel { attempt: a },
    ];
    for command in commands {
        let bytes = command.encode();
        assert_eq!(
            Command::decode(&bytes)
                .unwrap_or_else(|_| panic!("decode"))
                .encode(),
            bytes
        );
        for length in 0..bytes.len() {
            assert!(Command::decode(&bytes[..length]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Command::decode(&extra).is_err());
        let mut version = bytes.clone();
        version[0] = 2;
        assert!(Command::decode(&version).is_err());
    }
    for response in [
        Response::Done,
        Response::Challenge(nonce),
        Response::Pending,
        Response::Verified(Verdict {
            attempt: a,
            identity: delegation().identity,
        }),
    ] {
        let bytes = response.encode();
        assert!(bytes.len() <= wire::MAX_RECORD_BYTES);
        assert_eq!(
            Response::decode(&bytes)
                .unwrap_or_else(|_| panic!("decode"))
                .encode(),
            bytes
        );
        for length in 0..bytes.len() {
            assert!(Response::decode(&bytes[..length]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Response::decode(&extra).is_err());
    }
    // Independent field expectations, not just encoder/decoder round trips.
    assert_eq!(
        &Command::Begin {
            attempt: a,
            remaining_msec: 5000
        }
        .encode()[..24],
        &[
            1, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0
        ]
    );
}

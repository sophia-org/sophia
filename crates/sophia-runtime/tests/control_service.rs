use sophia_protocol::*;
use sophia_runtime::*;
use std::io::{Read, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture {
    service: ControlService,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let service = ControlService::bind(&root).unwrap();
        let catalog = Arc::new(catalog(7));
        while !service.publish(catalog.clone(), &[]) {
            std::thread::yield_now();
        }
        Self { service, root }
    }
    fn raw(&self) -> UnixStream {
        let mut stream = UnixStream::connect(self.service.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        send(
            &mut stream,
            0,
            ControlMessage::Hello {
                minimum_revision: 1,
                maximum_revision: 1,
                required_features: 0,
            },
        );
        assert!(matches!(read(&mut stream).1, ControlMessage::Welcome(_)));
        stream
    }
    fn ticket(&self) -> ControlTicket {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(ticket) = self.service.try_request() {
                return ticket;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn catalog(generation: u64) -> ControlCatalog {
    ControlCatalog {
        generation,
        commands: vec![action()],
    }
}
fn action() -> ControlCommand {
    ControlCommand {
        owner: ControlOwner::Policy,
        name: "focus-next".into(),
    }
}
fn send(stream: &mut UnixStream, id: u64, message: ControlMessage) {
    stream
        .write_all(&encode_control_frame(id, &message).unwrap())
        .unwrap();
}
fn read(stream: &mut UnixStream) -> (u64, ControlMessage) {
    let mut frame = vec![0; 24];
    stream.read_exact(&mut frame).unwrap();
    let (_, _, length) = decode_control_header(&frame).unwrap();
    frame.resize(24 + length, 0);
    stream.read_exact(&mut frame[24..]).unwrap();
    decode_control_frame(&frame).unwrap()
}
fn claim(ticket: &ControlTicket) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ticket.claim() {
        assert!(!ticket.cancelled());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn real_endpoint_client_waits_for_owner_settlement_and_reuses_connection() {
    let f = Fixture::new();
    let path = f.service.socket_path().to_path_buf();
    std::thread::scope(|scope| {
        let client = scope.spawn(|| {
            let mut client = ControlClient::connect(&path).unwrap();
            assert_eq!(client.commands().unwrap(), catalog(7));
            assert_eq!(
                client.invoke(action()).unwrap().1,
                ControlOutcome::Committed
            );
            assert_eq!(client.commands().unwrap(), catalog(7));
        });
        let ticket = f.ticket();
        claim(&ticket);
        std::thread::sleep(Duration::from_millis(30));
        assert!(!client.is_finished());
        ticket.finish(ControlOutcome::Committed);
        client.join().unwrap();
    });
}

#[test]
fn stale_and_unknown_commands_never_reach_owner() {
    let f = Fixture::new();
    let mut stream = f.raw();
    send(&mut stream, 1, ControlMessage::Commands);
    read(&mut stream);
    while !f.service.publish(Arc::new(catalog(8)), &[]) {
        std::thread::yield_now();
    }
    send(
        &mut stream,
        2,
        ControlMessage::Invoke {
            generation: 7,
            command: action(),
        },
    );
    assert!(matches!(
        read(&mut stream).1,
        ControlMessage::Outcome {
            outcome: ControlOutcome::Stale,
            ..
        }
    ));
    send(&mut stream, 3, ControlMessage::Commands);
    read(&mut stream);
    send(
        &mut stream,
        4,
        ControlMessage::Invoke {
            generation: 8,
            command: ControlCommand {
                owner: ControlOwner::Policy,
                name: "absent".into(),
            },
        },
    );
    assert!(matches!(
        read(&mut stream).1,
        ControlMessage::Outcome {
            outcome: ControlOutcome::Rejected,
            ..
        }
    ));
    assert!(f.service.try_request().is_none());
}

#[test]
fn duplicate_id_and_pipelining_close_only_the_offender() {
    let f = Fixture::new();
    let mut stream = f.raw();
    send(&mut stream, 1, ControlMessage::Commands);
    read(&mut stream);
    send(&mut stream, 1, ControlMessage::Commands);
    assert!(matches!(
        read(&mut stream).1,
        ControlMessage::ProtocolError { code: 2 }
    ));
    let mut stream = f.raw();
    let mut pipelined = encode_control_frame(1, &ControlMessage::Commands).unwrap();
    pipelined.extend(encode_control_frame(2, &ControlMessage::Commands).unwrap());
    stream.write_all(&pipelined).unwrap();
    assert_eq!(stream.read(&mut [0; 1]).unwrap_or(0), 0);
    assert_eq!(
        ControlClient::connect(f.service.socket_path())
            .unwrap()
            .commands()
            .unwrap(),
        catalog(7)
    );
}

#[test]
fn fresh_dispatch_authorization_rejects_newly_excluded_peer() {
    let f = Fixture::new();
    let mut stream = f.raw();
    send(&mut stream, 1, ControlMessage::Commands);
    read(&mut stream);
    send(
        &mut stream,
        2,
        ControlMessage::Invoke {
            generation: 7,
            command: action(),
        },
    );
    let ticket = f.ticket();
    while !f
        .service
        .publish(Arc::new(catalog(7)), &[std::process::id()])
    {
        std::thread::yield_now();
    }
    assert!(!ticket.claim());
    assert!(matches!(
        read(&mut stream).1,
        ControlMessage::Outcome {
            outcome: ControlOutcome::Denied,
            ..
        }
    ));
    assert!(ticket.cancelled());
    assert!(!ticket.claim());
}

#[test]
fn disconnect_before_dispatch_cancels_ticket() {
    let f = Fixture::new();
    let mut stream = f.raw();
    send(&mut stream, 1, ControlMessage::Commands);
    read(&mut stream);
    send(
        &mut stream,
        2,
        ControlMessage::Invoke {
            generation: 7,
            command: action(),
        },
    );
    let ticket = f.ticket();
    drop(stream);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ticket.cancelled() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!ticket.claim());
}

#[test]
fn incomplete_frame_deadline_does_not_starve_other_clients() {
    let f = Fixture::new();
    let mut slow = UnixStream::connect(f.service.socket_path()).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    slow.write_all(b"S").unwrap();
    for _ in 0..20 {
        assert_eq!(
            ControlClient::connect(f.service.socket_path())
                .unwrap()
                .commands()
                .unwrap()
                .generation,
            7
        );
    }
    assert_eq!(slow.read(&mut [0; 1]).unwrap(), 0);
}

#[test]
fn all_sixteen_pending_slots_are_bounded_and_overflow_is_explicit() {
    let f = Fixture::new();
    let mut peers = Vec::new();
    let mut tickets = Vec::new();
    for _ in 0..16 {
        let mut stream = f.raw();
        send(&mut stream, 1, ControlMessage::Commands);
        read(&mut stream);
        send(
            &mut stream,
            2,
            ControlMessage::Invoke {
                generation: 7,
                command: action(),
            },
        );
        tickets.push(f.ticket());
        peers.push(stream);
    }
    let mut client = ControlClient::connect(f.service.socket_path()).unwrap();
    assert_eq!(
        client.invoke(action()).unwrap().1,
        ControlOutcome::Overloaded
    );
    for ticket in tickets {
        claim(&ticket);
        ticket.finish(ControlOutcome::Committed);
    }
    for mut peer in peers {
        assert!(matches!(
            read(&mut peer).1,
            ControlMessage::Outcome {
                outcome: ControlOutcome::Committed,
                ..
            }
        ));
    }
}

#[test]
fn unsupported_revision_and_features_receive_terminal_errors() {
    let f = Fixture::new();
    for (revision, features, expected) in [(3, 0, 3), (1, 1, 4)] {
        let mut stream = UnixStream::connect(f.service.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        send(
            &mut stream,
            0,
            ControlMessage::Hello {
                minimum_revision: revision,
                maximum_revision: revision,
                required_features: features,
            },
        );
        assert_eq!(
            read(&mut stream).1,
            ControlMessage::ProtocolError { code: expected }
        );
    }
}

#[test]
fn independent_python_client_uses_the_real_endpoint() {
    let f = Fixture::new();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bindings/python/sophia_control_v1.py");
    let child = std::process::Command::new("python3")
        .arg(script)
        .arg("--socket")
        .arg(f.service.socket_path())
        .args(["policy", "focus-next"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let ticket = f.ticket();
    claim(&ticket);
    ticket.finish(ControlOutcome::Committed);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"outcome\": \"committed\""));
}

#[test]
fn ancillary_rights_are_rejected_without_owner_work() {
    let f = Fixture::new();
    let result = std::process::Command::new("python3")
        .args([
            "-c",
            r#"
import socket,os,array,sys
s=socket.socket(socket.AF_UNIX); s.settimeout(3); s.connect(sys.argv[1])
fd=os.open('/dev/null',os.O_RDONLY)
s.sendmsg([b'S'],[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[fd]))]); os.close(fd)
try: assert s.recv(1)==b''
except ConnectionResetError: pass
"#,
        ])
        .arg(f.service.socket_path())
        .status()
        .unwrap();
    assert!(result.success());
    assert!(f.service.try_request().is_none());
}

#[test]
fn sandbox_with_socket_path_still_fails_namespace_admission() {
    if std::env::var_os("SOPHIA_CONTROL_SANDBOX_PROOF").is_none() {
        return;
    }
    let f = Fixture::new();
    let result = std::process::Command::new("/usr/bin/bwrap")
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "python3",
            "-c",
            r#"
import socket,sys,struct
s=socket.socket(socket.AF_UNIX); s.settimeout(3); s.connect(sys.argv[1])
hello=struct.pack('<4sHHQIIHHQ',b'SOPH',1,128,0,12,0,1,1,0)
try:
 s.sendall(hello)
 assert s.recv(1)==b''
except (ConnectionResetError,BrokenPipeError): pass
"#,
        ])
        .arg(f.service.socket_path())
        .status()
        .unwrap();
    assert!(
        result.success(),
        "sandbox denial proof requires working user namespaces and bubblewrap"
    );
    assert!(f.service.try_request().is_none());
}

#[test]
fn queued_and_dispatched_deadlines_have_distinct_outcomes() {
    let f = Fixture::new();
    let mut queued = f.raw();
    let mut dispatched = f.raw();
    queued
        .set_read_timeout(Some(Duration::from_secs(12)))
        .unwrap();
    dispatched
        .set_read_timeout(Some(Duration::from_secs(12)))
        .unwrap();
    for stream in [&mut queued, &mut dispatched] {
        send(stream, 1, ControlMessage::Commands);
        read(stream);
        send(
            stream,
            2,
            ControlMessage::Invoke {
                generation: 7,
                command: action(),
            },
        );
    }
    let a = f.ticket();
    let b = f.ticket();
    let (first, second) = if a.connection < b.connection {
        (a, b)
    } else {
        (b, a)
    };
    claim(&second);
    assert!(matches!(
        read(&mut queued).1,
        ControlMessage::Outcome {
            outcome: ControlOutcome::TimedOut,
            ..
        }
    ));
    assert!(matches!(
        read(&mut dispatched).1,
        ControlMessage::Outcome {
            outcome: ControlOutcome::Indeterminate,
            ..
        }
    ));
    assert!(first.cancelled());
    assert!(!first.claim());
    second.finish(ControlOutcome::Committed);
}

#[test]
fn connection_limit_closes_excess_peer_and_recovers_capacity() {
    let f = Fixture::new();
    let mut peers = (0..32).map(|_| f.raw()).collect::<Vec<_>>();
    let mut excess = UnixStream::connect(f.service.socket_path()).unwrap();
    excess
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert_eq!(excess.read(&mut [0]).unwrap_or(0), 0);
    peers.pop();
    // A service wake forces a turn that observes the disconnected peer.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match ControlClient::connect(f.service.socket_path()) {
            Ok(mut client) => {
                assert_eq!(client.commands().unwrap(), catalog(7));
                break;
            }
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    assert!(f.service.try_request().is_none());
}

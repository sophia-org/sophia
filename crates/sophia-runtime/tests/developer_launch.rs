#![cfg(target_os = "linux")]
#[path = "support/inspection_wire.rs"]
mod wire;
use sophia_protocol::developer_launch::*;
use sophia_runtime::developer_launch::*;
use std::os::unix::{fs::PermissionsExt, net::UnixStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use wire::*;

struct Fixture {
    service: DeveloperLaunchService,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dl-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            service: DeveloperLaunchService::bind(&root).unwrap(),
            root,
        }
    }
    fn connect(&self) -> UnixStream {
        let mut stream = UnixStream::connect(self.service.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert_eq!(call(&mut stream, &version())[4], 101);
        assert_eq!(call(&mut stream, &attachment(b"", b""))[4], 105);
        stream
    }
    fn ticket(&self) -> DeveloperLaunchTicket {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(ticket) = self.service.try_request() {
                return ticket;
            }
            assert!(
                Instant::now() < deadline,
                "launch request delivery deadline"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn attachment(uname: &[u8], aname: &[u8]) -> Vec<u8> {
    let mut b = 0u32.to_le_bytes().to_vec();
    b.extend(u32::MAX.to_le_bytes());
    string(&mut b, uname);
    string(&mut b, aname);
    b.extend(u32::MAX.to_le_bytes());
    frame(104, 1, &b)
}
fn write(fid: u32, offset: u64, data: &[u8]) -> Vec<u8> {
    let mut b = fid.to_le_bytes().to_vec();
    b.extend(offset.to_le_bytes());
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    frame(118, 8, &b)
}
fn writer(s: &mut UnixStream, fid: u32, name: &[u8]) {
    assert_eq!(call(s, &walk(2, fid, name))[4], 111);
    assert_eq!(call(s, &open(3, fid, 1))[4], 13);
}
fn candidate() -> Vec<u8> {
    encode_developer_launch_request(&DeveloperLaunchRequest {
        version: 1,
        request: 17,
        recipe: "tools".into(),
        executable: "/usr/bin/true".into(),
        arguments: vec![],
    })
    .unwrap()
}
fn submit(s: &mut UnixStream) {
    writer(s, 1, b"request");
    writer(s, 2, b"submit");
    assert_eq!(call(s, &write(1, 0, &candidate()))[4], 119);
    assert_eq!(call(s, &write(2, 0, &17u32.to_le_bytes()))[4], 119);
}
fn claim(t: &DeveloperLaunchTicket) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !t.claim() {
        assert!(!t.cancelled());
        assert!(Instant::now() < deadline, "fresh admission claim deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn submission_is_not_readiness_and_retries_cannot_launch_twice() {
    let f = Fixture::new();
    let mut s = f.connect();
    writer(&mut s, 1, b"request");
    writer(&mut s, 2, b"submit");
    assert_eq!(call(&mut s, &write(1, 0, &candidate()))[4], 119);
    assert!(
        f.service.try_request().is_none(),
        "candidate has no launch effect"
    );
    assert_eq!(call(&mut s, &write(2, 0, &17u32.to_le_bytes()))[4], 119);
    let t = f.ticket();
    assert_eq!(call(&mut s, &walk(2, 3, b"result"))[4], 111);
    assert_eq!(
        errno(&call(&mut s, &open(3, 3, 0))),
        11,
        "submitted is not Started"
    );
    assert!(
        !t.finish(DeveloperLaunchStatus::Started, Some(123)),
        "unclaimed request cannot report Started"
    );
    assert_eq!(call(&mut s, &write(2, 0, &17u32.to_le_bytes()))[4], 119);
    assert!(
        f.service.try_request().is_none(),
        "submit replay cannot enqueue another launch"
    );
    assert_eq!(
        errno(&call(&mut s, &write(2, 0, &18u32.to_le_bytes()))),
        114
    );
    claim(&t);
    assert!(t.finish(DeveloperLaunchStatus::Started, Some(123)));
    assert!(!t.finish(DeveloperLaunchStatus::Failed, None));
    assert_eq!(call(&mut s, &open(3, 3, 0))[4], 13);
    let bytes = body(&call(&mut s, &read(4, 3, 0, 256))).to_vec();
    assert_eq!(
        decode_developer_launch_result(&bytes).unwrap(),
        DeveloperLaunchResult {
            version: 1,
            request: 17,
            status: DeveloperLaunchStatus::Started,
            process: Some(123)
        }
    );
    assert_eq!(body(&call(&mut s, &read(4, 3, 0, 256))), bytes);
    assert_eq!(call(&mut s, &version())[4], 101);
    assert_eq!(call(&mut s, &attachment(b"", b""))[4], 105);
    writer(&mut s, 2, b"submit");
    assert_eq!(call(&mut s, &write(2, 0, &17u32.to_le_bytes()))[4], 119);
    assert!(
        f.service.try_request().is_none(),
        "Tversion cannot renew launch budget"
    );
}

#[test]
fn incomplete_candidates_and_writer_clunk_never_submit() {
    let f = Fixture::new();
    let mut s = f.connect();
    writer(&mut s, 1, b"request");
    writer(&mut s, 2, b"submit");
    let bytes = candidate();
    assert_eq!(
        errno(&call(&mut s, &write(1, 0, &bytes[..bytes.len() - 1]))),
        22
    );
    assert_eq!(errno(&call(&mut s, &write(1, 1, &bytes))), 22);
    assert_eq!(errno(&call(&mut s, &write(2, 0, &17u32.to_le_bytes()))), 22);
    assert_eq!(call(&mut s, &write(1, 0, &bytes))[4], 119);
    assert_eq!(call(&mut s, &clunk(9, 1))[4], 121);
    assert_eq!(errno(&call(&mut s, &write(2, 0, &17u32.to_le_bytes()))), 22);
    assert!(f.service.try_request().is_none());
}

#[test]
fn disconnect_cancels_unclaimed_work_but_not_dispatched_custody() {
    for dispatched in [false, true] {
        let f = Fixture::new();
        let mut s = f.connect();
        submit(&mut s);
        let t = f.ticket();
        if dispatched {
            claim(&t);
        }
        drop(s);
        let deadline = Instant::now() + Duration::from_secs(3);
        if !dispatched {
            while !t.cancelled() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(!t.claim(), "disconnected queued launch cannot be claimed");
        } else {
            assert!(t.dispatched());
            assert!(t.finish(DeveloperLaunchStatus::Started, Some(123)));
        }
    }
}

#[test]
fn claimed_identity_strings_cannot_select_administration() {
    let f = Fixture::new();
    let mut s = f.connect();
    assert_eq!(call(&mut s, &version())[4], 101);
    assert_eq!(errno(&call(&mut s, &attachment(b"admin", b"tools"))), 13);
    assert!(f.service.try_request().is_none());
}

#[test]
fn unclaimed_requests_expire_without_becoming_dispatchable() {
    let f = Fixture::new();
    let mut s = f.connect();
    submit(&mut s);
    let ticket = f.ticket();
    let deadline = ticket.received + DEVELOPER_LAUNCH_ADMISSION_TIMEOUT + Duration::from_secs(2);
    while !ticket.cancelled() {
        assert!(
            Instant::now() < deadline,
            "unclaimed launch must expire within its admission bound"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(ticket.received.elapsed() >= DEVELOPER_LAUNCH_ADMISSION_TIMEOUT);
    assert!(
        !ticket.claim(),
        "expired launch must never become dispatchable"
    );
    opened(&mut s, 3, b"result");
    let result = decode_developer_launch_result(body(&call(&mut s, &read(4, 3, 0, 256)))).unwrap();
    assert_eq!(result.status, DeveloperLaunchStatus::Unavailable);
}

#[test]
fn connection_capacity_is_bounded_and_reclaimed_after_disconnect() {
    let f = Fixture::new();
    let mut clients = Vec::new();
    let mut tickets = Vec::new();
    for _ in 0..DEVELOPER_LAUNCH_MAX_PEERS {
        let mut s = f.connect();
        submit(&mut s);
        tickets.push(f.ticket());
        clients.push(s);
    }
    let mut extra = UnixStream::connect(f.service.socket_path()).unwrap();
    extra
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = extra.write_all(&version());
    assert!(
        receive(&mut extra).is_err(),
        "full launch capacity cannot admit another peer"
    );
    drop(clients.pop());
    let retired = tickets.pop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !retired.cancelled() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut fresh = f.connect();
    submit(&mut fresh);
    let replacement = f.ticket();
    claim(&replacement);
    assert!(replacement.finish(DeveloperLaunchStatus::Denied, None));
}

#[test]
fn host_domain_refuses_same_uid_in_another_namespace() {
    let f = Fixture::new();
    let output = std::process::Command::new("/usr/bin/bwrap")
        .args([
            "--die-with-parent",
            "--unshare-user",
            "--unshare-pid",
            "--ro-bind",
            "/",
            "/",
            "--",
        ])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "foreign_host_namespace_child",
            "--nocapture",
        ])
        .env("SOPHIA_TEST_LAUNCH_SOCKET", f.service.socket_path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("foreign launch admission refused"));
    assert!(f.service.try_request().is_none());
}
#[test]
#[ignore = "child invoked by host-domain proof"]
fn foreign_host_namespace_child() {
    let mut s =
        UnixStream::connect(std::env::var_os("SOPHIA_TEST_LAUNCH_SOCKET").unwrap()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let _ = s.write_all(&version());
    assert!(
        receive(&mut s).is_err(),
        "same UID in foreign namespaces must not enter the launch export"
    );
    println!("foreign launch admission refused");
}

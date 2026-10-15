#![allow(dead_code)]
#[path = "inspection_wire.rs"]
pub mod wire;
pub use sophia_protocol::admin_files::*;
pub use sophia_protocol::{ControlCatalog, ControlCommand, ControlOutcome, ControlOwner};
pub use sophia_runtime::{ControlService, ControlTicket};
use std::os::unix::fs::PermissionsExt;
pub use std::os::unix::net::UnixStream;
pub use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
pub use std::time::{Duration, Instant};
pub use wire::*;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct Fixture {
    pub service: ControlService,
    root: std::path::PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "saf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let service = ControlService::bind_files(&root).unwrap();
        let fixture = Self { service, root };
        fixture.publish(7, &[]);
        fixture
    }
    pub fn publish(&self, generation: u64, excluded: &[u32]) {
        let catalog = Arc::new(catalog(generation));
        until(|| self.service.publish(catalog.clone(), excluded));
    }
    pub fn peer(&self) -> Client {
        Client::new(self.service.socket_path())
    }
    pub fn ticket(&self) -> ControlTicket {
        let mut ticket = None;
        until(|| {
            ticket = self.service.try_request();
            ticket.is_some()
        });
        ticket.unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn catalog(generation: u64) -> ControlCatalog {
    ControlCatalog {
        generation,
        commands: vec![action()],
    }
}
pub fn action() -> ControlCommand {
    ControlCommand {
        owner: ControlOwner::Policy,
        name: "focus next".into(),
    }
}
pub fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !check() {
        assert!(Instant::now() < deadline, "condition did not become true");
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub fn claim(ticket: &ControlTicket) {
    until(|| {
        assert!(!ticket.cancelled());
        ticket.claim()
    });
}
pub fn write(tag: u16, fid: u32, offset: u64, bytes: &[u8]) -> Vec<u8> {
    let mut request = fid.to_le_bytes().to_vec();
    request.extend_from_slice(&offset.to_le_bytes());
    request.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    request.extend_from_slice(bytes);
    frame(118, tag, &request)
}
pub struct Client {
    pub stream: UnixStream,
    pub info: AdminInfo,
}
impl Client {
    pub fn new(path: &std::path::Path) -> Self {
        let mut stream = connect(path);
        opened(&mut stream, 1, b"info");
        let info = decode_admin_info(body(&call(&mut stream, &read(4, 1, 0, 512)))).unwrap();
        assert_eq!(call(&mut stream, &clunk(5, 1))[4], 121);
        let mut client = Self { stream, info };
        assert_eq!(client.catalog(), catalog(7));
        client.open_ctl();
        client
    }
    pub fn open_ctl(&mut self) {
        assert_eq!(call(&mut self.stream, &walk(2, 2, b"ctl"))[4], 111);
        assert_eq!(call(&mut self.stream, &open(3, 2, 1))[4], 13);
    }
    pub fn catalog(&mut self) -> ControlCatalog {
        opened(&mut self.stream, 1, b"commands");
        let result =
            decode_admin_catalog(body(&call(&mut self.stream, &read(4, 1, 0, 65500)))).unwrap();
        assert_eq!(call(&mut self.stream, &clunk(5, 1))[4], 121);
        result
    }
    pub fn invocation(&self, request: u64, generation: u64) -> Vec<u8> {
        encode_admin_invocation(&AdminInvocation {
            connection: self.info.connection,
            request,
            generation,
            command: action(),
        })
        .unwrap()
    }
    pub fn invoke(&mut self, request: u64, generation: u64) -> Vec<u8> {
        let bytes = self.invocation(request, generation);
        call(&mut self.stream, &write(6, 2, 0, &bytes))
    }
    pub fn status(&mut self) -> AdminStatus {
        opened(&mut self.stream, 1, b"status");
        let result =
            decode_admin_status(body(&call(&mut self.stream, &read(4, 1, 0, 512)))).unwrap();
        assert_eq!(call(&mut self.stream, &clunk(5, 1))[4], 121);
        result
    }
    pub fn outcome(&mut self, expected: ControlOutcome) -> AdminOutcome {
        let mut outcome = None;
        until(|| {
            if let AdminStatus::Outcome(value) = self.status() {
                assert_eq!(value.outcome, expected);
                outcome = Some(value);
                true
            } else {
                false
            }
        });
        outcome.unwrap()
    }
}

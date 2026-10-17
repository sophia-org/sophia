//! Opt-in, bounded diagnostic for buffered shell requests. Not a cost sampler.
//! Records no payload or principal data. Writes received in place are outside
//! this trace's scope; the shell export uses the buffered request path.
//! The request phase marks dispatch, not socket arrival.
use crate::records::{Reply, Request};
use std::sync::Mutex;

#[derive(Debug)]
pub struct Event {
    pub connection: u64,
    pub phase: &'static str,
    pub tag: u16,
    pub op: &'static str,
    pub fid: u32,
    pub newfid: u32,
    pub offset: u64,
    pub count: u32,
    pub names: String,
}
const MAX_EVENTS: usize = 100_000;
static TRACE: Mutex<Option<Vec<Event>>> = Mutex::new(None);
pub fn begin() {
    let mut trace = TRACE.lock().unwrap();
    assert!(trace.is_none(), "trace already active");
    *trace = Some(Vec::new());
}
pub fn take() -> Vec<Event> {
    TRACE.lock().unwrap().take().expect("active trace")
}
fn event(connection: u64, tag: u16, phase: &'static str) -> Event {
    Event {
        connection,
        tag,
        phase,
        op: "other",
        fid: 0,
        newfid: 0,
        offset: 0,
        count: 0,
        names: String::new(),
    }
}
fn push(make: impl FnOnce() -> Event) {
    let mut trace = TRACE.lock().unwrap();
    if let Some(events) = trace.as_mut() {
        assert!(events.len() < MAX_EVENTS, "diagnostic trace overflow");
        events.push(make());
    }
}
pub(crate) fn request(connection: u64, tag: u16, request: &Request<'_>) {
    push(|| {
        let mut e = event(connection, tag, "request");
        match request {
            Request::Attach { fid, .. } => {
                e.op = "attach";
                e.fid = fid.0;
            }
            Request::Walk { fid, newfid, names } => {
                e.op = "walk";
                e.fid = fid.0;
                e.newfid = newfid.0;
                e.names = names
                    .iter()
                    .map(|n| String::from_utf8_lossy(n))
                    .collect::<Vec<_>>()
                    .join("/");
            }
            Request::Lopen { fid, .. } => {
                e.op = "open";
                e.fid = fid.0;
            }
            Request::Read { fid, offset, count } => {
                e.op = "read";
                e.fid = fid.0;
                e.offset = *offset;
                e.count = *count;
            }
            Request::Write { fid, offset, data } => {
                e.op = "write";
                e.fid = fid.0;
                e.offset = *offset;
                e.count = data.len() as u32;
            }
            Request::Clunk { fid } => {
                e.op = "clunk";
                e.fid = fid.0;
            }
            _ => (),
        }
        e
    });
}
pub(crate) fn reply(connection: u64, tag: u16, reply: &Reply) {
    push(|| {
        let mut e = event(connection, tag, "reply");
        match reply {
            Reply::Read(bytes) => {
                e.op = "read";
                e.count = bytes.len() as u32;
            }
            Reply::Write(n) => {
                e.op = "write";
                e.count = *n;
            }
            Reply::Lerror(errno) => {
                e.op = "error";
                e.count = errno.0;
            }
            _ => (),
        }
        e
    });
}
pub(crate) fn sent(connection: u64, tag: u16) {
    push(|| event(connection, tag, "sent"));
}

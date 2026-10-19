//! Comparator-only classification of an owner's existing successful wait.
//! Registrations expire at each wait; raw descriptors are never cached across waits.
use rustix::event::{PollFd, PollFlags};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::os::fd::{AsFd, AsRawFd};

pub const NAMES: [&str; 11] = [
    "server_wake",
    "wire_socket",
    "wire_listener",
    "image_listener",
    "image_socket",
    "worker",
    "relay",
    "transport_other",
    "native",
    "ring",
    "unknown",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Role {
    ServerWake,
    WireSocket,
    WireListener,
    ImageListener,
    ImageSocket,
    Worker,
    Relay,
    TransportOther,
    Native,
    Ring,
    Unknown,
}
const CAPACITY: usize = 256;
const MASKS: usize = 1 << NAMES.len();

/// Bounded accounting, independent of the scheduler and its authority decisions.
pub struct Ledger {
    registrations: [Option<(i32, Role)>; CAPACITY],
    len: usize,
    active: bool,
    pub waits: u64,
    pub overflow: u64,
    pub conflicts: u64,
    pub missing_scope: u64,
    pub masks: [u64; MASKS],
    pub input: [u64; NAMES.len()],
    pub output: [u64; NAMES.len()],
    pub fault: [u64; NAMES.len()],
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            registrations: [None; CAPACITY],
            len: 0,
            active: false,
            waits: 0,
            overflow: 0,
            conflicts: 0,
            missing_scope: 0,
            masks: [0; MASKS],
            input: [0; NAMES.len()],
            output: [0; NAMES.len()],
            fault: [0; NAMES.len()],
        }
    }
}
impl Ledger {
    pub fn begin(&mut self) {
        self.len = 0;
        self.active = true;
    }
    pub fn register(&mut self, fd: i32, role: Role, fallback: bool) {
        if !self.active {
            return;
        }
        for entry in self.registrations[..self.len].iter_mut().flatten() {
            if entry.0 == fd {
                if !fallback && entry.1 != role {
                    entry.1 = Role::Unknown;
                    self.conflicts += 1;
                }
                return;
            }
        }
        if self.len == CAPACITY {
            self.overflow += 1;
            return;
        }
        self.registrations[self.len] = Some((fd, role));
        self.len += 1;
    }
    /// Regions refer to the actual polled slice: non-native, native, then ring.
    pub fn record(&mut self, fds: &[PollFd<'_>], native_start: usize, ring: usize) {
        assert!(native_start <= ring && ring + 1 == fds.len());
        let mut masks = [0usize; 4];
        if !self.active {
            self.missing_scope += 1;
        }
        for (index, fd) in fds.iter().enumerate() {
            let flags = fd.revents();
            if flags.is_empty() {
                continue;
            }
            let role = if index == ring {
                Role::Ring
            } else if index >= native_start {
                Role::Native
            } else if self.active {
                self.registrations[..self.len]
                    .iter()
                    .flatten()
                    .find(|(raw, _)| *raw == fd.as_fd().as_raw_fd())
                    .map_or(Role::Unknown, |(_, role)| *role)
            } else {
                Role::Unknown
            };
            let bit = 1 << role as usize;
            masks[0] |= bit;
            if flags.contains(PollFlags::IN) {
                masks[1] |= bit;
            }
            if flags.contains(PollFlags::OUT) {
                masks[2] |= bit;
            }
            if flags.intersects(PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL) {
                masks[3] |= bit;
            }
        }
        self.waits += 1;
        self.masks[masks[0]] += 1;
        for i in 0..NAMES.len() {
            self.input[i] += u64::from(masks[1] & (1 << i) != 0);
            self.output[i] += u64::from(masks[2] & (1 << i) != 0);
            self.fault[i] += u64::from(masks[3] & (1 << i) != 0);
        }
        self.active = false;
    }
}
impl fmt::Display for Ledger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "waits={} overflow={} conflicts={} missing_scope={}",
            self.waits, self.overflow, self.conflicts, self.missing_scope
        )?;
        for (i, name) in NAMES.iter().enumerate() {
            write!(
                f,
                " in_{name}={} out_{name}={} fault_{name}={}",
                self.input[i], self.output[i], self.fault[i]
            )?;
        }
        for (mask, count) in self.masks.iter().enumerate() {
            if *count != 0 {
                write!(f, " mask_{mask}={count}")?;
            }
        }
        Ok(())
    }
}
thread_local! {
    static LEDGER: RefCell<Ledger> = RefCell::new(Ledger::default());
    static CONTEXT: Cell<Option<Role>> = const { Cell::new(None) };
}
/// A registration owner may classify nested implementation descriptors as its
/// own source (for example, a relay implemented by a 9P server). Set this before
/// entering the nested visitor, rather than relabeling already registered fds.
#[must_use]
pub struct RegistrationContext(Option<Role>);
impl Drop for RegistrationContext {
    fn drop(&mut self) {
        CONTEXT.set(self.0);
    }
}
pub fn context(role: Role) -> RegistrationContext {
    RegistrationContext(CONTEXT.replace(Some(role)))
}
fn contextual(role: Role) -> Role {
    CONTEXT.get().unwrap_or(role)
}

/// Periodic diagnostics only, never called on the per-wait accounting path.
/// One newline-terminated write no larger than Linux PIPE_BUF prevents mixing
/// with the sampler/peer records on the inherited blocking capture pipe.
pub fn write_line(
    output: &mut impl std::io::Write,
    arguments: fmt::Arguments<'_>,
) -> std::io::Result<()> {
    let mut line = fmt::format(arguments);
    line.push('\n');
    if line.len() > 4096 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "diagnostic record exceeds PIPE_BUF",
        ));
    }
    output.write_all(line.as_bytes())
}

pub fn begin() {
    LEDGER.with_borrow_mut(Ledger::begin);
}
pub fn register(fd: &PollFd<'_>, role: Role) {
    LEDGER.with_borrow_mut(|l| l.register(fd.as_fd().as_raw_fd(), contextual(role), false));
}
pub fn fallback(fd: &PollFd<'_>, role: Role) {
    LEDGER.with_borrow_mut(|l| l.register(fd.as_fd().as_raw_fd(), contextual(role), true));
}
pub fn record(fds: &[PollFd<'_>], native_start: usize, ring: usize) {
    LEDGER.with_borrow_mut(|l| l.record(fds, native_start, ring));
}
pub struct Record;
impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        LEDGER.with_borrow(|l| l.fmt(f))
    }
}

//! Private Session–Factotum control records, not the child's afid transcript.
//! Little-endian records: version u32, kind u32, then the exact body. Kind 9
//! adds a bounded audience set; the original single-audience kind 2 is unchanged.

use super::{Audiences, Delegation, MAX_AUDIENCES, Nonce, Verdict};
use sophia_9p::{
    ConnectionId, Errno,
    authentication::{Attempt, Identity},
};

pub const VERSION: u32 = 1;
pub const MAX_RECORD_BYTES: usize = 44 + 8 * MAX_AUDIENCES;
pub const ATTACH: &[u8] = b"sophia-launch-v1";

#[derive(Clone, Copy)]
#[expect(
    clippy::large_enum_variant,
    reason = "owner commands retain Copy cancellation custody; the inline audience set is bounded to 64 values and the queue to 32 commands"
)]
pub enum Command {
    Initialize {
        session: u64,
    },
    Reserve(Delegation),
    ReserveAudiences {
        delegation: Delegation,
        audiences: Audiences,
    },
    Activate {
        launch: u64,
    },
    Retire {
        launch: u64,
    },
    Begin {
        attempt: Attempt,
        remaining_msec: u32,
    },
    Relay {
        launch: u64,
        nonce: Nonce,
    },
    Take {
        attempt: Attempt,
    },
    Cancel {
        attempt: Attempt,
    },
}

#[derive(Clone, Copy)]
pub enum Response {
    Done,
    Challenge(Nonce),
    Pending,
    Verified(Verdict),
}

impl Command {
    pub fn encode(self) -> Vec<u8> {
        let kind = match self {
            Self::Initialize { .. } => 1,
            Self::Reserve(_) => 2,
            Self::ReserveAudiences { .. } => 9,
            Self::Activate { .. } => 3,
            Self::Retire { .. } => 4,
            Self::Begin { .. } => 5,
            Self::Relay { .. } => 6,
            Self::Take { .. } => 7,
            Self::Cancel { .. } => 8,
        };
        let mut b = header(kind);
        match self {
            Self::Initialize { session } => u64_to(&mut b, session),
            Self::Reserve(d) => {
                identity_to(&mut b, d.identity);
                u64_to(&mut b, d.audience);
            }
            Self::ReserveAudiences {
                delegation,
                audiences,
            } => {
                identity_to(&mut b, delegation.identity);
                u64_to(&mut b, delegation.audience);
                b.extend((audiences.as_slice().len() as u32).to_le_bytes());
                for &audience in audiences.as_slice() {
                    u64_to(&mut b, audience);
                }
            }
            Self::Activate { launch } | Self::Retire { launch } => u64_to(&mut b, launch),
            Self::Begin {
                attempt,
                remaining_msec,
            } => {
                attempt_to(&mut b, attempt);
                b.extend(remaining_msec.to_le_bytes());
            }
            Self::Relay { launch, nonce } => {
                u64_to(&mut b, launch);
                b.extend(nonce.0);
            }
            Self::Take { attempt } | Self::Cancel { attempt } => attempt_to(&mut b, attempt),
        }
        b
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Errno> {
        let (mut r, kind) = Reader::start(bytes)?;
        let command = match kind {
            1 => Self::Initialize { session: r.u64()? },
            2 => Self::Reserve(Delegation {
                identity: r.identity()?,
                audience: r.u64()?,
            }),
            3 => Self::Activate { launch: r.u64()? },
            4 => Self::Retire { launch: r.u64()? },
            5 => Self::Begin {
                attempt: r.attempt()?,
                remaining_msec: r.u32()?,
            },
            6 => Self::Relay {
                launch: r.u64()?,
                nonce: Nonce(r.take()?),
            },
            7 => Self::Take {
                attempt: r.attempt()?,
            },
            8 => Self::Cancel {
                attempt: r.attempt()?,
            },
            9 => {
                let delegation = Delegation {
                    identity: r.identity()?,
                    audience: r.u64()?,
                };
                let count = usize::try_from(r.u32()?).map_err(|_| Errno::EINVAL)?;
                if count == 0 || count > MAX_AUDIENCES {
                    return Err(Errno::EINVAL);
                }
                let mut values = [0; MAX_AUDIENCES];
                for value in &mut values[..count] {
                    *value = r.u64()?;
                }
                let audiences = Audiences::new(&values[..count]).map_err(|_| Errno::EINVAL)?;
                if audiences.as_slice() != &values[..count]
                    || !audiences.contains(delegation.audience)
                {
                    return Err(Errno::EINVAL);
                }
                Self::ReserveAudiences {
                    delegation,
                    audiences,
                }
            }
            _ => return Err(Errno::EINVAL),
        };
        r.end()?;
        Ok(command)
    }
}

impl Response {
    pub fn encode(self) -> Vec<u8> {
        let mut b = header(match self {
            Self::Done => 0,
            Self::Challenge(_) => 1,
            Self::Pending => 2,
            Self::Verified(_) => 3,
        });
        match self {
            Self::Challenge(n) => b.extend(n.0),
            Self::Verified(v) => {
                attempt_to(&mut b, v.attempt);
                identity_to(&mut b, v.identity);
            }
            Self::Done | Self::Pending => {}
        }
        b
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Errno> {
        let (mut r, kind) = Reader::start(bytes)?;
        let response = match kind {
            0 => Self::Done,
            1 => Self::Challenge(Nonce(r.take()?)),
            2 => Self::Pending,
            3 => Self::Verified(Verdict {
                attempt: r.attempt()?,
                identity: r.identity()?,
            }),
            _ => return Err(Errno::EINVAL),
        };
        r.end()?;
        Ok(response)
    }
}

fn header(kind: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(MAX_RECORD_BYTES);
    b.extend(VERSION.to_le_bytes());
    b.extend(kind.to_le_bytes());
    b
}
fn u64_to(b: &mut Vec<u8>, value: u64) {
    b.extend(value.to_le_bytes());
}
fn identity_to(b: &mut Vec<u8>, i: Identity) {
    for value in [i.principal, i.namespace, i.launch] {
        u64_to(b, value);
    }
}
fn attempt_to(b: &mut Vec<u8>, a: Attempt) {
    for value in [
        a.server,
        a.connection.0,
        a.session,
        a.audience,
        a.wire_generation,
        a.attempt_generation,
    ] {
        u64_to(b, value);
    }
    b.extend(a.afid.to_le_bytes());
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn start(b: &'a [u8]) -> Result<(Self, u32), Errno> {
        if b.len() > MAX_RECORD_BYTES {
            return Err(Errno::EINVAL);
        }
        let mut r = Self(b);
        if r.u32()? != VERSION {
            return Err(Errno::EINVAL);
        }
        let kind = r.u32()?;
        Ok((r, kind))
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Errno> {
        let (v, rest) = self.0.split_at_checked(N).ok_or(Errno::EINVAL)?;
        self.0 = rest;
        v.try_into().map_err(|_| Errno::EINVAL)
    }
    fn u32(&mut self) -> Result<u32, Errno> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, Errno> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    fn identity(&mut self) -> Result<Identity, Errno> {
        Ok(Identity {
            principal: self.u64()?,
            namespace: self.u64()?,
            launch: self.u64()?,
        })
    }
    fn attempt(&mut self) -> Result<Attempt, Errno> {
        Ok(Attempt {
            server: self.u64()?,
            connection: ConnectionId(self.u64()?),
            session: self.u64()?,
            audience: self.u64()?,
            wire_generation: self.u64()?,
            attempt_generation: self.u64()?,
            afid: self.u32()?,
        })
    }
    fn end(self) -> Result<(), Errno> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Errno::EINVAL)
        }
    }
}

//! Supplied afid verdicts for the sealed timing peer. This exercises the real
//! launch-client plumbing in the child against a supplied authority and the
//! supplied relay in shell_timing_image_relay.rs. It is not Factotum
//! verification or launch-domain proof; that join belongs to a protected
//! Session test.
use sophia_9p::{AttachContext, ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt};
use sophia_protocol::*;
use sophia_runtime::authenticated_files::{FileAuthentication, FileAuthenticationFactory};
use std::time::{Duration, Instant};

/// The launch challenge the afid serves; the relay requires the same bytes.
pub fn challenge() -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..8].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    for (index, byte) in bytes[8..].iter_mut().enumerate() {
        *byte = 0x40 + index as u8;
    }
    bytes
}

pub struct Factory;
struct Owner {
    connection: ConnectionId,
    status_read: bool,
    admitted: bool,
}
impl FileAuthenticationFactory for Factory {
    fn create(
        &mut self,
        connection: ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        Ok(Box::new(Owner {
            connection,
            status_read: false,
            admitted: false,
        }))
    }
}
impl FileAuthentication for Owner {
    fn claims(&self, context: &AttachContext<'_>) -> Result<(), Errno> {
        if context.connection == self.connection {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    }
    fn begin(&mut self, afid: Fid, _: Instant) -> Result<Attempt, Errno> {
        Ok(Attempt {
            server: 1,
            connection: self.connection,
            session: 1,
            audience: 1,
            wire_generation: 1,
            attempt_generation: 1,
            afid: afid.0,
        })
    }
    /// The challenge at 0 and a supplied success status at 40, the only
    /// two reads the launch client makes.
    fn read(
        &mut self,
        attempt: Attempt,
        offset: u64,
        count: u32,
        _: Instant,
    ) -> Result<ReadOutcome, Errno> {
        if attempt.connection != self.connection {
            return Err(Errno::EACCES);
        }
        match (offset, count) {
            (0, 40) => Ok(ReadOutcome::Ready(challenge().to_vec())),
            (40, 8) => {
                self.status_read = true;
                Ok(ReadOutcome::Ready(vec![1, 0, 0, 0, 0, 0, 0, 0]))
            }
            _ => Err(Errno::EINVAL),
        }
    }
    fn commit(&mut self, attempt: Attempt, _: Instant) -> Result<(), Errno> {
        if attempt.connection != self.connection || !self.status_read {
            return Err(Errno::EACCES);
        }
        self.admitted = true;
        Ok(())
    }
    fn check(&mut self, connection: ConnectionId, _: Instant) -> Result<(), Errno> {
        if self.admitted && connection == self.connection {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    }
    fn release(&mut self, _: Attempt, _: Instant) {}
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
        self.status_read = false;
        self.admitted = false;
        Ok(())
    }
    fn close(&mut self, _: Instant) {
        self.admitted = false;
    }
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        Ok(())
    }
    fn drained(&self) -> bool {
        !self.admitted
    }
    fn admitted(&self) -> bool {
        self.admitted
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::MAX
    }
    fn admission_context(&self) -> Option<ClientAdmissionContext> {
        self.admitted.then(|| {
            ClientAdmissionContext::new(
                ClientAdmissionId::from_raw(1),
                NamespaceContext::new(
                    NamespaceId::from_raw(2),
                    NamespaceProfile::Confined,
                    NamespaceCapabilities::NONE,
                )
                .unwrap(),
                ClientAuthProvenance::new(
                    ClientAuthenticationMethod::FactotumLaunch {
                        principal: 3,
                        launch_generation: 4,
                    },
                    5,
                )
                .unwrap(),
            )
            .unwrap()
        })
    }
}

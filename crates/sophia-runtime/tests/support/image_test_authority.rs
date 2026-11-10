//! Supplied afid verdicts for transport tests, not launch authentication evidence.
use sophia_9p::{AttachContext, ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt};
use sophia_protocol::*;
use sophia_runtime::authenticated_files::{FileAuthentication, FileAuthenticationFactory};
#[cfg(feature = "experimental-native-frame-transaction")]
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Factory;
#[cfg(feature = "experimental-native-frame-transaction")]
#[derive(Default)]
pub struct CheckState {
    pub calls: usize,
    pub deny: bool,
}
#[cfg(feature = "experimental-native-frame-transaction")]
pub struct CountingFactory(pub Arc<Mutex<CheckState>>);
#[cfg(feature = "experimental-native-frame-transaction")]
impl FileAuthenticationFactory for CountingFactory {
    fn create(
        &mut self,
        connection: ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        Ok(Box::new(Owner {
            connection,
            admitted: false,
            checks: Some(self.0.clone()),
        }))
    }
}
struct Owner {
    connection: ConnectionId,
    admitted: bool,
    #[cfg(feature = "experimental-native-frame-transaction")]
    checks: Option<Arc<Mutex<CheckState>>>,
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
            admitted: false,
            #[cfg(feature = "experimental-native-frame-transaction")]
            checks: None,
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
    fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
        Err(Errno::EACCES)
    }
    fn commit(&mut self, attempt: Attempt, _: Instant) -> Result<(), Errno> {
        if attempt.connection != self.connection {
            return Err(Errno::EACCES);
        }
        self.admitted = true;
        Ok(())
    }
    fn check(&mut self, connection: ConnectionId, _: Instant) -> Result<(), Errno> {
        #[cfg(feature = "experimental-native-frame-transaction")]
        if let Some(checks) = &self.checks {
            let mut checks = checks.lock().unwrap();
            checks.calls += 1;
            if checks.deny {
                return Err(Errno::EACCES);
            }
        }
        if self.admitted && connection == self.connection {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    }
    fn release(&mut self, _: Attempt, _: Instant) {}
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
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

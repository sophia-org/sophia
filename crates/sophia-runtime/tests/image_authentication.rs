//! Side-channel authority joins current 9P custody, never legacy root access.
use sophia_9p::{
    ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt, export::AttachContext,
};
use sophia_protocol::*;
use sophia_runtime::authenticated_files::{AuthenticatedFiles, FileAuthentication};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct Owner {
    live: Arc<AtomicBool>,
    admitted: bool,
    context: Option<ClientAdmissionContext>,
}
impl FileAuthentication for Owner {
    fn claims(&self, _: &AttachContext<'_>) -> Result<(), Errno> {
        unreachable!()
    }
    fn begin(&mut self, _: Fid, _: Instant) -> Result<Attempt, Errno> {
        unreachable!()
    }
    fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
        unreachable!()
    }
    fn commit(&mut self, _: Attempt, _: Instant) -> Result<(), Errno> {
        unreachable!()
    }
    fn check(&mut self, connection: ConnectionId, _: Instant) -> Result<(), Errno> {
        if connection != ConnectionId(7) || !self.live.load(Ordering::SeqCst) {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    fn release(&mut self, _: Attempt, _: Instant) {
        unreachable!()
    }
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
        unreachable!()
    }
    fn close(&mut self, _: Instant) {
        unreachable!()
    }
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        unreachable!()
    }
    fn drained(&self) -> bool {
        false
    }
    fn admitted(&self) -> bool {
        self.admitted
    }
    fn admission_context(&self) -> Option<ClientAdmissionContext> {
        self.context
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::MAX
    }
}

#[test]
fn side_channel_requires_live_authenticated_custody_and_context() {
    let now = Instant::now();
    for mut files in [
        AuthenticatedFiles::legacy(()),
        AuthenticatedFiles::pending(()),
    ] {
        assert_eq!(
            files.check_side_channel(ConnectionId(7), now),
            Err(Errno::EACCES)
        );
    }
    let context = ClientAdmissionContext::new(
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
    .unwrap();
    let live = Arc::new(AtomicBool::new(true));
    for (admitted, identity) in [(false, Some(context)), (true, None), (true, Some(context))] {
        let mut files = AuthenticatedFiles::pending(());
        files
            .install(Box::new(Owner {
                live: live.clone(),
                admitted,
                context: identity,
            }))
            .unwrap();
        let expected = if admitted && identity.is_some() {
            Ok(context)
        } else {
            Err(Errno::EACCES)
        };
        assert_eq!(files.check_side_channel(ConnectionId(7), now), expected);
        assert_eq!(
            files.check_side_channel(ConnectionId(8), now),
            Err(Errno::ESTALE)
        );
        live.store(false, Ordering::SeqCst);
        // Remembered identity remains available but grants no operation.
        assert_eq!(files.admission_context(), identity);
        assert_eq!(
            files.check_side_channel(ConnectionId(7), now),
            Err(Errno::ESTALE)
        );
        live.store(true, Ordering::SeqCst);
    }
}

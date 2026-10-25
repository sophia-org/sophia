// Instrumentation compiles away outside tests; production never pays for the barriers.
macro_rules! capture_probe {
    ($name:ident) => {
        #[cfg(test)]
        let $name = crate::file_service::tests::fixture::current_probe();
    };
}
macro_rules! before_wait {
    ($probe:ident, $thread:literal, $timeout:expr) => {
        #[cfg(test)]
        if $probe
            .as_ref()
            .is_some_and(|probe| !probe.before_wait($thread, $timeout))
        {
            break;
        }
    };
}
pub(crate) use {before_wait, capture_probe};

#[cfg(test)]
pub(crate) mod fixture {

    use super::super::*;
    use sophia_protocol::{
        NamespaceCapabilities, NamespaceContext, NamespaceProfile, PortalRequest,
    };
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::{Mutex, atomic::AtomicU64, mpsc::Receiver};

    const BOUND: Duration = Duration::from_secs(2);
    type Gate = (mpsc::Sender<()>, Receiver<bool>);
    thread_local! {
        static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
    }

    #[derive(Clone)]
    pub(crate) struct Probe {
        events: mpsc::Sender<(&'static str, Option<Duration>)>,
        gates: Arc<Mutex<BTreeMap<&'static str, Gate>>>,
    }
    impl Probe {
        pub(crate) fn before_wait(&self, thread: &'static str, timeout: Option<Duration>) -> bool {
            let gate = self.gates.lock().unwrap().remove(thread);
            let _ = self.events.send((thread, timeout));
            if let Some((entered, resume)) = gate {
                let _ = entered.send(());
                return resume.recv_timeout(BOUND).unwrap_or(false);
            }
            true
        }
    }

    pub(crate) fn current_probe() -> Option<Probe> {
        PROBE.with(|p| p.borrow().clone())
    }

    pub(crate) struct Observer {
        pub(crate) events: Receiver<(&'static str, Option<Duration>)>,
        probe: Probe,
    }
    impl Observer {
        pub(crate) fn install() -> Self {
            let (events, receive) = mpsc::channel();
            let probe = Probe {
                events,
                gates: Arc::default(),
            };
            PROBE.with(|p| assert!(p.replace(Some(probe.clone())).is_none()));
            Self {
                events: receive,
                probe,
            }
        }
        pub(crate) fn pause(&self, thread: &'static str) -> Pause {
            let (entered, ready) = mpsc::channel();
            let (resume, receive) = mpsc::channel();
            assert!(
                self.probe
                    .gates
                    .lock()
                    .unwrap()
                    .insert(thread, (entered, receive))
                    .is_none()
            );
            Pause {
                entered: ready,
                resume,
            }
        }
        fn parked(&self) {
            loop {
                if self.events.recv_timeout(BOUND).unwrap() == ("portal-files", None) {
                    return;
                }
            }
        }
    }
    impl Drop for Observer {
        fn drop(&mut self) {
            PROBE.with(|p| {
                p.take();
            });
        }
    }
    pub(crate) struct Pause {
        pub(crate) entered: Receiver<()>,
        resume: mpsc::Sender<bool>,
    }
    impl Pause {
        pub(crate) fn finish(self, keep_running: bool) {
            let _ = self.resume.send(keep_running);
        }
    }
    impl Drop for Pause {
        fn drop(&mut self) {
            let _ = self.resume.send(true);
        }
    }

    #[derive(Clone)]
    struct Authority {
        live: Arc<AtomicBool>,
        revision: Arc<AtomicU64>,
        revision_reads: Arc<AtomicU64>,
        panic: Arc<AtomicBool>,
    }
    impl Default for Authority {
        fn default() -> Self {
            Self {
                live: Arc::new(AtomicBool::new(true)),
                revision: Arc::new(AtomicU64::new(1)),
                revision_reads: Arc::new(AtomicU64::new(0)),
                panic: Arc::new(AtomicBool::new(false)),
            }
        }
    }
    impl PortalFileAuthority for Authority {
        fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
            [1, 2].contains(&id.raw()).then(|| {
                NamespaceContext::new(id, NamespaceProfile::Confined, NamespaceCapabilities::ALL)
                    .unwrap()
            })
        }
        fn source_generation(&self, _: &PortalRequest) -> Option<u64> {
            Some(1)
        }
        fn now_msec(&self) -> u64 {
            1
        }
        fn live(&self) -> bool {
            assert!(
                !self.panic.load(Ordering::Acquire),
                "injected authority failure"
            );
            self.live.load(Ordering::Acquire)
        }
        fn revision(&self) -> u64 {
            self.revision_reads.fetch_add(1, Ordering::AcqRel);
            self.revision.load(Ordering::Acquire)
        }
    }
    fn service(authority: Authority) -> SessionPortalFileService {
        SessionPortalFileService::spawn(authority, PortalFileLimits::defaults(1), |_| false)
            .unwrap()
    }
    fn connect(
        service: &SessionPortalFileService,
        revision: u64,
    ) -> Result<PortalRequesterConnection, Errno> {
        service.connect(NamespaceId::from_raw(1), NamespaceId::from_raw(2), revision)
    }

    #[test]
    fn no_periodic_turns_while_idle_and_a_late_enqueue_wakes_the_server() {
        let observer = Observer::install();
        let service = service(Authority::default());
        observer.parked();
        assert!(matches!(
            observer.events.recv_timeout(Duration::from_millis(80)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        let first = connect(&service, 1).unwrap();
        assert_eq!(first.epoch, 1);
        assert_eq!(
            observer.events.recv_timeout(BOUND).unwrap(),
            ("portal-files", Some(Duration::from_millis(5)))
        );
        drop(first);
        observer.parked();
        let (done, joined) = mpsc::channel();
        std::thread::spawn(move || {
            drop(service);
            done.send(()).unwrap();
        });
        joined
            .recv_timeout(BOUND)
            .expect("parked Drop must wake before join");
    }

    #[test]
    fn enqueue_between_empty_check_and_poll_is_not_lost() {
        let observer = Observer::install();
        let pause = observer.pause("portal-files");
        let service = service(Authority::default());
        pause.entered.recv_timeout(BOUND).unwrap();
        // Publish and notify while the worker is past its empty check but not in poll.
        let (reply, answer) = mpsc::sync_channel(1);
        service
            .connect
            .try_send(Connect {
                source: NamespaceId::from_raw(1),
                target: NamespaceId::from_raw(2),
                revision: 1,
                deadline: Instant::now() + BOUND,
                reply,
            })
            .unwrap();
        service.wake.wake();
        pause.finish(true);
        let first = answer.recv_timeout(BOUND).unwrap().unwrap();
        assert_eq!(first.epoch, 1);
        drop(first);
        for epoch in 2..=101 {
            observer.parked();
            assert_eq!(connect(&service, 1).unwrap().epoch, epoch);
        }
    }

    #[test]
    fn a_full_connection_queue_preserves_the_successor_and_refuses_an_extra_request() {
        let observer = Observer::install();
        let service = service(Authority::default());
        let first = connect(&service, 1).unwrap();
        let pause = observer.pause("portal-files");
        service.wake.wake();
        pause.entered.recv_timeout(BOUND).unwrap();
        let (reply, answer) = mpsc::sync_channel(1);
        service
            .connect
            .try_send(Connect {
                source: NamespaceId::from_raw(1),
                target: NamespaceId::from_raw(2),
                revision: 1,
                deadline: Instant::now() + BOUND,
                reply,
            })
            .unwrap();
        service.wake.wake();
        assert!(matches!(connect(&service, 1), Err(Errno::EAGAIN)));
        pause.finish(true);
        assert!(matches!(
            answer.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(first);
        assert_eq!(answer.recv_timeout(BOUND).unwrap().unwrap().epoch, 2);
    }

    #[test]
    fn revocation_while_parked_does_not_restore_an_old_revision() {
        let observer = Observer::install();
        let authority = Authority::default();
        let service = service(authority.clone());
        observer.parked();
        authority.live.store(false, Ordering::Release);
        authority.revision.store(2, Ordering::Release);
        authority.live.store(true, Ordering::Release);
        assert!(matches!(
            observer.events.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(connect(&service, 1), Err(Errno::ESTALE)));
        assert_eq!(connect(&service, 2).unwrap().epoch, 1);
    }

    #[test]
    fn an_admitted_connection_prevents_idle_until_disconnect() {
        let mut export =
            PortalFileExport::new(Authority::default(), PortalFileLimits::defaults(1)).unwrap();
        assert!(export.idle());
        export
            .admit(PortalRequesterAdmission {
                connection: sophia_9p::ConnectionId(1),
                custody: PortalRequesterCustody::SessionAuthority {
                    registration: std::num::NonZeroU64::new(1).unwrap(),
                },
                epoch: 1,
                source: NamespaceId::from_raw(1),
                target: NamespaceId::from_raw(2),
            })
            .unwrap();
        assert!(!export.idle());
        export.disconnect(sophia_9p::ConnectionId(1));
        assert!(export.idle());
    }
    #[test]
    fn active_connections_keep_checking_revision_without_peer_traffic() {
        let observer = Observer::install();
        let authority = Authority::default();
        let service = service(authority.clone());
        let connection = connect(&service, 1).unwrap();
        let reads = authority.revision_reads.load(Ordering::Acquire);
        authority.revision.store(2, Ordering::Release);
        while authority.revision_reads.load(Ordering::Acquire) <= reads {
            let (thread, timeout) = observer.events.recv_timeout(BOUND).unwrap();
            assert_eq!(thread, "portal-files");
            if let Some(timeout) = timeout {
                assert_eq!(timeout, Duration::from_millis(5));
            }
        }
        assert!(
            crate::file_client::PortalFileClient::over(connection.stream, connection.epoch, BOUND)
                .is_err()
        );
    }

    #[test]
    fn an_unwinding_file_thread_sets_stop_before_notifying() {
        let observer = Observer::install();
        let authority = Authority::default();
        let stop = Arc::new(AtomicBool::new(false));
        let observed_stop = stop.clone();
        let (notify, notified) = mpsc::channel();
        let service = SessionPortalFileService::spawn_with_stop(
            authority.clone(),
            PortalFileLimits::defaults(1),
            stop.clone(),
            move || {
                assert!(observed_stop.load(Ordering::Acquire));
                notify.send(()).unwrap();
            },
            |_| false,
        )
        .unwrap();
        observer.parked();
        authority.panic.store(true, Ordering::Release);
        service.wake.wake();
        notified
            .recv_timeout(BOUND)
            .expect("unwind must wake the other worker");
        assert!(stop.load(Ordering::Acquire));
        drop(service);
    }
}

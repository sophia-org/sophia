//! Actual worker Stop/Drop with a bounded simulated transport-credit wait.
//! This does not claim a 9P socket or journal; those join in the next fixture.
#![cfg(test)]
use super::adapter::{
    PolicyAdapter, PolicyAdapterEvent, PolicyAdapterStop, PolicyProfileAdmission,
};
use super::driver::PolicyReceivePermit;
use super::*;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Default)]
struct CreditWait {
    stopped: Mutex<bool>,
    wake: Condvar,
}
struct Interrupt(Arc<CreditWait>);
impl PolicyAdapterStop for Interrupt {
    fn stop(&self) {
        *self.0.stopped.lock().unwrap() = true;
        self.0.wake.notify_all();
    }
}
struct WaitingAdapter {
    wait: Arc<CreditWait>,
    entered: SyncSender<()>,
    disconnected: SyncSender<()>,
}
impl PolicyAdapter for WaitingAdapter {
    fn admit(
        &mut self,
        _: crate::live_session::policy_transport_worker::driver::PolicyAdmissionPermit,
        _: u64,
        _: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        Ok(())
    }
    fn selected_capabilities(&self) -> u64 {
        0
    }
    fn receive_within(
        &mut self,
        permit: PolicyReceivePermit,
        _: Duration,
    ) -> Result<PolicyAdapterEvent, String> {
        let event = PolicyAdapterEvent::Configuration {
            transaction: TransactionId::from_raw(1),
            configuration: PolicyConfiguration {
                connection_epoch: 1,
                generation: 1,
                actions: vec![],
                chrome: sophia_protocol::WmChromePolicy::default(),
            },
        };
        assert!(permit.allows(&event));
        Ok(event)
    }
    fn try_receive(
        &mut self,
        _: PolicyReceivePermit,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        Ok(None)
    }
    fn send(&mut self, _: &PolicyTransportCommand) -> Result<(), String> {
        self.entered.send(()).unwrap();
        let (stopped, deadline) = self
            .wait
            .wake
            .wait_timeout_while(
                self.wait.stopped.lock().unwrap(),
                Duration::from_secs(5),
                |stopped| !*stopped,
            )
            .unwrap();
        assert!(
            !deadline.timed_out(),
            "Stop failed to wake transport-credit wait"
        );
        assert!(*stopped);
        Err("transport stopped".into())
    }
    fn stop_handle(&self) -> Option<Box<dyn PolicyAdapterStop>> {
        Some(Box::new(Interrupt(self.wait.clone())))
    }
    fn disconnect(&mut self) {
        self.disconnected.send(()).unwrap();
    }
}

#[test]
fn stop_and_drop_wake_inflight_send_even_when_command_queue_is_full() {
    for explicit_stop in [false, true] {
        let wait = Arc::new(CreditWait::default());
        let (entered, receiving) = sync_channel(1);
        let (disconnected, closed) = sync_channel(1);
        let worker = PolicyTransportWorker::spawn(
            WaitingAdapter {
                wait,
                entered,
                disconnected,
            },
            1,
            None,
        )
        .unwrap();
        assert!(matches!(
            worker.event_timeout(Duration::from_secs(2)),
            Ok(PolicyTransportEvent::Negotiated)
        ));
        assert!(matches!(
            worker.event_timeout(Duration::from_secs(2)),
            Ok(PolicyTransportEvent::Configuration { .. })
        ));
        let command = || PolicyTransportCommand::ConfigurationOutcome {
            transaction: TransactionId::from_raw(1),
            generation: 1,
            outcome: PolicyProjectionOutcome::Committed,
        };
        assert!(worker.try_command(command()).is_ok());
        receiving.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(worker.try_command(command()).is_ok());
        assert!(worker.try_command(command()).is_err());
        if explicit_stop {
            assert!(worker.try_command(PolicyTransportCommand::Stop).is_ok());
        }
        let (done, finished) = sync_channel(1);
        std::thread::spawn(move || {
            drop(worker);
            done.send(()).unwrap();
        });
        finished
            .recv_timeout(Duration::from_secs(2))
            .expect("Drop did not wake and join the producer");
        closed.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(closed.try_recv(), Err(TryRecvError::Disconnected)));
    }
}

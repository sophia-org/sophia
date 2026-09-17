//! Exact request/response custody and aggregate credits with simulated FIFO
//! drain. These controls do not claim a kernel socket-backpressure schedule.
use super::*;
use sophia_protocol::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    transport: ShellSessionTransport,
    directory: std::path::PathBuf,
}
impl Fixture {
    fn new(content: bool) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-indicator-credit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellSessionTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::getuid().as_raw(),
        )
        .unwrap();
        transport.state.connection_epoch = 1;
        transport.state.capabilities =
            SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION;
        if content {
            let grant = ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            };
            let mut limits = ContentLimits::prototype(grant);
            limits.max_control_records = 1;
            transport.content_epochs.admit(limits.clone()).unwrap();
            transport.state.store_grant = grant;
            transport.state.content_limits = Some(limits);
            transport.state.content_grant = Some(grant);
        }
        Self {
            transport,
            directory,
        }
    }
    fn request(&mut self, event_id: u64) -> (TransactionId, ShellIndicatorActivation) {
        let tx = TransactionId::from_raw(event_id + 10);
        let activation = ShellIndicatorActivation {
            connection_epoch: 1,
            snapshot_generation: 2,
            output: OutputId::from_raw(3),
            indicator: 4,
            action: 5,
            event_id,
        };
        self.transport
            .state
            .inbox
            .push_back(encode_shell_indicator_activation(tx, &activation).unwrap());
        (tx, activation)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn indicator_request_waits_for_credit_then_transfers_it_through_final_byte() {
    for content in [false, true] {
        let mut f = Fixture::new(content);
        let (tx, activation) = f.request(1);
        let t = &mut f.transport;
        // Synthetic owned bulk frames fill the real aggregate record budget.
        for _ in 0..if content { 1 } else { 64 } {
            t.state.output.push(vec![0; 32], false);
        }
        assert!(
            t.state
                .take_indicator_request(&mut t.content_epochs,)
                .unwrap()
                .is_none()
        );
        assert_eq!(t.state.inbox.len(), 1);
        assert!(t.state.indicator_response.is_none());
        while !t.state.output.is_empty() {
            t.state.output.written(t.state.output.front().len());
        }
        assert_eq!(
            t.state
                .take_indicator_request(&mut t.content_epochs,)
                .unwrap(),
            Some((tx, activation))
        );
        assert_eq!(t.content_accounting().response_records, 1);
        assert_eq!(t.content_accounting().response_bytes, CONTROL_FRAME_BYTES);
        assert!(
            t.state
                .take_indicator_request(&mut t.content_epochs,)
                .unwrap()
                .is_none()
        );
        if !content {
            for _ in 0..63 {
                t.state.output.push(vec![0; 32], false);
            }
        }
        assert!(!t.state.bulk_capacity_available(&t.content_epochs, 1));
        // A mismatched completion cannot change the original obligation.
        assert!(
            t.finish_indicator_activation(
                TransactionId::from_raw(99),
                &activation,
                ShellIndicatorActivationStatus::Accepted,
                0
            )
            .is_err()
        );
        assert!(t.state.indicator_response.unwrap().outcome.is_none());
        t.finish_indicator_activation(tx, &activation, ShellIndicatorActivationStatus::Accepted, 0)
            .unwrap();
        assert!(t.state.indicator_response.is_none());
        if !content {
            assert_eq!(t.state.output.records(), 64);
            for _ in 0..63 {
                t.state.output.written(t.state.output.front().len());
            }
        }
        assert_eq!(t.state.output.records(), 1);
        assert_eq!(t.state.output.controls(), 1);
        let owned = t.content_accounting();
        assert_eq!(owned.response_records, 1);
        assert_eq!(owned.response_bytes, CONTROL_FRAME_BYTES);
        let (actual_tx, outcome) =
            decode_shell_indicator_activation_outcome(t.state.output.front()).unwrap();
        assert_eq!(actual_tx, tx);
        assert_eq!(
            (outcome.event_id, outcome.status),
            (1, ShellIndicatorActivationStatus::Accepted)
        );
        assert!(
            t.finish_indicator_activation(
                tx,
                &activation,
                ShellIndicatorActivationStatus::Accepted,
                0
            )
            .is_err()
        );
        t.state.output.written(1);
        assert_eq!(t.content_accounting(), owned);
        assert_eq!(t.state.output.records(), 1);
        assert_eq!(t.state.output.controls(), 1);
        if content {
            assert!(!t.state.control_capacity_available(&t.content_epochs, 1));
        }
        t.state.output.written(t.state.output.front().len());
        assert_eq!(
            (t.state.output.records(), t.state.output.controls()),
            (0, 0)
        );
        assert_eq!(t.content_accounting().response_records, 0);
        assert_eq!(t.content_accounting().response_bytes, 0);
        assert!(
            !t.state
                .flush_indicator_response(&mut t.content_epochs,)
                .unwrap()
        );
    }
}

#[test]
fn refused_outcome_transfer_retains_exact_completed_result_without_readmission() {
    let mut f = Fixture::new(true);
    let (tx, activation) = f.request(1);
    assert!(
        f.transport
            .state
            .take_indicator_request(&mut f.transport.content_epochs)
            .unwrap()
            .is_some()
    );
    f.request(2);
    let t = &mut f.transport;
    // Inject admission refusal after reservation. Negotiated limits are not
    // mutable in production; this tests the defensive returned-refusal path.
    t.state.content_limits.as_mut().unwrap().max_control_records = 0;
    t.finish_indicator_activation(tx, &activation, ShellIndicatorActivationStatus::Accepted, 0)
        .unwrap();
    assert!(t.state.output.is_empty());
    let pending = t.state.indicator_response.unwrap();
    assert_eq!(
        pending.outcome.unwrap().status,
        ShellIndicatorActivationStatus::Accepted
    );
    assert!(
        t.state
            .take_indicator_request(&mut t.content_epochs,)
            .unwrap()
            .is_none()
    );
    assert_eq!(t.state.inbox.len(), 1);
    assert!(
        t.finish_indicator_activation(tx, &activation, ShellIndicatorActivationStatus::Stale, 0)
            .is_err()
    );
    assert_eq!(t.state.indicator_response.unwrap().outcome, pending.outcome);
    t.state.content_limits.as_mut().unwrap().max_control_records = 1;
    assert!(
        t.state
            .flush_indicator_response(&mut t.content_epochs,)
            .unwrap()
    );
    assert!(
        !t.state
            .flush_indicator_response(&mut t.content_epochs,)
            .unwrap()
    );
    assert!(
        t.state
            .take_indicator_request(&mut t.content_epochs,)
            .unwrap()
            .is_none()
    ); // FIFO still owns credit.
    t.state.output.written(t.state.output.front().len());
    assert_eq!(
        t.state
            .take_indicator_request(&mut t.content_epochs,)
            .unwrap()
            .unwrap()
            .1
            .event_id,
        2
    );
    t.disconnect().unwrap();
    assert!(t.state.indicator_response.is_none());
    assert!(
        t.finish_indicator_activation(tx, &activation, ShellIndicatorActivationStatus::Accepted, 0)
            .is_err()
    );
}

#[test]
fn pending_indicator_credit_is_charged_against_control_and_total_bytes() {
    let mut f = Fixture::new(true);
    let (tx, activation) = f.request(1);
    let t = &mut f.transport;
    let limits = t.state.content_limits.as_mut().unwrap();
    limits.max_control_records = 64;
    limits.reserved_control_queue_bytes = CONTROL_FRAME_BYTES as u32;
    let bulk = limits.max_output_queue_bytes as usize - CONTROL_FRAME_BYTES;
    t.state.output.push(vec![0; bulk], false);
    assert!(
        t.state
            .take_indicator_request(&mut t.content_epochs,)
            .unwrap()
            .is_some()
    );
    assert!(!t.state.control_capacity_available(&t.content_epochs, 1));
    assert!(!t.state.bulk_capacity_available(&t.content_epochs, 1));
    t.finish_indicator_activation(tx, &activation, ShellIndicatorActivationStatus::Accepted, 0)
        .unwrap();
    assert!(t.state.indicator_response.is_none());
    assert_eq!(
        (t.state.output.records(), t.state.output.controls()),
        (2, 1)
    );
    assert!(!t.state.control_capacity_available(&t.content_epochs, 1));
    assert!(!t.state.bulk_capacity_available(&t.content_epochs, 1));
}

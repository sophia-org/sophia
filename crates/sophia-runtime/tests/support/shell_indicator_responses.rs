//! Exact request/response custody and aggregate credits over a real socket
//! pair whose peer does not read. Requests arrive through the production
//! read path; FIFO drain is the production write path, one bounded turn at a
//! time. These controls do not claim a kernel socket-backpressure schedule.
use super::super::control_budget::{CONTROL_RECORD_BYTES, Class};
use super::super::outbound::{Admitted, OutboundRecord};
use super::super::socket::SocketWire;
use super::super::wire::Wire;
use super::*;
use sophia_protocol::*;
use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    transport: ShellSessionTransport,
    peer: UnixStream,
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
            let grant = grant();
            let mut limits = ContentLimits::prototype(grant);
            limits.max_control_records = 1;
            transport.content_epochs.admit(limits.clone()).unwrap();
            transport.state.store_grant = grant;
            transport.state.content_limits = Some(limits);
            transport.state.content_grant = Some(grant);
        }
        let (local, peer) = UnixStream::pair().unwrap();
        local.set_nonblocking(true).unwrap();
        let limits = transport.state.content_limits.clone();
        transport.state.wire = Some(Wire::Socket(Box::new(SocketWire::new(
            local,
            limits.as_ref(),
        ))));
        Self {
            transport,
            peer,
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
        self.peer
            .write_all(&encode_shell_indicator_activation(tx, &activation).unwrap())
            .unwrap();
        let socket = self.transport.state.socket_mut().unwrap();
        let before = socket.input_accounting().0;
        while socket.input_accounting().0 == before {
            socket.receive(4096).unwrap();
        }
        (tx, activation)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

/// One owned typed bulk record: output facts with `outputs` rows.
fn bulk(t: &ShellSessionTransport, outputs: usize) -> Admitted {
    let facts = ContentOutputFacts {
        grant: grant(),
        facts_generation: 1,
        outputs: (0..outputs)
            .map(|index| ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: index as u64 + 1,
                    generation: 1,
                },
                local_width: 100,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            })
            .collect(),
    };
    t.state
        .admit_record(
            OutboundRecord::Content(
                TransactionId::from_raw(90),
                ShellContentRecord::OutputFacts(facts),
            ),
            Class::Bulk,
        )
        .unwrap()
}

fn inbox(t: &ShellSessionTransport) -> usize {
    t.state.socket().unwrap().input_accounting().0
}

fn front_frame(t: &ShellSessionTransport) -> Vec<u8> {
    SocketWire::encode(&t.state.output.front().unwrap().record).unwrap()
}

/// Writes at most `bytes` of the output order through the socket.
fn send(t: &mut ShellSessionTransport, bytes: usize) {
    let Some(Wire::Socket(socket)) = t.state.wire.as_mut() else {
        unreachable!("the fixture attaches a socket");
    };
    socket.send(&mut t.state.output, bytes).unwrap();
}

#[test]
fn indicator_request_waits_for_credit_then_transfers_it_through_final_byte() {
    for content in [false, true] {
        let mut f = Fixture::new(content);
        let (tx, activation) = f.request(1);
        let t = &mut f.transport;
        // Owned typed bulk records fill the real aggregate record budget.
        for _ in 0..if content { 1 } else { 64 } {
            let record = bulk(t, 0);
            t.state.transfer_record(record);
        }
        assert!(
            t.state
                .take_indicator_request(&mut t.content_epochs,)
                .unwrap()
                .is_none()
        );
        assert_eq!(inbox(t), 1);
        assert!(t.state.indicator_response.is_none());
        while t.state.output.front().is_some() {
            let bytes = front_frame(t).len();
            send(t, bytes);
        }
        assert_eq!(
            t.state
                .take_indicator_request(&mut t.content_epochs,)
                .unwrap(),
            Some((tx, activation))
        );
        assert_eq!(t.content_accounting().response_records, 1);
        assert_eq!(t.content_accounting().response_bytes, CONTROL_RECORD_BYTES);
        assert!(
            t.state
                .take_indicator_request(&mut t.content_epochs,)
                .unwrap()
                .is_none()
        );
        if !content {
            for _ in 0..63 {
                let record = bulk(t, 0);
                t.state.transfer_record(record);
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
                let bytes = front_frame(t).len();
                send(t, bytes);
            }
        }
        assert_eq!(t.state.output.records(), 1);
        assert_eq!(t.state.output.controls(), 1);
        let owned = t.content_accounting();
        assert_eq!(owned.response_records, 1);
        assert_eq!(owned.response_bytes, CONTROL_RECORD_BYTES);
        let OutboundRecord::IndicatorOutcome(actual_tx, outcome) =
            t.state.output.front().unwrap().record
        else {
            panic!("the completed outcome is queued as a typed record");
        };
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
        let bytes = front_frame(t).len();
        send(t, 1);
        assert_eq!(t.content_accounting(), owned);
        assert_eq!(t.state.output.records(), 1);
        assert_eq!(t.state.output.controls(), 1);
        if content {
            assert!(!t.state.control_capacity_available(&t.content_epochs, 1));
        }
        send(t, bytes - 1);
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
    assert!(t.state.output.front().is_none());
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
    assert_eq!(inbox(t), 1);
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
    let bytes = front_frame(t).len();
    send(t, bytes);
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
    // One typed bulk record of 680 body bytes spends the whole bulk budget:
    // the output bound is exactly one control credit above it.
    let record = bulk(t, 16);
    assert_eq!(record.charge, 680);
    let limits = t.state.content_limits.as_mut().unwrap();
    limits.max_control_records = 64;
    limits.reserved_control_queue_bytes = CONTROL_RECORD_BYTES as u32;
    limits.max_output_queue_bytes = (CONTROL_RECORD_BYTES + record.charge) as u32;
    t.state.transfer_record(record);
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

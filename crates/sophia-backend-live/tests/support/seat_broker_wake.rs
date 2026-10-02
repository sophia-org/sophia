//! The libseat broker sleeps without a timeout. These drive its production
//! loop and wait through a scripted backend whose connection is a wake (or a
//! socket whose peer hung up), so no seat, seatd or logind is involved.

use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sophia_wake::{Notifier, Wake};

use super::{
    LiveSeatCommand, LiveSeatCommands, LiveSeatDisableOutcome, LiveSeatLease, SeatBrokerBackend,
    run_broker_loop,
};

/// Bounds every wait on the broker, so a lost wake fails instead of hanging.
const BOUND: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Dispatch,
    /// The loop offered its connection: it is about to block.
    Wait,
    Close(u64),
    Switch(u8),
    Disable,
}

enum Connection {
    /// Readable when rung; dispatch drains it, as libseat drains its socket.
    Wake(Wake),
    /// A socket whose peer is gone: readable and hung up for good.
    HungUp(UnixStream),
}

struct ScriptedSeat {
    connection: Connection,
    steps: Sender<Step>,
    /// Runs as the loop offers its connection: after it drained the command
    /// queue and immediately before it blocks.
    before_wait: Box<dyn FnMut() + Send>,
}

impl SeatBrokerBackend for ScriptedSeat {
    fn dispatch_ready(&mut self) -> Result<(), String> {
        if let Connection::Wake(wake) = &self.connection {
            wake.clear().unwrap();
        }
        let _ = self.steps.send(Step::Dispatch);
        Ok(())
    }

    fn connection(&mut self) -> Result<BorrowedFd<'_>, String> {
        let _ = self.steps.send(Step::Wait);
        (self.before_wait)();
        Ok(match &self.connection {
            Connection::Wake(wake) => wake.as_fd(),
            Connection::HungUp(socket) => socket.as_fd(),
        })
    }

    fn execute(&mut self, command: LiveSeatCommand) {
        match command {
            LiveSeatCommand::Open(_, reply) => {
                let _ = reply.send(Err("scripted seat opens nothing".to_owned()));
            }
            LiveSeatCommand::Close(token) => {
                let _ = self.steps.send(Step::Close(token));
            }
            LiveSeatCommand::Switch(terminal, reply) => {
                let _ = self.steps.send(Step::Switch(terminal));
                let _ = reply.send(Ok(()));
            }
            LiveSeatCommand::Disable(reply) => {
                let _ = self.steps.send(Step::Disable);
                let _ = reply.send(Ok(LiveSeatDisableOutcome::Acknowledged));
            }
            LiveSeatCommand::Shutdown => unreachable!("the loop handles shutdown itself"),
        }
    }
}

struct Broker {
    commands: LiveSeatCommands,
    steps: Receiver<Step>,
    done: Receiver<Result<(), String>>,
    worker: JoinHandle<()>,
}

impl Broker {
    fn next_step(&self) -> Step {
        self.steps
            .recv_timeout(BOUND)
            .expect("the broker moved without a timed wakeup")
    }

    fn expect_steps(&self, expected: &[Step]) {
        let observed: Vec<_> = expected.iter().map(|_| self.next_step()).collect();
        assert_eq!(observed, expected);
    }

    fn shutdown(self) -> Vec<Step> {
        self.commands.send(LiveSeatCommand::Shutdown).unwrap();
        let result = self
            .done
            .recv_timeout(BOUND)
            .expect("the broker left its untimed wait");
        assert_eq!(result, Ok(()));
        self.worker.join().unwrap();
        self.steps.try_iter().collect()
    }
}

/// The broker's command channel, made before its backend so a backend hook
/// can send through it.
fn command_channel() -> (LiveSeatCommands, Receiver<LiveSeatCommand>, Wake) {
    let wake = Wake::new().unwrap();
    let (sender, receiver) = mpsc::channel();
    (
        LiveSeatCommands {
            sender: Some(sender),
            wake: wake.notifier(),
        },
        receiver,
        wake,
    )
}

fn spawn(
    (commands, receiver, wake): (LiveSeatCommands, Receiver<LiveSeatCommand>, Wake),
    connection: Connection,
    before_wait: impl FnMut() + Send + 'static,
) -> Broker {
    let (steps_tx, steps) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let mut backend = ScriptedSeat {
        connection,
        steps: steps_tx,
        before_wait: Box::new(before_wait),
    };
    let worker = thread::spawn(move || {
        let _ = done_tx.send(run_broker_loop(&mut backend, &receiver, &wake));
    });
    Broker {
        commands,
        steps,
        done,
        worker,
    }
}

fn wake_connection() -> (Connection, Notifier) {
    let wake = Wake::new().unwrap();
    let notifier = wake.notifier();
    (Connection::Wake(wake), notifier)
}

#[test]
fn a_lease_close_wakes_an_idle_broker() {
    let (connection, _ring) = wake_connection();
    let broker = spawn(command_channel(), connection, || {});
    broker.expect_steps(&[Step::Dispatch, Step::Wait]);

    // Dropping the last device handle is the only way a lease speaks.
    drop(LiveSeatLease {
        token: 7,
        commands: broker.commands.clone(),
    });

    // Served, then dispatched again before the broker sleeps.
    broker.expect_steps(&[Step::Dispatch, Step::Close(7), Step::Dispatch, Step::Wait]);
    assert_eq!(broker.shutdown(), [Step::Dispatch]);
}

#[test]
fn a_command_sent_between_drain_and_wait_is_served() {
    let channel = command_channel();
    let racing = channel.0.clone();
    let (reply_tx, reply) = mpsc::sync_channel(1);
    let mut reply_tx = Some(reply_tx);
    // The producer lands after the loop found the queue empty and before it
    // blocks: exactly the window a timeout used to paper over.
    let (connection, _ring) = wake_connection();
    let broker = spawn(channel, connection, move || {
        if let Some(reply_tx) = reply_tx.take() {
            racing.send(LiveSeatCommand::Switch(3, reply_tx)).unwrap();
        }
    });

    assert_eq!(reply.recv_timeout(BOUND), Ok(Ok(())));
    broker.expect_steps(&[
        Step::Dispatch,
        Step::Wait,
        Step::Dispatch,
        Step::Switch(3),
        Step::Dispatch,
        Step::Wait,
    ]);
    assert_eq!(broker.shutdown(), [Step::Dispatch]);
}

#[test]
fn queued_commands_are_served_before_the_first_wait() {
    let channel = command_channel();
    let (reply_tx, reply) = mpsc::sync_channel(1);
    channel.0.send(LiveSeatCommand::Disable(reply_tx)).unwrap();
    let (connection, _ring) = wake_connection();
    let broker = spawn(channel, connection, || {});

    assert_eq!(
        reply.recv_timeout(BOUND),
        Ok(Ok(LiveSeatDisableOutcome::Acknowledged))
    );
    broker.expect_steps(&[Step::Dispatch, Step::Disable, Step::Dispatch, Step::Wait]);
    // The ring that came with the queued command is still pending, so the
    // broker may take one spurious turn; it serves nothing twice.
    let tail = broker.shutdown();
    assert_eq!(tail.last(), Some(&Step::Dispatch));
    assert!(
        tail.iter()
            .all(|step| matches!(step, Step::Dispatch | Step::Wait)),
        "tail: {tail:?}"
    );
}

#[test]
fn connection_readiness_dispatches_libseat() {
    let (connection, ring) = wake_connection();
    let broker = spawn(command_channel(), connection, || {});
    broker.expect_steps(&[Step::Dispatch, Step::Wait]);

    ring.notify();

    broker.expect_steps(&[Step::Dispatch, Step::Wait]);
    assert_eq!(broker.shutdown(), [Step::Dispatch]);
}

#[test]
fn shutdown_ends_a_waiting_broker() {
    let (connection, _ring) = wake_connection();
    let broker = spawn(command_channel(), connection, || {});
    broker.expect_steps(&[Step::Dispatch, Step::Wait]);

    assert_eq!(broker.shutdown(), [Step::Dispatch]);
}

#[test]
fn dropping_the_last_command_sender_ends_a_waiting_broker() {
    let (connection, _ring) = wake_connection();
    let broker = spawn(command_channel(), connection, || {});
    broker.expect_steps(&[Step::Dispatch, Step::Wait]);

    drop(broker.commands);

    assert_eq!(broker.done.recv_timeout(BOUND).unwrap(), Ok(()));
    broker.worker.join().unwrap();
    assert_eq!(
        broker.steps.try_iter().collect::<Vec<_>>(),
        [Step::Dispatch]
    );
}

#[test]
fn a_hung_up_connection_reports_failure_instead_of_spinning() {
    let (socket, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let broker = spawn(command_channel(), Connection::HungUp(socket), || {});
    broker.expect_steps(&[Step::Dispatch, Step::Wait]);
    assert_eq!(
        broker.done.recv_timeout(BOUND).unwrap().unwrap_err(),
        "libseat connection closed while waiting"
    );
    broker.worker.join().unwrap();
    assert_eq!(broker.steps.try_iter().count(), 0);
}

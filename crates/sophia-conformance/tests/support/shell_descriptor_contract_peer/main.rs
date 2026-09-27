//! Scripted public-codec peer for the descriptor conformance host's three modes.
//!
//! It has no UI or product policy. The host drives snapshots and outcomes; this
//! peer echoes admitted identities, waits for Presented before acknowledging an
//! activation, and withdraws on the second descriptor snapshot. This is Rust
//! owner/codec coverage. The independent C wire and descriptor tests stay separate.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use sophia_protocol::*;

mod persistent;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (mode, fault) = match args.as_slice() {
        [mode] => (mode, None),
        [mode, option] if mode == "--serve" => {
            let value = option.strip_prefix("--fault=").unwrap_or_else(|| usage());
            let fault = match value {
                "ack-epoch" => Fault::AckEpoch(false),
                "ack-activation" => Fault::AckActivation(false),
                "ack-transaction" => Fault::AckTransaction(false),
                "stale-ack-epoch" => Fault::AckEpoch(true),
                "stale-ack-activation" => Fault::AckActivation(true),
                "stale-ack-transaction" => Fault::AckTransaction(true),
                "stale-accepted" => Fault::StaleAccepted,
                _ => usage(),
            };
            (mode, Some(fault))
        }
        _ => usage(),
    };
    let capabilities = match mode.as_str() {
        "--proof" => SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
        "--bar-proof" => {
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
        }
        "--serve" => {
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_TAB_GROUPS
                | SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG
                | SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET
        }
        _ => usage(),
    };
    let mut peer = Peer::connect(capabilities);
    peer.fault = fault;
    if mode == "--serve" {
        persistent::tabs(&mut peer);
        persistent::reference(&mut peer);
    }
    descriptors(&mut peer, mode == "--bar-proof");
}

fn usage() -> ! {
    eprintln!(
        "usage: shell_descriptor_contract_peer --proof|--serve|--bar-proof [--fault=CASE (serve only)]"
    );
    std::process::exit(2);
}

struct Peer {
    stream: UnixStream,
    epoch: u64,
    generation: u64,
    fault: Option<Fault>,
}

// These controls emit validly encoded but incorrectly correlated tab acks. The
// host must reject them itself; a codec error or timeout is not the expected exit.
#[derive(Clone, Copy)]
enum Fault {
    AckEpoch(bool),
    AckActivation(bool),
    AckTransaction(bool),
    StaleAccepted,
}

impl Peer {
    fn connect(capabilities: u64) -> Self {
        let socket = std::env::var_os("SOPHIA_SHELL_SOCKET").expect("explicit shell endpoint");
        let stream = UnixStream::connect(socket).expect("connect to the conformance host");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut peer = Self {
            stream,
            epoch: 0,
            generation: 0,
            fault: None,
        };
        peer.send(
            encode_shell_v1_client_hello_frame(ShellV1ClientHello {
                minimum_revision: SOPHIA_SHELL_INTERFACE_REVISION,
                maximum_revision: SOPHIA_SHELL_REFERENCE_REVISION,
                required_capabilities: capabilities,
            })
            .unwrap(),
        );
        let welcome = decode_shell_v1_server_welcome_frame(&peer.read()).unwrap();
        assert!(
            (SOPHIA_SHELL_INTERFACE_REVISION..=SOPHIA_SHELL_REFERENCE_REVISION)
                .contains(&welcome.selected_revision)
        );
        assert_eq!(welcome.capabilities & capabilities, capabilities);
        assert_ne!(welcome.connection_epoch, 0);
        peer.epoch = welcome.connection_epoch;
        peer
    }

    fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    fn send(&mut self, frame: Vec<u8>) {
        self.stream.write_all(&frame).unwrap();
    }

    fn read(&mut self) -> Vec<u8> {
        let mut header = [0; SOPHIA_IPC_HEADER_LEN];
        self.stream.read_exact(&mut header).unwrap();
        let size = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        assert!(size <= SOPHIA_IPC_MAX_PAYLOAD_LEN);
        let mut frame = header.to_vec();
        frame.resize(SOPHIA_IPC_HEADER_LEN + size, 0);
        self.stream
            .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
            .unwrap();
        decode_frame(&frame).unwrap();
        frame
    }

    fn transfer(
        &mut self,
        first: IpcMessageKind,
        last: IpcMessageKind,
        bound: usize,
    ) -> Vec<Vec<u8>> {
        let mut frames = vec![self.read()];
        assert_eq!(decode_frame(&frames[0]).unwrap().0.message_kind, first);
        loop {
            assert!(frames.len() < bound, "bounded transfer did not end");
            let frame = self.read();
            let done = decode_frame(&frame).unwrap().0.message_kind == last;
            frames.push(frame);
            if done {
                return frames;
            }
        }
    }

    fn outcome(
        &mut self,
        tx: TransactionId,
        generation: u64,
        kind: ShellV1CandidateOutcomeKind,
    ) -> ShellV1CandidateOutcome {
        let (actual, outcome) = decode_shell_v1_candidate_outcome_frame(&self.read()).unwrap();
        assert_eq!(actual, tx);
        assert_eq!(outcome.connection_epoch, self.epoch);
        assert_eq!(outcome.candidate_generation, generation);
        assert_eq!(outcome.kind, kind);
        if kind == ShellV1CandidateOutcomeKind::Presented {
            assert_ne!(outcome.presentation_epoch, 0);
        } else {
            assert_eq!(outcome.presentation_epoch, 0);
        }
        outcome
    }

    fn activation(
        &mut self,
        generation: u64,
        presented: u64,
        actions: &[ToplevelActionCapabilityRef],
    ) -> ShellV1ActivationDisposition {
        let (tx, activation) = decode_shell_v1_activation_frame(&self.read()).unwrap();
        let valid = activation.connection_epoch == self.epoch
            && activation.candidate_generation == generation
            && activation.presentation_epoch == presented
            && actions.contains(&activation.action);
        let mut disposition = if valid {
            ShellV1ActivationDisposition::Consumed
        } else {
            ShellV1ActivationDisposition::RejectedStale
        };
        let mut ack_tx = tx;
        let mut epoch = self.epoch;
        let mut id = activation.activation;
        let stale_event = !valid;
        match self.fault {
            Some(Fault::AckEpoch(stale)) if stale == stale_event => epoch += 1,
            Some(Fault::AckActivation(stale)) if stale == stale_event => id += 1,
            Some(Fault::AckTransaction(stale)) if stale == stale_event => {
                ack_tx = TransactionId::from_raw(tx.raw() + 1);
            }
            Some(Fault::StaleAccepted) if !valid => {
                disposition = ShellV1ActivationDisposition::Consumed
            }
            _ => {}
        }
        self.send(
            encode_shell_v1_activation_ack_frame(
                ack_tx,
                ShellV1ActivationAck {
                    connection_epoch: epoch,
                    activation: id,
                    disposition,
                },
            )
            .unwrap(),
        );
        disposition
    }
}

fn descriptors(peer: &mut Peer, bar: bool) {
    for visible in [true, false] {
        let (tx, snapshot) = decode_shell_v1_descriptor_snapshot_frame(&peer.read()).unwrap();
        assert_eq!(snapshot.connection_epoch, peer.epoch);
        let generation = peer.next_generation();
        let entries: Vec<_> = snapshot
            .descriptors
            .iter()
            .filter(|_| visible)
            .map(|d| ShellV1CandidateEntry {
                slot: d.slot,
                generation: d.generation,
            })
            .collect();
        let reservation = if bar && visible {
            Some(ShellV1WorkAreaReservation {
                edge: ShellV1ReservationEdge::Bottom,
                thickness_px: std::env::var("SOPHIA_SHELL_BAR_THICKNESS")
                    .unwrap()
                    .parse()
                    .unwrap(),
            })
        } else {
            None
        };
        peer.send(
            encode_shell_v1_candidate_frame(
                tx,
                &ShellV1Candidate {
                    connection_epoch: peer.epoch,
                    snapshot_generation: snapshot.snapshot_generation,
                    candidate_generation: generation,
                    output: snapshot.output,
                    visible,
                    selected_slot: entries.first().map(|e| e.slot),
                    entries,
                    reservation,
                },
            )
            .unwrap(),
        );
        peer.outcome(tx, generation, ShellV1CandidateOutcomeKind::Prepared);
        let presented = peer.outcome(tx, generation, ShellV1CandidateOutcomeKind::Presented);
        if visible && !bar {
            let actions: Vec<_> = snapshot.descriptors.iter().map(|d| d.action).collect();
            assert_eq!(
                peer.activation(generation, presented.presentation_epoch, &actions),
                ShellV1ActivationDisposition::Consumed
            );
        }
    }
}

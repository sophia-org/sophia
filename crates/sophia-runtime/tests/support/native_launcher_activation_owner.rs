//! Actual owner and outbox custody over a real file server and raw 9P peer.
//! Negotiation, focus and request facts are supplied. Journal saturation is
//! real; reduced owner limits are defensive controls, not normal negotiation.
use super::*;
use crate::shell_transport::files::{ShellFileWire, ShellFiles, role_bounds};
use crate::shell_transport::outbound::{Admitted, OutboundRecord};
use sophia_protocol::shell_files::*;
#[path = "shell_file_peer.rs"]
mod shell_file_peer;
use crate::shell_transport::wire::Wire;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    transport: ShellComponentTransport,
    epochs: crate::ContentEpochRegistry,
    peer: shell_file_peer::Peer,
    directory: std::path::PathBuf,
    opening: NativeLauncherOpening,
    activation: NativeLauncherActivation,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "native-response-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::getuid().as_raw(),
        )
        .unwrap();
        let grant = ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        };
        let output = ContentOutputId {
            id: 1,
            generation: 1,
        };
        let mut limits = ContentLimits::prototype(grant);
        limits.max_control_records = 3;
        let mut epochs = crate::ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        epochs
            .admit_with_profile(limits.clone(), crate::ContentStoreProfile::NativeLauncher)
            .unwrap();
        transport.store_grant = grant;
        transport.content_grant = Some(grant);
        transport.content_limits = Some(limits);
        transport.connection_epoch = 1;
        transport.capabilities = SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;
        let opening = NativeLauncherOpening {
            grant,
            opening: 1,
            output,
            catalog_generation: 1,
            state_revision: 1,
        };
        let binding = NativeLauncherBinding {
            grant,
            output,
            opening: 1,
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            catalog_generation: 1,
            candidate_generation: 1,
            presentation_epoch: 1,
            interaction_generation: 1,
            state_revision: 1,
            focus_lease: 1,
        };
        let activation = NativeLauncherActivation {
            event: NativeLauncherEvent {
                binding,
                event_id: 1,
                state_revision: 1,
            },
            cause: 1,
            slot: 1,
        };
        transport.native_control.opening = Some(opening);
        transport.native_control.focus = Some(binding);
        transport.native_control.last_opening = 1;
        let (local, peer) = UnixStream::pair().unwrap();
        let mut export = ShellFiles::awaiting_negotiation(
            1,
            "launcher",
            true,
            role_bounds(Some(crate::ContentStoreProfile::NativeLauncher)).1,
            100,
            std::time::Instant::now(),
        );
        let limits = transport.content_limits.clone().unwrap();
        let bytes = encode_shell_file_limits(
            ShellFileHeader {
                kind: ShellFileKind::Limits,
                connection_epoch: 1,
                submission_id: 0,
                sequence: 0,
            },
            limits.clone(),
        )
        .unwrap();
        export
            .complete_negotiation(Some((bytes, limits)), transport.capabilities)
            .unwrap();
        transport.wire = Some(Wire::Files(Box::new(
            ShellFileWire::adopt(local, export).unwrap(),
        )));
        let mut fixture = Self {
            transport,
            epochs,
            peer: shell_file_peer::Peer::from_stream(peer),
            directory,
            opening,
            activation,
        };
        fixture.drive(|peer| {
            peer.setup();
            let bytes = encode_shell_file_native_launcher_transaction(
                ShellFileHeader {
                    kind: ShellFileKind::NativeActivate,
                    connection_epoch: 1,
                    submission_id: 1,
                    sequence: 0,
                },
                &ShellFileNativeLauncherRecord {
                    transaction: TransactionId::from_raw(1),
                    record: ShellNativeLauncherRecord::Activate(activation),
                },
            )
            .unwrap();
            peer.submit_acknowledged(&bytes, 1);
        });
        fixture
    }

    fn drive<R: Send>(
        &mut self,
        operation: impl FnOnce(&mut shell_file_peer::Peer) -> R + Send,
    ) -> R {
        std::thread::scope(|scope| {
            let peer = &mut self.peer;
            let worker = scope.spawn(move || operation(peer));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !worker.is_finished() {
                // Serve file requests only. These controls choose separately
                // when owner obligations move from the outbox into the journal.
                self.transport.files_mut().unwrap().turn().unwrap();
                assert!(std::time::Instant::now() < deadline, "file peer hung");
                std::thread::yield_now();
            }
            worker.join().unwrap()
        })
    }
    fn inbox(&self) -> usize {
        let Some(Wire::Files(files)) = &self.transport.wire else {
            unreachable!()
        };
        usize::from(files.export().peek_native_activate().is_some())
    }
    fn send_all(&mut self) -> bool {
        let Some(Wire::Files(files)) = self.transport.wire.as_mut() else {
            unreachable!()
        };
        ShellComponentTransport::drain_file_output(files, &mut self.transport.output).unwrap()
    }
    fn next_event(&mut self) -> Vec<u8> {
        self.drive(|peer| {
            let event = peer.next_event();
            peer.ack(&event);
            event
        })
    }
    fn next_native(&mut self, kind: ShellFileKind) -> ShellNativeLauncherRecord {
        decode_shell_file_native_launcher_transaction(&self.next_event(), kind)
            .unwrap()
            .record
    }
    fn filler(&self) -> OutboundRecord {
        OutboundRecord::Content(
            TransactionId::from_raw(90),
            ShellContentRecord::ResourceStatus(ContentResourceStatus {
                grant: self.opening.grant,
                resource: ContentResourceId {
                    id: 99,
                    generation: 1,
                },
                status: 3,
                reason: ContentReason::Stale as u16,
                next_ordinal: 0,
                admitted_bytes: 0,
            }),
        )
    }
    fn fill_journal(&mut self) -> usize {
        let (kind, bytes) = self.filler().native().unwrap();
        for count in 0..=256 {
            if !self
                .transport
                .files_mut()
                .unwrap()
                .append(kind, &bytes, true)
                .unwrap()
            {
                assert!(count > 0);
                return count;
            }
        }
        panic!("journal did not saturate");
    }
    fn read_filler(&mut self) {
        let value = decode_shell_file_resource_status(&self.next_event()).unwrap();
        assert_eq!(
            OutboundRecord::Content(value.transaction, value.record),
            self.filler()
        );
    }

    /// One owned typed bulk record; no resource-store state is changed.
    fn bulk(&self) -> Admitted {
        self.transport
            .admit_record(
                self.filler(),
                crate::shell_transport::control_budget::Class::Bulk,
            )
            .unwrap()
    }
    fn front_native(&self) -> ShellNativeLauncherRecord {
        let Some(OutboundRecord::NativeLauncher(_, record)) = self
            .transport
            .output
            .front()
            .map(|queued| queued.record.clone())
        else {
            panic!("a typed native launcher record is queued");
        };
        record
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn exact_native_response_credit_survives_journal_pressure_and_cannot_hand_out_twice() {
    let mut f = Fixture::new();
    let tx = TransactionId::from_raw(1);
    let bulk = f.bulk();
    f.transport.transfer_record(bulk);
    assert_eq!(
        f.transport.take_native_launcher_request(&f.epochs).unwrap(),
        None
    );
    assert_eq!(f.inbox(), 1);
    assert!(f.send_all());
    assert_eq!(
        f.transport.take_native_launcher_request(&f.epochs).unwrap(),
        Some((tx, f.activation))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        3
    );
    assert!(!f.transport.bulk_capacity_available(&f.epochs, 1));
    assert_eq!(
        f.transport.take_native_launcher_request(&f.epochs).unwrap(),
        None
    );
    assert!(
        f.transport
            .finish_native_launcher_activation(
                &f.epochs,
                TransactionId::from_raw(2),
                &f.activation,
                NativeLauncherActivationDecision::Admitted
            )
            .is_err()
    );
    assert!(
        f.transport
            .native_control
            .activation_response
            .unwrap()
            .outcome
            .is_none()
    );
    f.transport
        .finish_native_launcher_activation(
            &f.epochs,
            tx,
            &f.activation,
            NativeLauncherActivationDecision::Admitted,
        )
        .unwrap();
    assert!(f.transport.native_control.activation_response.is_none());
    assert!(f.transport.native_control.launch_admitted);
    assert!(matches!(f.front_native(),
        ShellNativeLauncherRecord::ActivationOutcome(v) if v.status == 1 && v.activation == f.activation));
    let fillers = f.fill_journal();
    let before = f.transport.content_accounting(&f.epochs);
    assert!(!f.send_all());
    assert_eq!(f.transport.content_accounting(&f.epochs), before);
    // ACK one old event; the outcome's whole charge transfers only when the
    // journal accepts it, even though the peer has not read the outcome yet.
    f.read_filler();
    assert!(f.send_all());
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    for _ in 0..fillers {
        f.read_filler();
    }
    assert!(
        matches!(f.next_native(ShellFileKind::NativeActivationOutcome),
        ShellNativeLauncherRecord::ActivationOutcome(v) if v.status == 1 && v.activation == f.activation)
    );

    assert!(!f.transport.flush_native_activation(&f.epochs).unwrap());
    f.drive(|peer| peer.no_event());
}

#[test]
fn refused_outcome_survives_close_without_disarming_a_new_opening() {
    let mut f = Fixture::new();
    let tx = TransactionId::from_raw(1);
    f.transport
        .take_native_launcher_request(&f.epochs)
        .unwrap()
        .unwrap();
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 2;
    f.transport
        .finish_native_launcher_activation(
            &f.epochs,
            tx,
            &f.activation,
            NativeLauncherActivationDecision::Admitted,
        )
        .unwrap();
    assert_eq!(
        f.transport
            .native_control
            .activation_response
            .unwrap()
            .outcome
            .unwrap()
            .status,
        1
    );
    assert!(f.transport.output.front().is_none());
    assert!(
        f.transport
            .finish_native_launcher_activation(
                &f.epochs,
                tx,
                &f.activation,
                NativeLauncherActivationDecision::Stale
            )
            .is_err()
    );
    f.transport
        .close_native_launcher(
            &mut f.epochs,
            f.opening,
            TransactionId::from_raw(2),
            ContentReason::Cancelled,
        )
        .unwrap();
    assert!(f.transport.native_control.closing.is_some());
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 3;
    f.transport.flush_native_close(&mut f.epochs).unwrap();
    assert!(f.transport.native_control.opening.is_none());
    assert_eq!(
        f.transport
            .native_control
            .activation_response
            .unwrap()
            .outcome
            .unwrap()
            .status,
        1
    );
    assert!(f.send_all());
    assert_eq!(
        f.next_native(ShellFileKind::NativeFocusRevoked),
        ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
            binding: f.activation.event.binding,
            reason: ContentReason::Cancelled as u16,
        })
    );
    assert_eq!(
        f.next_native(ShellFileKind::NativeClosed),
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: f.opening.grant,
            opening: f.opening.opening,
            reason: ContentReason::Cancelled as u16,
        })
    );
    let mut newer = f.opening;
    newer.opening += 1;
    f.transport
        .publish_native_launcher_opening(&f.epochs, TransactionId::from_raw(3), newer)
        .unwrap();
    f.transport
        .finish_native_launcher_activation(
            &f.epochs,
            tx,
            &f.activation,
            NativeLauncherActivationDecision::Admitted,
        )
        .unwrap();
    assert_eq!(f.transport.native_control.opening, Some(newer));
    assert!(!f.transport.native_control.launch_admitted);
    assert!(f.transport.native_control.activation_response.is_none());
    assert!(f.send_all());
    assert_eq!(
        f.next_native(ShellFileKind::NativeOpening),
        ShellNativeLauncherRecord::Opening(newer)
    );
    assert!(
        matches!(f.next_native(ShellFileKind::NativeActivationOutcome),
        ShellNativeLauncherRecord::ActivationOutcome(v) if v.status == 1 && v.activation == f.activation)
    );
    f.drive(|peer| peer.no_event());
}

#[test]
fn a_pending_file_record_keeps_a_closed_opening_unsettled_until_journal_custody() {
    let mut f = Fixture::new();
    f.transport
        .close_native_launcher(
            &mut f.epochs,
            f.opening,
            TransactionId::from_raw(2),
            ContentReason::Cancelled,
        )
        .unwrap();
    // The setup Activate is late now; retain and observe its exact refusal.
    f.transport
        .service_closed_native_input(&mut f.epochs, f.opening)
        .unwrap();
    assert!(f.send_all());
    assert_eq!(
        f.next_native(ShellFileKind::NativeFocusRevoked),
        ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
            binding: f.activation.event.binding,
            reason: ContentReason::Cancelled as u16,
        })
    );
    assert_eq!(
        f.next_native(ShellFileKind::NativeClosed),
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: f.opening.grant,
            opening: f.opening.opening,
            reason: ContentReason::Cancelled as u16,
        })
    );
    assert!(
        matches!(f.next_native(ShellFileKind::NativeActivationOutcome),
        ShellNativeLauncherRecord::ActivationOutcome(v) if v.status == 2 && v.activation == f.activation)
    );
    assert!(
        f.transport
            .closed_native_owners_settled(&f.epochs, f.opening)
            .unwrap()
    );

    let fillers = f.fill_journal();
    let bulk = f.bulk();
    f.transport.transfer_record(bulk);
    assert!(!f.send_all());
    assert_eq!(f.transport.fifo_records(), 1);
    assert!(
        !f.transport
            .closed_native_owners_settled(&f.epochs, f.opening)
            .unwrap()
    );
    for _ in 0..fillers {
        f.read_filler();
    }
    assert!(f.send_all());
    assert_eq!(f.transport.fifo_records(), 0);
    // Local settlement excludes the journal's retention: it is not proof of
    // receipt. The unread event below remains available after settlement.
    assert!(
        f.transport
            .closed_native_owners_settled(&f.epochs, f.opening)
            .unwrap()
    );
    f.read_filler();
    f.drive(|peer| peer.no_event());
}

//! Actual aggregate/FIFO ownership with supplied connection/request facts and
//! simulated write completion. Limit reduction is a defensive refusal control,
//! not normal negotiation or observed kernel backpressure.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    transport: ShellComponentTransport,
    epochs: crate::ContentEpochRegistry,
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
        transport.inbox.push_back(
            encode_shell_native_launcher_frame(
                TransactionId::from_raw(1),
                &ShellNativeLauncherRecord::Activate(activation),
            )
            .unwrap(),
        );
        Self {
            transport,
            epochs,
            directory,
            opening,
            activation,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn exact_native_response_credit_survives_final_byte_and_cannot_hand_out_twice() {
    let mut f = Fixture::new();
    let tx = TransactionId::from_raw(1);
    f.transport.output.push(vec![0; 32], false);
    assert_eq!(
        f.transport.take_native_launcher_request(&f.epochs).unwrap(),
        None
    );
    assert_eq!(f.transport.inbox.len(), 1);
    f.transport.output.written(32);
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
    assert!(
        matches!(decode_shell_native_launcher_frame(f.transport.output.front()).unwrap().1,
        ShellNativeLauncherRecord::ActivationOutcome(v) if v.status == 1 && v.activation == f.activation)
    );
    let before = f.transport.content_accounting(&f.epochs);
    f.transport.output.written(1);
    assert_eq!(f.transport.content_accounting(&f.epochs), before);
    f.transport.output.written(f.transport.output.front().len());
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    assert!(!f.transport.flush_native_activation(&f.epochs).unwrap());
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
    assert!(f.transport.output.is_empty());
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
            &f.epochs,
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
    f.transport.flush_native_close(&f.epochs).unwrap();
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
    for _ in 0..2 {
        f.transport.output.written(f.transport.output.front().len());
    }
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
    f.transport.output.written(f.transport.output.front().len()); // Opening
    assert!(
        matches!(decode_shell_native_launcher_frame(f.transport.output.front()).unwrap().1,
        ShellNativeLauncherRecord::ActivationOutcome(v) if v.status == 1 && v.activation == f.activation)
    );
}

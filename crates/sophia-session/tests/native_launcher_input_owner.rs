#![cfg(feature = "native-session")]
//! Real private transport with supplied native presentation/protection facts.
use sophia_engine::PresentedContentTarget;
use sophia_protocol::*;
use sophia_session::session_actions::SessionLaunchQueue;
use sophia_session::shell_native_launcher::{
    NativeLauncherActionService, NativeLauncherContentService,
};
#[allow(dead_code)]
#[path = "support/content_actions/native_launcher_fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn exact_inputs_survive_receipt_saturation_and_resume_after_ack_without_replay() {
    let mut h = Harness::new();
    let mut owner =
        NativeLauncherContentService::new(&h.peer.transport.connection(&mut h.epochs)).unwrap();
    for i in 0..32 {
        assert!(
            owner
                .queue_input(
                    &h.peer.transport.connection(&mut h.epochs),
                    h.focus,
                    tx(100 + i),
                    NativeLauncherInputKind::Text,
                    &format!("λ{i}"),
                    1000
                )
                .unwrap()
        );
    }
    assert!(
        !owner
            .queue_input(
                &h.peer.transport.connection(&mut h.epochs),
                h.focus,
                tx(200),
                NativeLauncherInputKind::Text,
                "overflow",
                1000
            )
            .unwrap()
    );
    assert_eq!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 1001)
            .unwrap(),
        16
    );
    assert_eq!(owner.pending_inputs(), 16);
    assert_eq!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 1002)
            .unwrap(),
        0
    );
    h.peer.transport.poll_io(&mut h.epochs).unwrap();
    let mut events = vec![];
    for i in 0..16 {
        let (transaction, ShellNativeLauncherRecord::Input(input)) =
            decode_shell_native_launcher_frame(&h.peer.read()).unwrap()
        else {
            panic!("input")
        };
        assert_eq!(transaction, tx(100 + i));
        assert_eq!(input.text, format!("λ{i}"));
        assert_eq!(input.event.binding, h.focus);
        events.push(input.event);
    }
    for event in events {
        h.peer.send(ShellNativeLauncherRecord::InputAck(
            NativeLauncherInputAck {
                event,
                disposition: 1,
            },
        ));
    }
    assert_eq!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 1003)
            .unwrap(),
        16
    );
    assert_eq!(owner.pending_inputs(), 0);
    h.peer.transport.poll_io(&mut h.epochs).unwrap();
    for i in 16..32 {
        let (transaction, ShellNativeLauncherRecord::Input(input)) =
            decode_shell_native_launcher_frame(&h.peer.read()).unwrap()
        else {
            panic!("input")
        };
        assert_eq!(transaction, tx(100 + i));
        assert_eq!(input.text, format!("λ{i}"));
        assert_eq!(input.issued_mono_usec, 1003);
    }
    assert_eq!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 1004)
            .unwrap(),
        0
    );
}

#[test]
fn stale_focus_invalid_text_and_expiry_never_retarget_or_consume_pending_input() {
    let mut h = Harness::new();
    let mut owner =
        NativeLauncherContentService::new(&h.peer.transport.connection(&mut h.epochs)).unwrap();
    let mut wrong = h.focus;
    wrong.grant.content_grant_epoch += 1;
    assert!(
        owner
            .queue_input(
                &h.peer.transport.connection(&mut h.epochs),
                wrong,
                tx(100),
                NativeLauncherInputKind::Text,
                "x",
                1000
            )
            .is_err()
    );
    assert!(
        owner
            .queue_input(
                &h.peer.transport.connection(&mut h.epochs),
                h.focus,
                tx(100),
                NativeLauncherInputKind::Text,
                "\0",
                1000
            )
            .is_err()
    );
    assert_eq!(owner.pending_inputs(), 0);
    assert!(
        owner
            .queue_input(
                &h.peer.transport.connection(&mut h.epochs),
                h.focus,
                tx(100),
                NativeLauncherInputKind::Text,
                "x",
                1000
            )
            .unwrap()
    );
    assert!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 999)
            .is_err()
    );
    assert_eq!(owner.pending_inputs(), 1);
    assert!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 5_001_001)
            .is_err()
    );
    assert_eq!(owner.pending_inputs(), 1);
}

#[test]
fn exact_close_cancels_only_local_input_and_preserves_issued_receipt_ownership() {
    let mut h = Harness::new();
    let mut owner =
        NativeLauncherContentService::new(&h.peer.transport.connection(&mut h.epochs)).unwrap();
    for i in 0..32 {
        assert!(
            owner
                .queue_input(
                    &h.peer.transport.connection(&mut h.epochs),
                    h.focus,
                    tx(100 + i),
                    NativeLauncherInputKind::Text,
                    "x",
                    1000
                )
                .unwrap()
        );
    }
    assert_eq!(
        owner
            .service_inputs(&mut h.peer.transport.connection(&mut h.epochs), 1001)
            .unwrap(),
        16
    );
    let opening = h
        .peer
        .transport
        .connection(&mut h.epochs)
        .native_launcher_state()
        .unwrap()
        .0;
    let mut wrong = opening;
    wrong.opening += 1;
    assert!(
        owner
            .begin_close(
                &mut h.peer.transport.connection(&mut h.epochs),
                wrong,
                tx(200),
                ContentReason::Cancelled
            )
            .is_err()
    );
    assert_eq!(owner.pending_inputs(), 16);
    owner
        .begin_close(
            &mut h.peer.transport.connection(&mut h.epochs),
            opening,
            tx(200),
            ContentReason::Cancelled,
        )
        .unwrap();
    assert_eq!(owner.pending_inputs(), 0);
    assert!(
        h.peer
            .transport
            .connection(&mut h.epochs)
            .native_launcher_focus()
            .is_none()
    );
    h.peer.transport.poll_io(&mut h.epochs).unwrap();
    for i in 0..16 {
        let (transaction, ShellNativeLauncherRecord::Input(input)) =
            decode_shell_native_launcher_frame(&h.peer.read()).unwrap()
        else {
            panic!("issued input retained")
        };
        assert_eq!(transaction, tx(100 + i));
        assert_eq!(input.text, "x");
    }
    // Closing owns revocation/Closed after the already-issued FIFO records.
    assert!(matches!(
        decode_shell_native_launcher_frame(&h.peer.read())
            .unwrap()
            .1,
        ShellNativeLauncherRecord::FocusRevoked(_)
    ));
    assert!(matches!(
        decode_shell_native_launcher_frame(&h.peer.read())
            .unwrap()
            .1,
        ShellNativeLauncherRecord::Closed(_)
    ));
}

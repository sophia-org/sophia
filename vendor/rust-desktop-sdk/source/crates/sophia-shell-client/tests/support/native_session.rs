use super::*;
use std::os::fd::AsFd;

fn drive_session(
    s: &mut NativeLauncherSession,
    p: &mut Peer,
    mut done: impl FnMut(&mut NativeLauncherSession, &mut Peer) -> bool,
) {
    let end = Instant::now() + WAIT;
    loop {
        p.pump();
        s.dispatch(Instant::now(), 1000, 262144).unwrap();
        p.pump();
        if done(s, p) {
            return;
        }
        assert!(Instant::now() < end, "session exchange stalled");
    }
}
fn session(first: u64) -> (NativeLauncherSession, Peer) {
    let (c, p) = ready();
    let mut s = NativeLauncherSession::new(c, first).unwrap();
    assert!(matches!(
        s.next_event(1000).unwrap(),
        Some(NativeLauncherObservation::Content(
            _,
            ShellContentRecord::Limits(_)
        ))
    ));
    s.done().unwrap();
    (s, p)
}
fn send_input(s: &mut NativeLauncherSession, p: &mut Peer) {
    push(
        p,
        ShellNativeLauncherRecord::Opening(NativeLauncherOpening {
            grant: binding().grant,
            opening: binding().opening,
            output: binding().output,
            catalog_generation: binding().catalog_generation,
            state_revision: 1,
        }),
    );
    drive_session(s, p, |s, _| s.next_event(1000).unwrap().is_some());
    s.done().unwrap();
    push(
        p,
        ShellNativeLauncherRecord::Input(NativeLauncherInput {
            event: NativeLauncherEvent {
                binding: binding(),
                event_id: 11,
                state_revision: 2,
            },
            issued_mono_usec: 500_999,
            kind: NativeLauncherInputKind::Text,
            text: "ab".into(),
        }),
    );
    drive_session(s, p, |s, _| s.next_event(1000).unwrap().is_some());
}

#[test]
fn owning_session_retains_head_and_commits_response_once() {
    let (mut s, mut p) = session(80);
    let _fd = s.as_fd();
    assert!(s.io_interest().is_ok());
    send_input(&mut s, &mut p);
    for _ in 0..3 {
        assert!(matches!(
            s.next_event(1000).unwrap(),
            Some(NativeLauncherObservation::Record(_))
        ));
        assert_eq!(s.lifecycle().revision(), 2);
        assert!(s.done().is_err());
    }
    let expected_due =
        500 + u64::from(ContentLimits::prototype(binding().grant).action_ack_timeout_ms);
    let o = s.obligations();
    assert_eq!(o.input_due_ms, Some(expected_due));
    assert_eq!(o.next_ms, Some(expected_due));
    assert!(o.needs_catalog && o.needs_facts);
    drop(s.prepare_input(2, false).unwrap());
    assert!(s.lifecycle().input().is_some());
    let a = s.prepare_input(2, false).unwrap().commit();
    assert!(s.lifecycle().input().is_none());
    assert!(s.next_event(1000).unwrap().is_none());
    assert!(s.prepare_input(2, false).is_err());
    assert_eq!(s.obligations().input_due_ms, None);
    drive_session(&mut s, &mut p, |_, p| !p.submits.is_empty());
    let held = p.submits.pop_front().unwrap();
    let submit = decode_shell_file_submit(&held.data).unwrap();
    let bytes = p.transactions.last().unwrap();
    let ack = decode_shell_file_native_launcher_transaction(bytes, ShellFileKind::NativeInputAck)
        .unwrap();
    assert_eq!(ack.transaction, tx(80)); // cancelled preparation consumed no ID
    p.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = p.submitted(submit.submission_id, ShellFileKind::NativeInputAck);
    p.push(event);
    drive_session(&mut s, &mut p, |s, _| {
        s.custody(a.first) == Ok(Custody::Submitted)
    });
}

#[test]
fn session_clock_and_transaction_exhaustion_preserve_head() {
    let (mut s, mut p) = session(u64::MAX - 1);
    send_input(&mut s, &mut p);
    assert!(s.next_event(999).is_err());
    assert!(s.lifecycle().input().is_some());
    s.prepare_input(2, false).unwrap().commit();
    push(
        &mut p,
        ShellNativeLauncherRecord::Input(NativeLauncherInput {
            event: NativeLauncherEvent {
                binding: binding(),
                event_id: 12,
                state_revision: 3,
            },
            issued_mono_usec: 600_000,
            kind: NativeLauncherInputKind::Text,
            text: "c".into(),
        }),
    );
    drive_session(&mut s, &mut p, |s, _| s.next_event(1000).unwrap().is_some());
    assert!(matches!(
        s.prepare_input(2, false),
        Err(ShellClientError::InvalidArgument(
            "native transaction exhausted"
        ))
    ));
    assert!(s.lifecycle().input().is_some());
}

#[test]
fn session_requires_native_role_limits_and_nonzero_transaction() {
    let (c, _p) = ready();
    assert!(NativeLauncherSession::new(c, 0).is_err());
    let (mut c, _p) = ready();
    c.take_native_launcher_observation().unwrap();
    assert!(NativeLauncherSession::new(c, 1).is_err());
    let (c, _p) = connect(
        Profile {
            role: "bar",
            revision: 6,
            capabilities: 1,
            limits_published: false,
        },
        1,
    );
    assert!(matches!(
        NativeLauncherSession::new(c.unwrap(), 1),
        Err(ShellClientError::MissingCapability)
    ));
}

#[test]
fn session_disconnect_clears_retained_input_and_obligations() {
    let (mut s, mut p) = session(80);
    send_input(&mut s, &mut p);
    p.close();
    let end = Instant::now() + WAIT;
    while s.lifecycle().state() == NativeLifecycleState::Live {
        let _ = s.dispatch(Instant::now(), 1000, 262144);
        assert!(Instant::now() < end);
    }
    assert_eq!(s.lifecycle().state(), NativeLifecycleState::Ended);
    assert!(s.lifecycle().input().is_none());
    assert_eq!(s.obligations(), NativeObligations::default());
    assert!(s.prepare_input(2, false).is_err());
    assert!(s.next_event(1000).is_err());
}

#[test]
fn action_hint_is_not_restarted_when_head_is_read_again() {
    let (mut s, mut p) = session(80);
    let b = binding();
    let (kind, body) = encode_shell_file_transaction_body(&ShellFileTransactionRecord {
        transaction: tx(70),
        record: ShellContentRecord::Action(ContentAction {
            grant: b.grant,
            output: b.output,
            candidate_generation: b.candidate_generation,
            presentation_epoch: b.presentation_epoch,
            interaction_generation: b.interaction_generation,
            allocation: b.allocation,
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            event_id: 123,
            kind: 1,
            reason: 0,
        }),
    })
    .unwrap();
    let event = p.event(kind, &body);
    p.push(event);
    drive_session(&mut s, &mut p, |s, _| s.next_event(1000).unwrap().is_some());
    let due = 1000 + u64::from(ContentLimits::prototype(b.grant).action_ack_timeout_ms);
    assert_eq!(s.obligations().action_due_ms, Some(due));
    s.next_event(1100).unwrap();
    assert_eq!(s.obligations().action_due_ms, Some(due));
    assert!(s.done().is_err());
    drop(s.prepare_action(2, false).unwrap());
    assert_eq!(s.obligations().action_due_ms, Some(due));
    s.prepare_action(2, false).unwrap().commit();
    assert_eq!(s.obligations().action_due_ms, None);
}

#[test]
fn explicit_session_close_disarms_input_without_waiting_for_peer() {
    let (mut s, mut p) = session(80);
    send_input(&mut s, &mut p);
    s.close();
    s.close();
    assert_eq!(s.lifecycle().state(), NativeLifecycleState::Ended);
    assert!(s.lifecycle().input().is_none());
    assert_eq!(s.obligations(), NativeObligations::default());
    assert!(s.prepare_input(2, false).is_err());
}

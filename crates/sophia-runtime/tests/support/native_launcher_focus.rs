use super::*;

#[path = "native_launcher_deadlines.rs"]
mod deadlines;
#[path = "native_launcher_input_limits.rs"]
mod input_limits;

fn setup() -> (
    ContentEpochRegistry,
    Peer,
    Vec<ContentAllocationSnapshot>,
    ShellApplicationCatalog,
) {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    (r, peer, allocations, catalog())
}
fn present(
    r: &mut ContentEpochRegistry,
    p: &mut Peer,
    a: &[ContentAllocationSnapshot],
    c: &ShellApplicationCatalog,
    generation: u64,
    revision: u64,
    drain: bool,
) {
    p.transport
        .grant_content_permit(r, tx(3), OUTPUT, generation, generation, 0)
        .unwrap();
    p.transport.poll_io(r).unwrap();
    assert!(matches!(
        decode_shell_content_frame(&p.read()).unwrap().1,
        ShellContentRecord::FramePermit(_)
    ));
    let mut b = begin();
    b.content.candidate_generation = generation;
    b.content.pacing_permit = generation;
    b.state_revision = revision;
    let mut ch = chunk();
    ch.candidate_generation = generation;
    let mut e = end();
    e.candidate_generation = generation;
    p.send(ShellNativeLauncherRecord::CandidateBegin(b));
    p.send(ShellNativeLauncherRecord::CandidateChunk(ch));
    p.send_content(ShellContentRecord::CandidateEnd(e));
    let current = NativeLauncherCandidateContext {
        state_revision: revision,
        ..native(c)
    };
    assert_eq!(
        p.transport
            .service_native_launcher_content(r, context(a), current, 0)
            .unwrap(),
        3
    );
    let _bundle = p
        .transport
        .begin_native_launcher_submission(r, generation, context(a), current, 0)
        .unwrap();
    p.transport
        .content_prepared(r, GRANT, OUTPUT, generation, 1, 1, 0)
        .unwrap();
    if generation == 1 {
        assert_eq!(
            p.transport.install_native_launcher_focus(r, tx(10)),
            Err(ShellTransportError::WrongCandidate)
        );
    }
    p.transport
        .content_presented(r, GRANT, OUTPUT, generation, generation + 10, 1, 1)
        .unwrap();
    if drain {
        p.transport.poll_io(r).unwrap();
        for kind in [1, 2] {
            assert!(
                matches!(decode_shell_content_frame(&p.read()).unwrap().1,ShellContentRecord::CandidateOutcome(v) if v.kind==kind && v.candidate_generation==generation)
            );
        }
    }
}
fn initial_focus(
    r: &mut ContentEpochRegistry,
    p: &mut Peer,
    a: &[ContentAllocationSnapshot],
    c: &ShellApplicationCatalog,
) -> NativeLauncherBinding {
    present(r, p, a, c, 1, 1, true);
    let f = p
        .transport
        .install_native_launcher_focus(r, tx(30))
        .unwrap();
    p.transport.poll_io(r).unwrap();
    assert_eq!(
        decode_shell_native_launcher_frame(&p.read()).unwrap().1,
        ShellNativeLauncherRecord::Focus(f)
    );
    f
}
fn input(
    p: &mut Peer,
    r: &mut ContentEpochRegistry,
    kind: NativeLauncherInputKind,
    text: &str,
    issued: u64,
) -> NativeLauncherInput {
    let binding = p.transport.native_launcher_focus().unwrap();
    let expected = p
        .transport
        .issue_native_launcher_input(r, binding, tx(40), kind, text, issued)
        .unwrap()
        .unwrap();
    p.transport.poll_io(r).unwrap();
    let (_, ShellNativeLauncherRecord::Input(v)) =
        decode_shell_native_launcher_frame(&p.read()).unwrap()
    else {
        panic!()
    };
    assert_eq!(v.event, expected);
    v
}
fn ack(
    p: &mut Peer,
    r: &mut ContentEpochRegistry,
    event: NativeLauncherEvent,
    disposition: u16,
) -> bool {
    p.send(ShellNativeLauncherRecord::InputAck(
        NativeLauncherInputAck { event, disposition },
    ));
    p.transport
        .poll_native_launcher_input_ack(r)
        .unwrap()
        .unwrap()
        .2
}
fn no_frame(p: &mut Peer, r: &mut ContentEpochRegistry) {
    p.transport.poll_io(r).unwrap();
    p.client.set_nonblocking(true).unwrap();
    let mut byte = [0];
    assert_eq!(
        p.client.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    p.client.set_nonblocking(false).unwrap();
}

#[test]
fn presented_outcome_precedes_focus_and_prepared_cannot_install_it() {
    let (mut r, mut p, a, c) = setup();
    assert_eq!(
        p.transport.install_native_launcher_focus(&mut r, tx(10)),
        Err(ShellTransportError::WrongCandidate)
    );
    present(&mut r, &mut p, &a, &c, 1, 1, false);
    let f = p
        .transport
        .install_native_launcher_focus(&mut r, tx(10))
        .unwrap();
    p.transport.poll_io(&mut r).unwrap();
    for kind in [1, 2] {
        assert!(
            matches!(decode_shell_content_frame(&p.read()).unwrap().1,ShellContentRecord::CandidateOutcome(v) if v.kind==kind)
        );
    }
    assert_eq!(
        decode_shell_native_launcher_frame(&p.read()).unwrap().1,
        ShellNativeLauncherRecord::Focus(f)
    );
    assert_eq!(f.grant, GRANT);
    assert_eq!(f.opening, 7);
    assert_eq!(f.candidate_generation, 1);
    assert_eq!(f.presentation_epoch, 11);
    assert_eq!(f.focus_lease, 1);
    assert_eq!(
        p.transport
            .install_native_launcher_focus(&mut r, tx(11))
            .unwrap(),
        f
    );
    no_frame(&mut p, &mut r);
}

#[test]
fn maximum_utf8_input_and_exact_ack_keep_original_binding() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let text = "é".repeat(128);
    let v = input(&mut p, &mut r, NativeLauncherInputKind::Text, &text, 10);
    assert_eq!(v.text, text);
    assert_eq!(v.event.binding, focus);
    assert_eq!(v.event.state_revision, 2);
    assert_eq!(v.issued_mono_usec, 10);
    let mut wrong = v.event;
    wrong.binding.focus_lease += 1;
    assert!(!ack(&mut p, &mut r, wrong, 1));
    assert!(ack(&mut p, &mut r, v.event, 1));
    assert!(!ack(&mut p, &mut r, v.event, 1));
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 2);
    assert!(
        p.transport
            .issue_native_launcher_input(
                &r,
                focus,
                tx(41),
                NativeLauncherInputKind::Text,
                &"é".repeat(129),
                11
            )
            .is_err()
    );
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 2);
}

#[test]
fn enter_waits_for_exact_revision_and_keeps_original_issuance_time() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let edit = input(&mut p, &mut r, NativeLauncherInputKind::Text, "app", 10);
    let before = p.transport.content_accounting(&r).response_records;
    assert_eq!(
        p.transport
            .issue_native_launcher_input(&r, focus, tx(41), NativeLauncherInputKind::Accept, "", 11)
            .unwrap(),
        None
    );
    assert_eq!(
        p.transport.content_accounting(&r).response_records,
        before + 1
    );
    no_frame(&mut p, &mut r);
    present(&mut r, &mut p, &a, &c, 2, 2, true);
    assert!(p.transport.native_launcher_focus().is_none());
    assert!(
        p.transport
            .issue_native_launcher_input(&r, focus, tx(42), NativeLauncherInputKind::Text, "x", 12)
            .is_err()
    );
    let current = p
        .transport
        .install_native_launcher_focus(&mut r, tx(43))
        .unwrap();
    p.transport.poll_io(&mut r).unwrap();
    assert!(current.focus_lease > focus.focus_lease);
    assert!(
        matches!(decode_shell_native_launcher_frame(&p.read()).unwrap().1,ShellNativeLauncherRecord::FocusRevoked(v) if v.binding==focus)
    );
    assert_eq!(
        decode_shell_native_launcher_frame(&p.read()).unwrap().1,
        ShellNativeLauncherRecord::Focus(current)
    );
    let (transaction, ShellNativeLauncherRecord::Input(enter)) =
        decode_shell_native_launcher_frame(&p.read()).unwrap()
    else {
        panic!()
    };
    assert_eq!(transaction, tx(41));
    assert_eq!(enter.kind, NativeLauncherInputKind::Accept);
    assert_eq!(enter.issued_mono_usec, 11);
    assert_eq!(enter.event.binding, current);
    assert_eq!(enter.event.state_revision, 2);
    assert!(ack(&mut p, &mut r, edit.event, 1));
    assert!(ack(&mut p, &mut r, enter.event, 2));
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 2);
    no_frame(&mut p, &mut r);
}

#[test]
fn a_later_edit_invalidates_unsent_enter_instead_of_retargeting_it() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    input(&mut p, &mut r, NativeLauncherInputKind::Text, "a", 10);
    assert_eq!(
        p.transport
            .issue_native_launcher_input(&r, focus, tx(41), NativeLauncherInputKind::Accept, "", 11)
            .unwrap(),
        None
    );
    input(&mut p, &mut r, NativeLauncherInputKind::Next, "", 12);
    present(&mut r, &mut p, &a, &c, 2, 3, true);
    p.transport
        .install_native_launcher_focus(&mut r, tx(43))
        .unwrap();
    p.transport.poll_io(&mut r).unwrap();
    assert!(matches!(
        decode_shell_native_launcher_frame(&p.read()).unwrap().1,
        ShellNativeLauncherRecord::FocusRevoked(_)
    ));
    assert!(
        matches!(decode_shell_native_launcher_frame(&p.read()).unwrap().1,ShellNativeLauncherRecord::Focus(v) if v.state_revision==3)
    );
    no_frame(&mut p, &mut r);
}

#[test]
fn bounded_receipts_refuse_new_input_without_advancing_revision() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let mut first = None;
    for issued in 1..=16 {
        let v = input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", issued);
        first.get_or_insert(v.event);
    }
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 17);
    assert_eq!(
        p.transport.issue_native_launcher_input(
            &r,
            focus,
            tx(41),
            NativeLauncherInputKind::Text,
            "x",
            17
        ),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 17);
    assert!(ack(&mut p, &mut r, first.unwrap(), 1));
    let v = input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 17);
    assert_eq!(v.event.state_revision, 18);
}

#[test]
fn closing_uses_owned_credits_and_disarms_before_more_input() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let frame = encode_shell_content_frame(
        tx(50),
        &ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: GRANT,
            facts_generation: 5,
            outputs: vec![facts()],
        }),
    )
    .unwrap();
    let mut sent = 0;
    let mut saturated = false;
    for _ in 0..4096 {
        match p.transport.send_async(&mut r, frame.clone()) {
            Ok(()) => sent += 1,
            Err(ShellTransportError::ActivationQueueSaturated) => {
                saturated = true;
                break;
            }
            Err(error) => panic!("unexpected send failure: {error}"),
        }
    }
    assert!(saturated);
    let count = p.transport.content_accounting(&r).response_records;
    p.transport
        .close_native_launcher(&r, opening(), tx(51), ContentReason::Cancelled)
        .unwrap();
    assert!(p.transport.native_launcher_state().is_none());
    assert!(p.transport.native_launcher_focus().is_none());
    assert_eq!(p.transport.content_accounting(&r).response_records, count);
    assert!(
        p.transport
            .issue_native_launcher_input(&r, focus, tx(52), NativeLauncherInputKind::Text, "x", 20)
            .is_err()
    );
    p.transport.poll_io(&mut r).unwrap();
    let mut terminal = Vec::new();
    for _ in 0..sent + 2 {
        p.transport.poll_io(&mut r).unwrap();
        let bytes = p.read();
        let kind = u16::from_le_bytes([bytes[6], bytes[7]]);
        if kind == 192 || kind == 197 {
            terminal.push(decode_shell_native_launcher_frame(&bytes).unwrap().1);
        }
        if kind == 197 {
            break;
        }
    }
    assert!(
        matches!(terminal.as_slice(),[ShellNativeLauncherRecord::FocusRevoked(v),ShellNativeLauncherRecord::Closed(closed)] if v.binding==focus && closed.opening==7)
    );
    assert!(
        p.transport
            .publish_native_launcher_opening(&r, tx(53), opening())
            .is_err()
    );
    let mut newer = opening();
    newer.opening += 1;
    p.transport
        .publish_native_launcher_opening(&r, tx(54), newer)
        .unwrap();
    assert!(p.transport.native_launcher_focus().is_none());
}

#[test]
fn stale_caller_revision_cannot_override_the_actual_issued_revision() {
    let (mut r, mut p, a, c) = setup();
    initial_focus(&mut r, &mut p, &a, &c);
    input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 10);
    assert_eq!(
        p.transport
            .service_native_launcher_content(&mut r, context(&a), native(&c), 0),
        Err(ShellTransportError::WrongCandidate)
    );
    assert!(matches!(
        p.transport
            .begin_native_launcher_submission(&mut r, 1, context(&a), native(&c), 0),
        Err(ShellTransportError::WrongCandidate)
    ));
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 2);
}

#[test]
fn old_focus_and_opening_callbacks_cannot_act_on_replacements() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let mut wrong = focus;
    wrong.presentation_epoch += 1;
    assert!(
        p.transport
            .issue_native_launcher_input(&r, wrong, tx(60), NativeLauncherInputKind::Text, "x", 20)
            .is_err()
    );
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 1);
    let mut wrong_opening = opening();
    wrong_opening.opening += 1;
    assert!(
        p.transport
            .close_native_launcher(&r, wrong_opening, tx(61), ContentReason::Cancelled)
            .is_err()
    );
    assert_eq!(p.transport.native_launcher_focus(), Some(focus));
    p.transport
        .close_native_launcher(&r, opening(), tx(62), ContentReason::Cancelled)
        .unwrap();
    p.transport
        .publish_native_launcher_opening(&r, tx(63), wrong_opening)
        .unwrap();
    assert!(
        p.transport
            .close_native_launcher(&r, opening(), tx(64), ContentReason::Cancelled)
            .is_err()
    );
    assert_eq!(
        p.transport.native_launcher_state().unwrap().0,
        wrong_opening
    );
}

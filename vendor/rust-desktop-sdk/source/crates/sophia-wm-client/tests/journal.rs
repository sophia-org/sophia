use sophia_wm_client::{
    journal::{Control, Error, Event, Journal},
    protocol::{wm_files::*, *},
};

fn record(kind: u16, sequence: u64, body: &[u8]) -> Vec<u8> {
    let mut b = ((32 + body.len()) as u32).to_le_bytes().to_vec();
    b.extend(1u16.to_le_bytes());
    b.extend(kind.to_le_bytes());
    b.extend(7u64.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend(sequence.to_le_bytes());
    b.extend(body);
    b
}
fn negotiated(caps: u64) -> Vec<u8> {
    record(16, 1, &caps.to_le_bytes())
}
fn submitted(sequence: u64) -> Vec<u8> {
    let mut b = 11u64.to_le_bytes().to_vec();
    b.extend(260u16.to_le_bytes());
    b.extend([0; 6]);
    record(17, sequence, &b)
}
fn outcome(sequence: u64) -> Vec<u8> {
    let mut b = 19u64.to_le_bytes().to_vec();
    b.extend(3u64.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend([0; 6]);
    record(21, sequence, &b)
}
fn journal() -> Journal {
    Journal::new(
        7,
        WmFileLimits {
            capability_ceiling: u64::MAX,
            profile_required: false,
        },
        WmFileNegotiationOffer {
            required_capabilities: 0,
            optional_capabilities: u64::MAX,
        },
        0,
    )
    .unwrap()
}
fn ready() -> Journal {
    let mut j = journal();
    j.append(
        &negotiated(SOPHIA_WM_CAPABILITY_CONFIGURATION),
        1,
        |_, _| Ok(()),
    )
    .unwrap();
    j
}

#[test]
fn held_application_head_does_not_hide_custody_or_allow_ack_to_skip() {
    let mut j = ready();
    j.append(&outcome(2), 2, |_, _| {
        panic!("application event is not a control")
    })
    .unwrap();
    let before = j.head().unwrap().1.clone();
    let mut controls = vec![];
    j.append(&submitted(3), 3, |h, c| {
        controls.push((h.sequence, c));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        controls,
        [(
            3,
            Control::Submitted(WmFileSubmitted {
                submission_id: 11,
                candidate_kind: WmFileKind::Configuration,
            })
        )]
    );
    assert_eq!(j.head().unwrap().1, &before);
    assert_eq!(j.consumed(), 1);
    assert_eq!(j.consume(), Ok(3));
    assert!(j.head().is_none());
}

#[test]
fn malformed_record_stops_later_custody_and_hides_application_head() {
    let mut j = ready();
    let mut bad = outcome(2);
    bad[54] = 1;
    bad.extend(submitted(3));
    assert_eq!(
        j.append(&bad, 2, |_, _| panic!("must not reach Submitted")),
        Err(Error::Invalid)
    );
    assert!(j.head().is_none());
    assert_eq!(j.capacity(), 0);
    assert_eq!(
        j.append(&submitted(4), 3, |_, _| panic!()),
        Err(Error::Invalid)
    );
}

#[test]
fn earlier_valid_control_survives_later_malformed_record() {
    let mut j = ready();
    let mut calls = 0;
    let mut bytes = submitted(2);
    bytes.extend([0; 4]);
    assert_eq!(
        j.append(&bytes, 2, |_, _| {
            calls += 1;
            Ok(())
        }),
        Err(Error::Invalid)
    );
    assert_eq!(calls, 1);
}

#[test]
fn fragment_deadline_does_not_extend_for_drip_feed() {
    let mut j = ready();
    let bytes = outcome(2);
    j.append(&bytes[..2], 10, |_, _| panic!()).unwrap();
    assert_eq!(j.fragment_deadline(), Some(4010));
    j.append(&bytes[2..3], 1000, |_, _| panic!()).unwrap();
    assert_eq!(j.fragment_deadline(), Some(4010));
    assert_eq!(
        j.append(&bytes[3..], 4010, |_, _| panic!()),
        Err(Error::Deadline)
    );
}

#[test]
fn completing_a_fragment_starts_a_new_deadline_for_the_next_record() {
    let mut j = ready();
    let first = outcome(2);
    let second = outcome(3);
    j.append(&first[..3], 10, |_, _| panic!()).unwrap();
    let mut tail = first[3..].to_vec();
    tail.extend(&second[..3]);
    j.append(&tail, 1000, |_, _| panic!()).unwrap();
    assert_eq!(j.fragment_deadline(), Some(5000));
    let offset = j.offset();
    assert_eq!(
        j.append(&second[3..], 999, |_, _| panic!()),
        Err(Error::ClockRegression)
    );
    assert_eq!(j.offset(), offset);
    assert_eq!(j.fault(), None);
    j.append(&second[3..], 4999, |_, _| panic!()).unwrap();
    assert_eq!(j.fragment_deadline(), None);
    assert_eq!(j.consume(), Ok(2));
    assert_eq!(j.consume(), Ok(3));
}

#[test]
fn every_split_reassembles_once_and_preserves_event_identity() {
    let bytes = outcome(2);
    for split in 1..bytes.len() {
        let mut j = ready();
        j.append(&bytes[..split], 10, |_, _| panic!()).unwrap();
        assert!(j.head().is_none());
        j.append(&bytes[split..], 11, |_, _| panic!()).unwrap();
        let (header, event) = j.head().unwrap();
        assert_eq!(header.sequence, 2);
        assert!(matches!(event, Event::ConfigurationOutcome(v) if v.transaction.raw() == 19));
        assert_eq!(j.fragment_deadline(), None);
        assert_eq!(j.consume(), Ok(2));
    }
}

#[test]
fn callback_refusal_stops_the_scan() {
    let mut j = ready();
    let mut bytes = submitted(2);
    bytes.extend(submitted(3));
    let mut calls = 0;
    assert_eq!(
        j.append(&bytes, 2, |_, _| {
            calls += 1;
            Err(Error::Invalid)
        }),
        Err(Error::Invalid)
    );
    assert_eq!(calls, 1);
}

#[test]
fn repeated_sequence_stale_epoch_and_event_before_negotiation_fail() {
    let mut j = journal();
    assert_eq!(j.append(&outcome(1), 0, |_, _| Ok(())), Err(Error::Invalid));
    let mut j = ready();
    assert_eq!(j.append(&outcome(1), 2, |_, _| Ok(())), Err(Error::Invalid));
    let mut j = ready();
    let mut stale = outcome(2);
    stale[8] = 8;
    assert_eq!(j.append(&stale, 2, |_, _| Ok(())), Err(Error::Invalid));
}

#[test]
fn retained_journal_is_limited_to_64_records() {
    let mut j = ready();
    let mut bytes = vec![];
    for sequence in 2..=65 {
        bytes.extend(outcome(sequence));
    }
    j.append(&bytes, 2, |_, _| panic!()).unwrap();
    assert!(j.head().is_some());
    assert_eq!(
        j.append(&outcome(66), 3, |_, _| panic!()),
        Err(Error::Overflow)
    );
}

#[test]
fn lifecycle_and_chord_causes_require_their_negotiated_capabilities() {
    let all = (1u64 << 23) - 1;
    for kind in [7u16, 8] {
        let removed = if kind == 7 {
            SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE | SOPHIA_WM_CAPABILITY_CHORD_ACTIONS
        } else {
            SOPHIA_WM_CAPABILITY_CHORD_ACTIONS
        };
        for enabled in [false, true] {
            let mut j = journal();
            j.append(
                &negotiated(if enabled { all } else { all & !removed }),
                0,
                |_, _| Ok(()),
            )
            .unwrap();
            let mut body = vec![];
            for id in [55u64, 56, 31, 9, 3] {
                body.extend(id.to_le_bytes());
            }
            body.extend(kind.to_le_bytes());
            body.extend(1u16.to_le_bytes());
            body.extend([0; 4]);
            body.extend(1u64.to_le_bytes());
            if kind == 7 {
                body.extend(41u64.to_le_bytes());
                body.extend(186u64.to_le_bytes());
                body.extend(2u16.to_le_bytes());
                body.extend(1u16.to_le_bytes());
                body.extend(3u32.to_le_bytes());
            } else {
                for id in [43u64, 41, 186] {
                    body.extend(id.to_le_bytes());
                }
            }
            let result = j.append(&record(22, 2, &body), 1, |_, _| panic!());
            if !enabled {
                assert_eq!(result, Err(Error::Invalid));
                assert!(j.head().is_none());
            } else {
                result.unwrap();
                let Event::Cycle(cycle) = j.head().unwrap().1 else {
                    panic!("expected Cycle")
                };
                match cycle.request.cause {
                    PolicyRequestCause::ActionLifecycle {
                        activation_serial,
                        action,
                        count,
                        ..
                    } => {
                        assert_eq!((activation_serial, action.raw(), count), (41, 186, 3));
                    }
                    PolicyRequestCause::ChordAction {
                        activation_serial,
                        chord_serial,
                        action,
                    } => {
                        assert_eq!(
                            (activation_serial, chord_serial, action.raw()),
                            (43, 41, 186)
                        );
                    }
                    _ => panic!("wrong cause"),
                }
                assert_eq!(j.consume(), Ok(2));
            }
        }
    }
}

#[test]
fn negotiation_rejects_every_missing_dependency_and_duplicate_selection() {
    for caps in [
        SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT,
        SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE,
        SOPHIA_WM_CAPABILITY_CHORD_ACTIONS,
        SOPHIA_WM_CAPABILITY_HELD_CAPTURE,
    ] {
        let mut j = journal();
        assert_eq!(
            j.append(&negotiated(caps), 1, |_, _| panic!()),
            Err(Error::Invalid)
        );
    }
    let mut j = ready();
    let mut second = negotiated(SOPHIA_WM_CAPABILITY_CONFIGURATION);
    second[24] = 2;
    assert_eq!(j.append(&second, 2, |_, _| panic!()), Err(Error::Invalid));
}

#[test]
fn negotiation_enforces_offer_ceiling_required_and_profile_constraints() {
    for (ceiling, required, optional, profile, selected) in [
        (0, 0, 1, false, 1),
        (1, 0, 0, false, 1),
        (1, 1, 0, false, 0),
        (u64::MAX, 0, u64::MAX, true, 0),
    ] {
        let mut j = Journal::new(
            7,
            WmFileLimits {
                capability_ceiling: ceiling,
                profile_required: profile,
            },
            WmFileNegotiationOffer {
                required_capabilities: required,
                optional_capabilities: optional,
            },
            0,
        )
        .unwrap();
        assert_eq!(
            j.append(&negotiated(selected), 1, |_, _| panic!()),
            Err(Error::Invalid)
        );
    }
}

//! The panel ceiling has two owners, and this is where they are made to agree.
//!
//! `sophia-config` depends on nothing else in the stack, so it cannot read the
//! wire's own maximum and carries its own copy. This crate sees both, so a
//! drift between them fails here rather than in a session that accepts a
//! profile the wire will refuse.

#[test]
fn the_profile_ceiling_is_the_wire_ceiling() {
    assert_eq!(
        sophia_config::SHELL_PANEL_MAX_THICKNESS_PX,
        sophia_protocol::SOPHIA_SHELL_MAX_RESERVATION_THICKNESS_PX,
        "a profile may promise exactly the panel the wire can carry",
    );
}

#[test]
fn the_compiled_switcher_reserves_nothing_and_the_configured_ceiling_round_trips() {
    let profile =
        sophia_config::load_desktop_profile(None, sophia_config::ConfigGeneration::INITIAL)
            .expect("the compiled profile loads");
    assert_eq!(
        sophia_config::desktop_profile_shell_panel_thickness(&profile),
        None,
        "a transient switcher is not a persistent panel",
    );
    let thickness = sophia_config::SHELL_PANEL_MAX_THICKNESS_PX;

    // An explicitly configured claim at the profile ceiling still has to be a
    // value the shell candidate codec accepts.
    let candidate = sophia_protocol::ShellV1Candidate {
        connection_epoch: 1,
        snapshot_generation: 1,
        candidate_generation: 1,
        output: sophia_protocol::OutputId::from_raw(1),
        visible: true,
        selected_slot: Some(1),
        reservation: Some(sophia_protocol::ShellV1WorkAreaReservation {
            edge: sophia_protocol::ShellV1ReservationEdge::Bottom,
            thickness_px: thickness,
        }),
        entries: vec![sophia_protocol::ShellV1CandidateEntry {
            slot: 1,
            generation: 1,
        }],
    };
    use sophia_protocol::shell_files::*;
    let record = ShellFileDescriptorRecord {
        transaction: sophia_protocol::TransactionId::from_raw(1),
        record: ShellDescriptorRecord::DescriptorCandidate(candidate),
    };
    let frame = encode_shell_file_descriptor(
        ShellFileHeader {
            kind: ShellFileKind::DescriptorCandidate,
            connection_epoch: 1,
            submission_id: 1,
            sequence: 0,
        },
        &record,
    )
    .expect("the compiled panel encodes");
    let decoded = decode_shell_file_descriptor(&frame, ShellFileKind::DescriptorCandidate)
        .expect("and decodes back");
    assert_eq!(decoded, record);
}

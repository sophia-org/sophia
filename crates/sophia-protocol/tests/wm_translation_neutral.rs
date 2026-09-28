//! The translation group records of `wm_translation.rs` without the socket
//! codecs: the shared sections round trip and malformed rows fail closed.
//! The legacy chunk ordinal stays in `wm_translation.rs` and retires with the
//! socket wire (t269).
use sophia_protocol::*;

fn groups() -> Vec<PolicyTranslationGroup> {
    vec![PolicyTranslationGroup {
        output: OutputId::from_raw(1),
        group: 7,
        x: -1268,
        y: 0,
        members: vec![SurfaceId::new(1, 1), SurfaceId::new(2, 1)],
    }]
}

fn decode(
    sections: &[PolicyRecordSection],
) -> Result<Vec<PolicyTranslationGroup>, BinaryCodecError> {
    let refs = sections
        .iter()
        .map(PolicyRecordSection::as_ref)
        .collect::<Vec<_>>();
    decode_policy_translation_groups_records(&refs)
}

/// `shared_translation_round_trip_and_malformed_members_fail_closed`.
#[test]
fn shared_translation_records_round_trip_and_malformed_members_fail_closed() {
    let sections = encode_policy_translation_groups_records(&groups()).unwrap();
    assert_eq!(
        sections.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [
            PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
            PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
        ]
    );
    assert_eq!(decode(&sections).unwrap(), groups());
    let mut corrupt = sections.clone();
    corrupt[0].bytes[28] = 1;
    assert!(decode(&corrupt).is_err());
    let mut corrupt = sections.clone();
    corrupt[1].bytes[8] = 8;
    assert!(decode(&corrupt).is_err());
    let mut corrupt = sections.clone();
    corrupt[1].bytes[20..24].fill(0);
    assert!(decode(&corrupt).is_err());
    let mut corrupt = sections.clone();
    corrupt.pop();
    assert!(decode(&corrupt).is_err());
    for index in 0..sections.len() {
        let mut corrupt = sections.clone();
        corrupt[index].bytes.pop();
        assert!(decode(&corrupt).is_err());
    }
}

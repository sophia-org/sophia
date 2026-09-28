use sophia_protocol::output_files::*;
use sophia_protocol::*;

fn welcome() -> OutputV1ServerWelcome {
    OutputV1ServerWelcome {
        selected_revision: 1,
        capabilities: 3,
        connection_epoch: 9,
        max_heads: 16,
        max_groups: 16,
        max_modes_per_head: 128,
        max_heads_per_group: 4,
    }
}

const NEGOTIATED: [u8; 24] = [
    1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 16, 0, 16, 0, 128, 0, 4, 0,
];
const REFUSED: [u8; 8] = [1, 0, 0, 0, 0, 0, 0, 0];
const PUBLISHED: [u8; 24] = [
    2, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0,
];

#[test]
fn literal_events_preserve_the_existing_welcome_and_distinct_publication_identity() {
    assert_eq!(
        decode_output_file_negotiated(&NEGOTIATED, 9).unwrap(),
        welcome()
    );
    assert_eq!(
        encode_output_file_negotiated(welcome()).unwrap(),
        NEGOTIATED
    );
    let mut observe_only = welcome();
    observe_only.capabilities = 1;
    assert_eq!(
        decode_output_file_negotiated(&encode_output_file_negotiated(observe_only).unwrap(), 9)
            .unwrap(),
        observe_only
    );
    assert_eq!(
        decode_output_file_refused(&REFUSED).unwrap(),
        OutputFileRefusal::UnsupportedRevision
    );
    assert_eq!(
        encode_output_file_refused(OutputFileRefusal::UnsupportedRevision),
        REFUSED
    );
    assert_eq!(
        encode_output_file_refused(OutputFileRefusal::ObservationRequired),
        [2, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        decode_output_file_refused(&[2, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
        OutputFileRefusal::ObservationRequired
    );
    let publication = OutputFilePublication {
        topology_epoch: 7,
        qid_path: 42,
    };
    assert_eq!(
        decode_output_file_publication(&PUBLISHED).unwrap(),
        publication
    );
    assert_eq!(
        encode_output_file_publication(publication).unwrap(),
        PUBLISHED
    );
}

#[test]
fn events_refuse_truncation_trailing_data_and_all_reserved_bytes() {
    type Decode = fn(&[u8]) -> bool;
    let fixtures: [(&[u8], Decode); 3] = [
        (&NEGOTIATED, |b| decode_output_file_negotiated(b, 9).is_ok()),
        (&REFUSED, |b| decode_output_file_refused(b).is_ok()),
        (&PUBLISHED, |b| decode_output_file_publication(b).is_ok()),
    ];
    for (bytes, decode) in fixtures {
        for end in 0..bytes.len() {
            assert!(!decode(&bytes[..end]));
        }
        let mut extra = bytes.to_vec();
        extra.push(0);
        assert!(!decode(&extra));
        for offset in 2..8 {
            let mut bad = bytes.to_vec();
            bad[offset] = 1;
            assert!(!decode(&bad));
        }
    }
}

#[test]
fn negotiated_values_are_bounded_and_cannot_grant_unknown_capabilities() {
    assert!(decode_output_file_negotiated(&NEGOTIATED, 0).is_err());
    for offset in [0, 8, 16, 18, 20, 22] {
        let mut bad = NEGOTIATED;
        bad[offset] = 0;
        assert!(
            decode_output_file_negotiated(&bad, 9).is_err(),
            "zero {offset}"
        );
        bad[offset] = 255;
        assert!(
            decode_output_file_negotiated(&bad, 9).is_err(),
            "excess {offset}"
        );
    }
    for capabilities in [0, 2, 1 | (1 << 63)] {
        let mut invalid = welcome();
        invalid.capabilities = capabilities;
        assert!(encode_output_file_negotiated(invalid).is_err());
        let mut bad = NEGOTIATED;
        bad[8..16].copy_from_slice(&capabilities.to_le_bytes());
        assert!(decode_output_file_negotiated(&bad, 9).is_err());
    }
}

#[test]
fn publication_requires_topology_kind_and_both_nonzero_identities() {
    for offset in [0, 8, 16] {
        let mut bad = PUBLISHED;
        bad[offset] = 0;
        assert!(decode_output_file_publication(&bad).is_err());
    }
    let mut bad = PUBLISHED;
    bad[0] = OutputFileKind::Limits as u8;
    assert!(decode_output_file_publication(&bad).is_err());
    for publication in [
        OutputFilePublication {
            topology_epoch: 0,
            qid_path: 42,
        },
        OutputFilePublication {
            topology_epoch: 7,
            qid_path: 0,
        },
    ] {
        assert!(encode_output_file_publication(publication).is_err());
    }
    for kind in [0, 3, 255] {
        let mut bad = REFUSED;
        bad[0] = kind;
        assert!(decode_output_file_refused(&bad).is_err());
    }
}

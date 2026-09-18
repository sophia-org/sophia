use sophia_protocol::*;
#[path = "../examples/support/catalog_action_fixtures.rs"]
mod fixtures;
use fixtures::{action, records};
#[test]
fn persistent_catalog_roundtrip_and_all_truncations() {
    for record in records() {
        let tx = TransactionId::from_raw(17);
        let bytes = encode_shell_catalog_action_frame(tx, &record).unwrap();
        assert_eq!(
            decode_shell_catalog_action_frame(&bytes).unwrap(),
            (tx, record.clone())
        );
        for end in 0..bytes.len() {
            assert!(decode_shell_catalog_action_frame(&bytes[..end]).is_err());
        }
        assert!(encode_shell_catalog_action_frame(TransactionId::from_raw(0), &record).is_err());
        assert!(decode_shell_native_launcher_frame(&bytes).is_err());
        assert!(decode_shell_content_frame(&bytes).is_err());
    }
}
#[test]
fn stable_identity_is_not_a_label_and_is_bounded() {
    for identity in ["", "terminal", "registered:", "desktop:", "registered:a\n"] {
        let record = ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
            connection_epoch: 2,
            catalog_generation: 1,
            slot: 1,
            identity: identity.into(),
        });
        assert!(encode_shell_catalog_action_frame(TransactionId::from_raw(1), &record).is_err());
    }
    let oversized = ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
        connection_epoch: 2,
        catalog_generation: 1,
        slot: 1,
        identity: format!("registered:{}", "x".repeat(256)),
    });
    assert!(encode_shell_catalog_action_frame(TransactionId::from_raw(1), &oversized).is_err());
}
#[test]
fn cancellation_and_wrong_action_family_never_become_catalog_requests() {
    for (kind, slot, generation) in [(2, 9, 11), (1, 4097, 11), (1, 9, 0)] {
        let mut action = action();
        action.kind = kind;
        action.action_id = slot;
        assert!(
            encode_shell_catalog_action_frame(
                TransactionId::from_raw(1),
                &ShellCatalogActionRecord::Activate(CatalogActivation {
                    action,
                    catalog_generation: generation
                })
            )
            .is_err()
        );
    }
    let ShellCatalogActionRecord::CandidateChunk(mut chunk) = records().remove(2) else {
        unreachable!()
    };
    assert!(
        encode_shell_content_frame(
            TransactionId::from_raw(1),
            &ShellContentRecord::CandidateChunk(chunk.clone())
        )
        .is_err()
    );
    for kind in [1, 2, 4] {
        chunk.targets[0].action_kind = kind;
        assert!(
            encode_shell_catalog_action_frame(
                TransactionId::from_raw(1),
                &ShellCatalogActionRecord::CandidateChunk(chunk.clone())
            )
            .is_err()
        );
    }
}

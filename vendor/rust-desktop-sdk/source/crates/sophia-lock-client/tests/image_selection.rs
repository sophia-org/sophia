use sophia_lock_client::{
    Candidate,
    journal::{Control, Error, Journal},
    protocol::lock_files::*,
};

fn offer(
    minimum_revision: u16,
    maximum_revision: u16,
    requested_capabilities: u64,
) -> LockNegotiate {
    LockNegotiate {
        minimum_revision,
        maximum_revision,
        requested_capabilities,
        chords: vec![],
    }
}
fn record(kind: u16, epoch: u64, sequence: u64, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 32];
    bytes[..4].copy_from_slice(&((32 + body.len()) as u32).to_le_bytes());
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&kind.to_le_bytes());
    bytes[8..16].copy_from_slice(&epoch.to_le_bytes());
    bytes[24..32].copy_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}
fn welcome(revision: u16, caps: u64) -> Vec<u8> {
    let mut body = [0; 16];
    body[..2].copy_from_slice(&revision.to_le_bytes());
    body[8..].copy_from_slice(&caps.to_le_bytes());
    record(16, 7, 1, &body)
}
fn receipt() -> Vec<u8> {
    let mut body = [0; 16];
    body[..8].copy_from_slice(&9u64.to_le_bytes());
    body[8..10].copy_from_slice(&0x7000u16.to_le_bytes());
    record(18, 7, 2, &body)
}

#[test]
fn selection_controls_same_read_and_split_read_receipt_decoding() {
    for split in [false, true] {
        for revision in [1, 2] {
            let mut j = Journal::new(7, &offer(1, 2, 5)).unwrap();
            let mut bytes = welcome(revision, 1); // r2 is explicit even without image grants.
            if split {
                j.append(&bytes).unwrap();
                j.advance(|_, _| Ok(())).unwrap();
                bytes = receipt();
            } else {
                bytes.extend(receipt());
            }
            j.append(&bytes).unwrap();
            let mut saw_receipt = false;
            let result = j.advance(|_, c| {
                if let Control::Submitted(v) = c {
                    assert_eq!(v.candidate_kind, LockFileKind::ImageBegin);
                    assert_eq!(v.submission_id, 9);
                    saw_receipt = true;
                }
                Ok(())
            });
            assert_eq!(j.selected_revision(), Some(revision));
            assert_eq!(saw_receipt, revision == 2);
            assert_eq!(result.is_ok(), revision == 2);
        }
    }
}

#[test]
fn invalid_selection_identity_or_grants_never_install_revision() {
    for (min, max, selected, caps) in [
        (1, 1, 2, 1),
        (2, 2, 1, 1),
        (1, 2, 0, 1),
        (1, 2, 3, 1),
        (1, 2, 1, 5),
        (1, 2, 2, 9),
    ] {
        let mut j = Journal::new(7, &offer(min, max, 5)).unwrap();
        j.append(&welcome(selected, caps)).unwrap();
        assert_eq!(
            j.advance(|_, _| panic!("invalid welcome reached callback")),
            Err(Error::Invalid)
        );
        assert_eq!(j.selected_revision(), None);
        assert_eq!(j.consumed(), 0);
    }
    for offset in [8, 24] {
        let mut j = Journal::new(7, &offer(1, 2, 5)).unwrap();
        let mut bytes = welcome(2, 5);
        bytes[offset..offset + 8].fill(0);
        j.append(&bytes).unwrap();
        assert!(j.advance(|_, _| panic!()).is_err());
        assert_eq!(j.selected_revision(), None);
    }
    let mut j = Journal::new(7, &offer(1, 2, 5)).unwrap();
    j.append(&welcome(2, 5)).unwrap();
    assert!(j.advance(|_, _| Err(Error::Invalid)).is_err());
    assert_eq!(j.selected_revision(), None);
}

#[test]
fn image_offer_is_explicit_and_dma_is_not_requested() {
    assert!(Candidate::Negotiate(offer(1, 2, 5)).prepare(7, 1).is_ok());
    for value in [offer(1, 1, 5), offer(1, 2, 9), offer(1, 2, 13)] {
        assert!(Candidate::Negotiate(value).prepare(7, 1).is_err());
    }
}

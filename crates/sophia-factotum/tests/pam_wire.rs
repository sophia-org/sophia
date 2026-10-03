//! The agent-helper frames: exact bytes, and refusal of anything malformed.

use sophia_factotum::pam_wire::{
    FLAG_DISALLOW_NULL, HelperReply, HelperRequest, MAX_SECRET, WireError,
};
use sophia_factotum::proto::PamVerdict;
use sophia_factotum::secret::SecretBytes;

fn request(secret: &[u8]) -> HelperRequest {
    HelperRequest {
        flags: FLAG_DISALLOW_NULL,
        service: "sophia-lock".into(),
        user: "tb".into(),
        secret: SecretBytes::from_slice(secret),
    }
}

#[test]
fn a_request_round_trips_with_exact_bytes() {
    let frame = request(b"pw").encode().unwrap();
    let expected = [
        &[33, 0, 0, 0][..],
        b"SFPM",
        &[1, 2, 0, 0],
        &[11, 0],
        b"sophia-lock",
        &[2, 0],
        b"tb",
        &[2, 0],
        b"pw",
    ]
    .concat();
    assert_eq!(frame.as_slice(), expected);
    assert_eq!(HelperRequest::decode(frame.as_slice()), Ok(request(b"pw")));
}

#[test]
fn malformed_requests_are_refused() {
    let frame = request(b"pw").encode().unwrap().as_slice().to_vec();
    let mut trailing = frame.clone();
    trailing.push(0);
    assert_eq!(HelperRequest::decode(&trailing), Err(WireError::BadLength));
    let mut magic = frame.clone();
    magic[4] = b'X';
    assert_eq!(HelperRequest::decode(&magic), Err(WireError::BadMagic));
    let mut version = frame.clone();
    version[8] = 2;
    assert_eq!(HelperRequest::decode(&version), Err(WireError::BadVersion));
    let mut flags = frame.clone();
    flags[9] = 0x80;
    assert_eq!(HelperRequest::decode(&flags), Err(WireError::BadField));
    assert_eq!(
        HelperRequest::decode(&frame[..20]),
        Err(WireError::BadLength)
    );
}

#[test]
fn secrets_and_names_are_bounded_and_free_of_nul() {
    assert!(request(&vec![b'x'; MAX_SECRET]).encode().is_ok());
    assert_eq!(
        request(&vec![b'x'; MAX_SECRET + 1]).encode(),
        Err(WireError::BadField)
    );
    assert_eq!(request(b"a\0b").encode(), Err(WireError::BadField));
    let mut nameless = request(b"pw");
    nameless.user.clear();
    assert_eq!(nameless.encode(), Err(WireError::BadField));
}

#[test]
fn replies_round_trip_and_unknown_verdicts_refuse() {
    for verdict in [
        PamVerdict::Accepted,
        PamVerdict::Rejected,
        PamVerdict::Unavailable,
        PamVerdict::ConversationRefused,
    ] {
        let reply = HelperReply {
            verdict,
            pam_code: -7,
        };
        assert_eq!(HelperReply::decode(&reply.encode()), Ok(reply));
    }
    let mut unknown = HelperReply {
        verdict: PamVerdict::Accepted,
        pam_code: 0,
    }
    .encode();
    unknown[9] = 9;
    assert_eq!(HelperReply::decode(&unknown), Err(WireError::BadField));
}

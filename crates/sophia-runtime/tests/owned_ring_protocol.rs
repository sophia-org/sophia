#![cfg(all(target_os = "linux", feature = "experimental-owned-copy"))]

use sophia_protocol::{ContentAllocationId, ContentGrant, ContentOutputId, ContentResourceId};
use sophia_runtime::owned_ring_protocol::*;
use sophia_runtime::{OwnedCopyImageRequest, OwnedSlotIdentity};
fn identity() -> Identity {
    Identity {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 2,
        },
        channel: 3,
        ring: 4,
        generation: 5,
    }
}
fn offer() -> Offer {
    Offer {
        identity: identity(),
        request: OwnedCopyImageRequest {
            slot: OwnedSlotIdentity {
                offer: 1,
                slot: 3,
                generation: 2,
            },
            resource: ContentResourceId {
                id: 6,
                generation: 7,
            },
            allocation: ContentAllocationId {
                id: 8,
                generation: 9,
            },
            output: ContentOutputId {
                id: 10,
                generation: 11,
            },
            topology_generation: 12,
            width: 1920,
            height: 24,
        },
    }
}
fn set_word(bytes: &mut [u8], word: usize, value: u64) {
    bytes[8 + word * 8..16 + word * 8].copy_from_slice(&value.to_le_bytes());
}
#[test]
fn controls_roundtrip_and_refuse_cross_kind_or_inexact_frames() {
    let registration = Registration {
        identity: identity(),
        slots: 4,
        pixels: 1920 * 24 * 4,
    };
    assert_eq!(
        Control::decode(&registration.encode()),
        Some(Control::Register(registration))
    );
    assert_eq!(
        Registration::decode_source(&registration.source_packet()),
        Some(registration)
    );
    assert_eq!(
        Control::decode(&offer().encode()),
        Some(Control::Offer(offer()))
    );
    assert!(Control::decode(&registration.source_packet()).is_none());
    assert!(Registration::decode_source(&registration.encode()).is_none());
    for bytes in [registration.encode(), offer().encode()] {
        for n in 0..bytes.len() {
            assert!(Control::decode(&bytes[..n]).is_none());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Control::decode(&extra).is_none());
        let mut wrong_length = bytes.clone();
        wrong_length[0] ^= 1;
        assert!(Control::decode(&wrong_length).is_none());
        for i in 1..6 {
            let mut zero = bytes.clone();
            set_word(&mut zero, i, 0);
            assert!(Control::decode(&zero).is_none());
        }
    }
}
#[test]
fn controls_refuse_invalid_fixed_geometry_and_offer_shape() {
    for (slots, pixels) in [(0, 4), (5, 4), (1, 0), (1, 5), (1, (4 << 20) + 4)] {
        let r = Registration {
            identity: identity(),
            slots,
            pixels,
        };
        assert!(Control::decode(&r.encode()).is_none());
        assert!(Registration::decode_source(&r.source_packet()).is_none());
    }
    for (word, value) in [
        (6, 0),
        (7, 4),
        (8, 0),
        (8, 3),
        (16, u64::MAX),
        (17, u64::MAX),
        (16, 1 << 20),
    ] {
        let mut bytes = offer().encode();
        set_word(&mut bytes, word, value);
        assert!(
            Control::decode(&bytes).is_none(),
            "word {word}, value {value}"
        );
    }
    for word in 9..18 {
        let mut bytes = offer().encode();
        set_word(&mut bytes, word, 0);
        assert!(Control::decode(&bytes).is_none());
    }
}
#[test]
fn event_roundtrips_and_refuses_noncanonical_fields() {
    let mut released = Event::ring(EventKind::SlotReleased, identity(), 0);
    released.slot = offer().request.slot;
    released.resource = offer().request.resource;
    for mut event in [
        Event::ring(EventKind::RingAdmitted, identity(), 0),
        Event::ring(EventKind::RingReady, identity(), 0),
        Event::ring(EventKind::RingRefused, identity(), 1),
        released,
    ] {
        assert!(Event::decode(&event.encode()).is_none());
        event.sequence = 42;
        let bytes = event.encode();
        assert_eq!(bytes.len(), EVENT_BYTES);
        assert_eq!(Event::decode(&bytes), Some(event));
        for n in 0..bytes.len() {
            assert!(Event::decode(&bytes[..n]).is_none());
        }
        for (word, value) in [(0, 0), (0, 5), (6, 0), (12, 65536), (13, 2)] {
            let mut invalid = bytes.clone();
            set_word(&mut invalid, word, value);
            assert!(Event::decode(&invalid).is_none());
        }
        if event.kind != EventKind::SlotReleased {
            for word in 7..12 {
                let mut invalid = bytes.clone();
                set_word(&mut invalid, word, 1);
                assert!(Event::decode(&invalid).is_none());
            }
        }
    }
    released.sequence = 1;
    released.changed_generation = true;
    assert!(Event::decode(&released.encode()).is_none());
    released.reason = 1;
    assert_eq!(Event::decode(&released.encode()), Some(released));
    for (word, value) in [(7, 0), (8, 4), (9, 0), (9, 3), (10, 0), (11, 0)] {
        let mut invalid = released.encode();
        set_word(&mut invalid, word, value);
        assert!(Event::decode(&invalid).is_none());
    }
}

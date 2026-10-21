//! The shared owned-ring corpus from the Sophia contract
//! (`protocol/golden/sophia-owned-ring-v1.tsv`, signed revision). Shape
//! decoding and capacity admission are separate checks, exactly as the
//! contract states; diagnostic categories are not error enums.
use sophia_shell_protocol::shell_files::{RingControl, RingEvent, RingRegistration};

/// The registration capacity every Offer row in the corpus is judged
/// against (1920 * 24 * 4).
const CORPUS_CAPACITY: u64 = 184_320;

fn hex_decode(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn owned_ring_golden_corpus() {
    let corpus = include_str!("sophia-owned-ring-v1.tsv");
    let mut rows = 0;
    for line in corpus.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        rows += 1;
        let fields: Vec<&str> = line.split('\t').collect();
        let (name, surface, accept, bytes) = (
            fields[0],
            fields[1],
            fields[2] == "1",
            hex_decode(fields[4]),
        );
        match surface {
            "control" => match RingControl::decode(&bytes) {
                Ok(RingControl::Offer(offer)) => {
                    // Capacity is an admission check on top of shape.
                    let bytes = offer.resource_bytes().expect("offer extent");
                    assert_eq!(accept, bytes <= CORPUS_CAPACITY, "{name}");
                }
                Ok(RingControl::Register(_)) => assert!(accept, "{name}"),
                Err(_) => assert!(!accept, "{name}"),
            },
            "source" => assert_eq!(
                RingRegistration::decode_source(&bytes).is_ok(),
                accept,
                "{name}"
            ),
            "event" => assert_eq!(RingEvent::decode(&bytes).is_ok(), accept, "{name}"),
            other => panic!("unknown corpus surface {other}"),
        }
    }
    assert_eq!(rows, 36);
}

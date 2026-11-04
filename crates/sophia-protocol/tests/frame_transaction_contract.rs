//! Contract/vector checks only. The pinned SDK does not implement SFT yet.
use kdl::{KdlDocument, KdlNode};
use sophia_shell_protocol::{ShellContentRecord, shell::encoding::content::*, shell_files::*};
use std::collections::{BTreeMap, BTreeSet};

const CONTRACT: &str = include_str!("../../../protocol/sophia-frame-transactions-v1.kdl");
const GOLDEN: &str = include_str!("../../../protocol/golden/sophia-frame-transactions-v1.tsv");

fn number(node: &KdlNode, property: &str) -> usize {
    node.get(property)
        .unwrap()
        .as_integer()
        .unwrap()
        .try_into()
        .unwrap()
}

fn name(node: &KdlNode) -> &str {
    node.get(0).unwrap().as_string().unwrap()
}

fn vectors() -> Vec<(String, u16, Vec<u8>)> {
    GOLDEN
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            assert!(fields[2].is_ascii() && fields[2].len().is_multiple_of(2));
            let bytes = fields[2]
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            (fields[0].to_owned(), fields[1].parse().unwrap(), bytes)
        })
        .collect()
}

fn word(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[test]
fn identifiers_do_not_reuse_existing_shell_or_draw_assignments() {
    let doc = KdlDocument::parse_v2(CONTRACT).unwrap();
    let root = &doc.nodes()[0];
    assert_eq!(number(root, "shell-revision"), 13);
    assert_eq!(number(root, "capability-bit"), 18);
    assert_eq!(number(root, "envelope-api-version"), 1);
    let expected = BTreeMap::from([
        ("FrameStreamOpen", ("candidate", 279)),
        ("FrameStreamClose", ("candidate", 280)),
        ("FrameCommit", ("candidate", 281)),
        ("FrameStreamOpened", ("event", 55)),
        ("FrameStreamClosed", ("event", 56)),
        ("FrameRefused", ("event", 57)),
        ("FrameOutcome", ("event", 58)),
    ]);
    let old =
        KdlDocument::parse_v2(include_str!("../../../protocol/sophia-shell-files-v1.kdl")).unwrap();
    let mut used = BTreeSet::from([54]); // DrawSettled remains allocated while integration is deferred.
    for node in old.nodes()[0].children().unwrap().nodes() {
        if node.get("kind").is_some() {
            assert!(used.insert(number(node, "kind")));
        }
    }
    let mut actual = BTreeMap::new();
    for node in root.children().unwrap().nodes() {
        if node.get("kind").is_some() {
            let kind = number(node, "kind");
            assert!(used.insert(kind), "collision for {}", name(node));
            assert!(
                actual
                    .insert(name(node), (node.name().value(), kind))
                    .is_none()
            );
        }
    }
    assert_eq!(actual, expected);
}

#[test]
fn every_fixed_field_covers_its_exact_declared_extent() {
    let doc = KdlDocument::parse_v2(CONTRACT).unwrap();
    let nodes = doc.nodes()[0].children().unwrap().nodes();
    for node in nodes
        .iter()
        .filter(|n| matches!(n.name().value(), "body" | "body-prefix" | "row"))
    {
        let mut end = 0;
        for field in node.children().unwrap().nodes() {
            assert_eq!(
                number(field, "offset"),
                end,
                "{}: {}",
                name(node),
                name(field)
            );
            end += match field.get("type").unwrap().as_string().unwrap() {
                "u16" => 2,
                "u32" => 4,
                "u64" => 8,
                "StreamKey" => 64,
                "bytes" | "CandidateOutcome" => number(field, "size"),
                other => panic!("unaccounted field type {other}"),
            };
        }
        assert_eq!(end, number(node, "size"), "{}", name(node));
        if node.name().value() == "body" {
            let record = nodes
                .iter()
                .find(|n| n.get("kind").is_some() && name(n) == name(node))
                .unwrap();
            assert_eq!(end, number(record, "body-bytes"));
            if record.name().value() == "event" {
                assert!(
                    32 + end <= 256,
                    "existing control credit must fit {}",
                    name(node)
                );
            }
        }
    }
}

#[test]
fn golden_envelopes_and_fixed_records_match_the_contract() {
    let doc = KdlDocument::parse_v2(CONTRACT).unwrap();
    let nodes = doc.nodes()[0].children().unwrap().nodes();
    let mut names = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for (name, kind, bytes) in vectors() {
        assert!(names.insert(name));
        kinds.insert(kind);
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize,
            bytes.len()
        );
        assert_eq!(&bytes[4..6], &1u16.to_le_bytes());
        assert_eq!(&bytes[6..8], &kind.to_le_bytes());
        assert_eq!(word(&bytes, 8), 1);
        assert_eq!(word(&bytes, 16) != 0, kind >= 256);
        assert_eq!(word(&bytes, 24) != 0, kind < 256);
        assert_eq!(word(&bytes, 32), 11);
        let record = nodes
            .iter()
            .find(|n| n.get("kind").is_some() && number(n, "kind") == usize::from(kind))
            .unwrap();
        if record.get("body-bytes").is_some() {
            assert_eq!(bytes.len(), 32 + number(record, "body-bytes"));
        } else {
            assert!(bytes.len() <= number(record, "max-record-bytes"));
        }
        // Neither an old default decoder nor an explicitly selected r10 may
        // accidentally acquire SFT vocabulary merely because the contract exists.
        let class = if kind < 256 {
            ShellFileClass::Event
        } else {
            ShellFileClass::Candidate
        };
        assert!(decode_shell_file_record(&bytes, class).is_err());
        assert!(decode_shell_file_record_at_revision(&bytes, class, 10).is_err());
    }
    assert_eq!(names.len(), 16);
    assert_eq!(kinds, BTreeSet::from([55, 56, 57, 58, 279, 280, 281]));
}

#[test]
fn fixed_controls_preserve_stream_and_request_identities() {
    for (_, kind, bytes) in vectors() {
        let body = &bytes[32..];
        match kind {
            55 | 56 | 279 | 280 => {
                assert_eq!(
                    (0..8).map(|i| word(body, 8 + i * 8)).collect::<Vec<_>>(),
                    vec![1, 2, 3, 4, 5, 6, 7, 8]
                );
                if kind == 55 {
                    assert_eq!(&body[72..76], &1u32.to_le_bytes());
                    assert_eq!(&body[76..80], &250u32.to_le_bytes());
                }
                if kind == 56 {
                    assert_eq!(&body[72..74], &11u16.to_le_bytes());
                    assert_eq!(&body[74..], &[0; 6]);
                }
            }
            57 => {
                assert_eq!(
                    (0..5).map(|i| word(body, 8 + i * 8)).collect::<Vec<_>>(),
                    vec![1, 2, 3, 4, 8]
                );
                let request = u16::from_le_bytes(body[56..58].try_into().unwrap());
                assert!(matches!(request, 279..=281));
                assert_eq!(word(body, 48), if request == 281 { 9 } else { 0 });
                assert_eq!(&body[58..60], &2u16.to_le_bytes());
                assert_eq!(&body[60..], &[0; 4]);
            }
            _ => {}
        }
    }
}

#[test]
fn commit_vectors_reuse_exact_candidate_and_owned_ring_bytes() {
    let mut flags_seen = BTreeSet::new();
    for (_, kind, bytes) in vectors().into_iter().filter(|v| v.1 == 281) {
        assert_eq!(kind, 281);
        let body = &bytes[32..];
        let scene_len = u32::from_le_bytes(body[8..12].try_into().unwrap()) as usize;
        let flags = u16::from_le_bytes(body[12..14].try_into().unwrap());
        flags_seen.insert(flags);
        assert_eq!(&body[14..16], &[0, 0]);
        let scene = decode_content_candidate(&body[16..16 + scene_len]).unwrap();
        assert_eq!(content_candidate_bytes(&scene).unwrap(), scene_len);
        assert_eq!(
            encode_content_candidate(&scene).unwrap(),
            body[16..16 + scene_len]
        );
        assert_eq!(scene.surfaces.len(), 1);
        assert_eq!(scene.pacing_permit, 8);
        assert_eq!(scene.grant.connection_epoch, word(&bytes, 8));
        let mut offset = 16 + scene_len;
        if flags & 1 != 0 {
            let RingControl::Offer(offer) =
                RingControl::decode(&body[offset..offset + 152]).unwrap()
            else {
                panic!("FrameCommit carries an offer, not registration");
            };
            assert_eq!(offer.encode(), body[offset..offset + 152]);
            assert_eq!(
                offer.identity.connection_epoch,
                scene.grant.connection_epoch
            );
            assert_eq!(
                offer.identity.content_grant_epoch,
                scene.grant.content_grant_epoch
            );
            assert_eq!(
                (offer.output, offer.output_generation),
                (scene.output.id, scene.output.generation)
            );
            assert_eq!(
                (offer.allocation, offer.allocation_generation),
                (
                    scene.surfaces[0].allocation.id,
                    scene.surfaces[0].allocation.generation
                )
            );
            assert_eq!(offer.topology_generation, scene.facts_generation);
            assert!(offer.slot < 2);
            assert!(
                scene
                    .placements
                    .iter()
                    .any(|p| (p.resource.id, p.resource.generation)
                        == (offer.resource, offer.resource_generation))
            );
            offset += 152;
        }
        if flags & 2 != 0 {
            assert_ne!(word(body, offset), word(body, 0));
            assert!((0..3).all(|i| word(body, offset + 8 * i) != 0));
            assert!(
                !scene
                    .placements
                    .iter()
                    .any(|p| (p.resource.id, p.resource.generation)
                        == (word(body, offset + 8), word(body, offset + 16)))
            );
            offset += 24;
        }
        assert_eq!(offset, body.len());
        assert_eq!(flags & !3, 0);
    }
    assert_eq!(flags_seen, BTreeSet::from([0, 1, 2, 3]));
}

#[test]
fn outcome_vectors_reuse_the_legacy_outcome_without_returning_prepared_credit() {
    let mut outcomes = BTreeSet::new();
    for (_, _, bytes) in vectors().into_iter().filter(|v| v.1 == 58) {
        let body = &bytes[32..];
        let record =
            decode_shell_content_value(ShellContentValueKind::CandidateOutcome, &body[8..76])
                .unwrap();
        assert_eq!(encode_shell_content_value(&record).unwrap(), body[8..76]);
        let ShellContentRecord::CandidateOutcome(outcome) = record else {
            unreachable!()
        };
        assert_eq!(outcome.grant.connection_epoch, word(&bytes, 8));
        assert!((0..4).all(|i| word(body, 76 + 8 * i) != 0));
        let returned = word(body, 108) != 0;
        assert!(outcome.kind != 1 || !returned);
        outcomes.insert((outcome.kind, returned));
    }
    assert_eq!(
        outcomes,
        BTreeSet::from([(1, false), (2, false), (2, true), (3, true), (4, true)])
    );
}

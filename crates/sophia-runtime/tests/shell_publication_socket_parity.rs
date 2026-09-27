//! `publish_indicators`/`publish_catalog` on the SOCKET wire must emit
//! exactly today's Session-built frames: the same protocol encoders, in the
//! same order, byte for byte. This is the transport-phase evidence the t252
//! B5 plan asks for before Session is switched to call these methods.
use sophia_protocol::*;
use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, Instant};

#[allow(dead_code)] // Shared connect/negotiate fixture; this file drives only part of it.
#[path = "support/native_launcher_socket.rs"]
mod socket;
use socket::*;

fn snapshot() -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch: GRANT.connection_epoch,
        generation: 9,
        active_output: Some(OutputId::from_raw(3)),
        statuses: vec![ShellOutputStatus {
            output: OutputId::from_raw(3),
            focus_bits: 1,
            layout: "single".to_owned(),
        }],
        indicators: vec![ShellIndicator {
            output: OutputId::from_raw(3),
            indicator: 1,
            action: 2,
            slot: 0,
            state_bits: 1,
            label: "clock".to_owned(),
        }],
    }
}

#[test]
fn publish_indicators_matches_the_ipc_encoder_on_the_socket_wire() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let transaction = tx(90);
    let value = snapshot();
    let expected = encode_shell_indicator_snapshot(transaction, &value).unwrap();
    peer.transport
        .publish_indicators(&r, transaction, &value)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    let actual: Vec<Vec<u8>> = (0..expected.len()).map(|_| peer.read()).collect();
    assert_eq!(actual, expected);
    peer.transport.disconnect(&mut r).unwrap();
}

fn plain_catalog() -> ShellPersistentCatalog {
    ShellPersistentCatalog {
        catalog: catalog(),
        identities: BTreeMap::new(),
    }
}

fn identified_catalog() -> ShellPersistentCatalog {
    let wire = catalog();
    let identities = wire
        .entries
        .iter()
        .map(|entry| (entry.slot, format!("registered:{}", entry.slot)))
        .collect();
    ShellPersistentCatalog {
        catalog: wire,
        identities,
    }
}

#[test]
fn publish_catalog_without_identities_matches_the_ipc_encoder() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let transaction = tx(91);
    let value = plain_catalog();
    let expected = encode_shell_application_catalog(transaction, &value.catalog).unwrap();
    peer.transport
        .publish_catalog(&r, transaction, &value)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    let actual: Vec<Vec<u8>> = (0..expected.len()).map(|_| peer.read()).collect();
    assert_eq!(actual, expected);
    peer.transport.disconnect(&mut r).unwrap();
}

#[test]
fn publish_catalog_with_r8_identities_inserts_one_identity_per_entry_before_end() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let transaction = tx(92);
    let value = identified_catalog();
    let mut expected = encode_shell_application_catalog(transaction, &value.catalog).unwrap();
    let end = expected.pop().unwrap();
    for entry in &value.catalog.entries {
        expected.push(
            encode_shell_catalog_action_frame(
                transaction,
                &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                    connection_epoch: value.catalog.connection_epoch,
                    catalog_generation: value.catalog.generation,
                    slot: entry.slot,
                    identity: value.identities[&entry.slot].clone(),
                }),
            )
            .unwrap(),
        );
    }
    expected.push(end);
    peer.transport
        .publish_catalog(&r, transaction, &value)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    let actual: Vec<Vec<u8>> = (0..expected.len()).map(|_| peer.read()).collect();
    assert_eq!(actual, expected);
    peer.transport.disconnect(&mut r).unwrap();
}

/// A catalog large enough that its encoded frames exceed the connection's
/// `max_output_queue_bytes` (262144, from the shared `limits()` fixture).
fn oversized_catalog(generation: u64) -> ShellApplicationCatalog {
    let label = "L".repeat(128);
    let keywords = "K".repeat(256);
    ShellApplicationCatalog {
        connection_epoch: GRANT.connection_epoch,
        generation,
        entries: (1..=1000u16)
            .map(|slot| ShellApplicationDescriptor {
                slot,
                available: true,
                label: label.clone(),
                keywords: keywords.clone(),
            })
            .collect(),
    }
}

/// Reads exactly one already-framed IPC message from a raw client handle,
/// independent of `Peer`'s own buffering.
fn read_one_frame(client: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
    let mut frame = vec![0u8; SOPHIA_IPC_HEADER_LEN];
    client.read_exact(&mut frame).unwrap();
    let n = u32::from_le_bytes(frame[16..20].try_into().unwrap()) as usize;
    frame.resize(SOPHIA_IPC_HEADER_LEN + n, 0);
    client
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}

// Regression for t252 fix B (socket wire): a publication larger than the
// output queue's bulk budget must still be delivered whole, in order and
// byte-for-byte, draining across as many `poll_io` turns as it takes; and a
// second publication started while the first is still draining must be
// refused outright, taking nothing.
#[test]
fn publish_catalog_larger_than_the_output_queue_drains_whole_in_order_and_saturates_a_second() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let transaction = tx(93);
    let catalog = oversized_catalog(20);
    let expected = encode_shell_application_catalog(transaction, &catalog).unwrap();
    let total_bytes: usize = expected.iter().map(Vec::len).sum();
    assert!(
        total_bytes > 262_144,
        "fixture must exceed the output queue budget, got {total_bytes}"
    );

    let value = ShellPersistentCatalog {
        catalog,
        identities: BTreeMap::new(),
    };
    peer.transport
        .publish_catalog(&r, transaction, &value)
        .unwrap();

    // A second publication while the first is still draining is refused
    // outright: not one of its frames may enter the queue, and the first
    // publication's own delivery must be unaffected.
    assert!(matches!(
        peer.transport.publish_catalog(&r, tx(94), &plain_catalog()),
        Err(sophia_runtime::ShellTransportError::ActivationQueueSaturated)
    ));

    let mut reader = peer.client.try_clone().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let expected_for_reader = expected.clone();
    let reader = std::thread::spawn(move || {
        (0..expected_for_reader.len())
            .map(|_| read_one_frame(&mut reader))
            .collect::<Vec<_>>()
    });

    let start = Instant::now();
    while !reader.is_finished() {
        peer.transport.poll_io(&mut r).unwrap();
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::yield_now();
    }
    let actual = reader.join().unwrap();
    assert_eq!(actual, expected);

    peer.transport.disconnect(&mut r).unwrap();
}

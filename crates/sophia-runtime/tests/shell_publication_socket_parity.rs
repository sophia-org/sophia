//! `publish_indicators`/`publish_catalog` on the SOCKET wire must emit
//! exactly today's Session-built frames: the same protocol encoders, in the
//! same order, byte for byte. This is the transport-phase evidence the t252
//! B5 plan asks for before Session is switched to call these methods.
use sophia_protocol::*;
use std::collections::BTreeMap;

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

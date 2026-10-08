#![cfg(test)]

use super::*;

#[test]
fn connector_ids_are_scoped_to_their_card() {
    let record = LiveSysfsConnectorRecord {
        connector_name: "card1-HDMI-A-2".into(),
        connector_id: 42,
        crtc_id: 0,
        mode: DrmKmsMode::new(1920, 1080, 60_000),
        scale: 1,
    };
    assert!(record.matches_card_connector(Path::new("/dev/dri/card1"), 42));
    assert!(!record.matches_card_connector(Path::new("/dev/dri/card0"), 42));
    assert!(!record.matches_card_connector(Path::new("/dev/dri/card1"), 43));
    assert!(!connector_belongs_to_card(
        Path::new("card10-DP-1"),
        Path::new("card1")
    ));
}

#[test]
#[cfg(feature = "seat-control")]
fn foreign_connectors_are_excluded_before_their_facts_are_read() {
    let root = std::env::temp_dir().join(format!("sophia-seat-connectors-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let own = root.join("card1-HDMI-A-2");
    let foreign = root.join("card0-DP-1");
    fs::create_dir(&own).unwrap();
    fs::create_dir(&foreign).unwrap();
    fs::write(own.join("status"), "connected\n").unwrap();
    fs::write(own.join("modes"), "1920x1080\n").unwrap();
    fs::write(own.join("connector_id"), "42\n").unwrap();
    // Reading this as a file fails. A foreign seat must not even reach it.
    fs::create_dir(foreign.join("status")).unwrap();
    let discovery = LiveDrmSysfsDiscovery::default();
    assert!(discovery.discover_connectors(&root).is_err());
    let records = discovery
        .discover_connectors_on_cards(&root, &["/dev/dri/card1".into()])
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].connector_name, "card1-HDMI-A-2");
    assert!(
        discovery
            .discover_connectors_on_cards(&root, &[])
            .unwrap()
            .is_empty()
    );
    // The identical read failure on the admitted connector must still refuse.
    fs::remove_file(own.join("status")).unwrap();
    fs::create_dir(own.join("status")).unwrap();
    assert!(
        discovery
            .discover_connectors_on_cards(&root, &["card1".into()])
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

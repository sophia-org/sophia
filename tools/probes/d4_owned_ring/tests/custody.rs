use sophia_owned_ring_probe::*;
use std::os::unix::fs::FileExt;
fn geometry() -> Geometry {
    Geometry {
        slots: SLOTS,
        pixel_bytes: PANEL,
    }
}
#[test]
fn owned_generations_outlive_reused_source_slots_for_both_adapters() {
    for (method, sealed) in [
        (Method::Mapped, true),
        (Method::Pread, true),
        (Method::Pread, false),
    ] {
        let mut producer = Producer::new(geometry(), sealed).unwrap();
        let source =
            Source::admit(producer.file().try_clone().unwrap(), geometry(), method).unwrap();
        let mut retained = Vec::new();
        for i in 0..8 {
            producer
                .publish(i % SLOTS, (i as u64 + 1) * 2, i as u8)
                .unwrap();
            retained.push(
                source
                    .capture(i % SLOTS, (i as u64 + 1) * 2, || {})
                    .unwrap()
                    .0,
            );
        }
        for (i, resource) in retained.iter().enumerate() {
            let mut expected = vec![0; PANEL];
            fill(&mut expected, i as u8);
            assert_eq!(resource.bytes(), expected);
        }
    }
}
#[test]
fn torn_generation_rejected_once_and_destination_validated_after_copy() {
    for method in [Method::Mapped, Method::Pread] {
        let mut producer = Producer::new(geometry(), true).unwrap();
        let source =
            Source::admit(producer.file().try_clone().unwrap(), geometry(), method).unwrap();
        producer.publish(0, 2, 7).unwrap();
        assert!(
            source
                .capture(0, 2, || producer.publish(0, 4, 8).unwrap())
                .is_err()
        );
        assert!(source.capture(0, 2, || {}).is_err());
        assert!(source.capture(SLOTS, 4, || {}).is_err());
        assert!(source.capture(0, 3, || {}).is_err());
        // Later source corruption cannot corrupt an accepted copy.
        let accepted = source.capture(0, 4, || {}).unwrap().0;
        producer
            .file()
            .write_all_at(&[255, 255, 255, 0], HEADER as u64)
            .unwrap();
        assert!(source.capture(0, 4, || {}).is_err());
        assert!(valid_pixels(accepted.bytes()));
    }
}
#[test]
fn unsealed_truncation_and_missing_size_seals_fail_without_consumer_mapping() {
    let mut producer = Producer::new(geometry(), false).unwrap();
    assert!(
        Source::admit(
            producer.file().try_clone().unwrap(),
            geometry(),
            Method::Mapped
        )
        .is_err()
    );
    producer.publish(0, 2, 7).unwrap();
    let source = Source::admit(
        producer.file().try_clone().unwrap(),
        geometry(),
        Method::Pread,
    )
    .unwrap();
    // Preserve header so the pixel read itself is short, not only the header.
    producer
        .file()
        .set_len((HEADER + PANEL / 2) as u64)
        .unwrap();
    assert_eq!(
        source.capture(0, 2, || {}).err().unwrap().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    producer.file().set_len(0).unwrap();
    assert_eq!(
        source.capture(0, 2, || {}).err().unwrap().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}
#[test]
fn admitted_extent_and_geometry_are_not_source_controlled() {
    let producer = Producer::new(geometry(), true).unwrap();
    assert!(producer.file().set_len(1).is_err());
    assert!(
        Source::admit(
            producer.file().try_clone().unwrap(),
            Geometry {
                slots: 0,
                pixel_bytes: PANEL
            },
            Method::Pread
        )
        .is_err()
    );
    assert!(
        Source::admit(
            producer.file().try_clone().unwrap(),
            Geometry {
                slots: SLOTS,
                pixel_bytes: BULK + 4
            },
            Method::Pread
        )
        .is_err()
    );
    assert!(
        Source::admit(
            producer.file().try_clone().unwrap(),
            Geometry {
                slots: SLOTS,
                pixel_bytes: PANEL + 4
            },
            Method::Pread
        )
        .is_err()
    );
}

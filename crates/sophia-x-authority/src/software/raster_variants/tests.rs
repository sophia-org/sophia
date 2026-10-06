#![cfg(test)]

use super::*;
use sophia_protocol::SurfaceId;

const SIZE: Size = Size {
    width: 4,
    height: 2,
};
const WHOLE: Rect = Rect {
    x: 0,
    y: 0,
    width: 4,
    height: 2,
};

fn image(rect: Rect, pixels: &[u8]) -> XAuthorityRasterCommand {
    XAuthorityRasterCommand::from_put_image(
        &XPutImageSemantics {
            format: X_IMAGE_FORMAT_Z_PIXMAP,
            depth: 24,
            left_pad: 0,
            byte_order: XByteOrder::LittleEndian,
            gc: XGraphicsContextValues::default(),
        },
        rect,
        pixels,
    )
}

fn pixels(command: &XAuthorityRasterCommand) -> &[u8] {
    let XAuthorityRasterCommand::PutImage { image, .. } = command else {
        panic!("expected retained image");
    };
    &image.pixels
}

fn doubled(store: &mut XAuthorityRasterStore, presentation: XResourceId) -> SurfaceRasterClass {
    let class = SurfaceRasterClass {
        density_millis: 2_000,
        transform: SurfaceRasterTransform::Normal,
    };
    let result = store
        .satisfy(
            presentation,
            &SurfaceRasterRequirements {
                surface: SurfaceId::new(1, 1),
                committed_content_generation: 1,
                requirement_generation: 1,
                logical_extent: SIZE,
                classes: vec![class],
            },
            32,
        )
        .unwrap();
    assert!(matches!(result, XRasterSatisfyOutcome::Satisfied(_)));
    class
}

fn assert_doubled(snapshot: &XAuthorityCpuBufferSnapshot, expected: &[u8]) {
    assert_eq!(
        snapshot.size,
        Size {
            width: 8,
            height: 4
        }
    );
    for y in 0..4 {
        for x in 0..8 {
            let source = ((y / 2) * 4 + x / 2) * 4;
            let destination = (y * 8 + x) * 4;
            assert_eq!(
                &snapshot.bytes[destination..destination + 3],
                &expected[source..source + 3]
            );
        }
    }
}

#[test]
fn journal_keeps_the_owned_image_allocation_after_replaying_variants() {
    let presentation = XResourceId::new(1, 1);
    let mut store = XAuthorityRasterStore::default();
    let baseline = [0x11, 0x22, 0x33, 0].repeat(8);
    store.record(presentation, SIZE, image(WHOLE, &baseline));
    let class = doubled(&mut store, presentation);
    let before = store.surfaces[&presentation].variants[&class]
        .snapshot
        .clone();

    let patch_rect = Rect {
        x: 1,
        y: 0,
        width: 2,
        height: 1,
    };
    let mut source = vec![0x51, 0x62, 0x73, 0, 0x84, 0x95, 0xa6, 0];
    let command = image(patch_rect, &source);
    let allocation = pixels(&command).as_ptr();
    let payload = command.payload_bytes();
    let updates = store.record(presentation, SIZE, command);
    let state = &store.surfaces[&presentation];
    assert_eq!(
        pixels(state.journal.last().unwrap()).as_ptr(),
        allocation,
        "journal must retain the consumed image allocation after replay"
    );
    assert_eq!(
        state.journal_payload_bytes,
        image(WHOLE, &baseline).payload_bytes() + payload
    );
    assert_eq!(state.journal.len(), 2);
    assert!(state.replayable);
    assert_eq!(updates.len(), 1);
    assert!(matches!(
        &updates[0],
        XAuthorityCpuBufferUpdate::PatchBatch(_)
    ));
    let mut expected = baseline.clone();
    expected[4..12].copy_from_slice(&source);
    source.fill(0xff);
    assert_eq!(pixels(&state.journal[1]), &expected[4..12]);
    assert_doubled(&before, &baseline);
    assert_doubled(&state.variants[&class].snapshot, &expected);
    assert_eq!(
        state.variants[&class].snapshot.generation,
        before.generation + 1
    );

    // A full upload still replaces the journal and updates existing variants.
    let replacement = [0x77, 0x88, 0x99, 0].repeat(8);
    let command = image(WHOLE, &replacement);
    let allocation = pixels(&command).as_ptr();
    let payload = command.payload_bytes();
    store.record(presentation, SIZE, command);
    let state = &store.surfaces[&presentation];
    assert_eq!(state.journal.len(), 1);
    assert_eq!(pixels(&state.journal[0]).as_ptr(), allocation);
    assert_eq!(state.journal_payload_bytes, payload);
    assert_doubled(&state.variants[&class].snapshot, &replacement);
    assert_doubled(&before, &baseline);
}

#[test]
fn journal_keeps_owned_image_allocations_while_rebuilding_coverage() {
    let presentation = XResourceId::new(2, 1);
    let mut store = XAuthorityRasterStore::default();
    store.invalidate_unjournaled_presentation(presentation, SIZE);
    let left = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    let right = Rect { x: 2, ..left };
    let left_pixels = [0x12, 0x34, 0x56, 0].repeat(4);
    let right_pixels = [0x78, 0x9a, 0xbc, 0].repeat(4);
    let mut payloads = 0;
    for (rect, source) in [(left, &left_pixels), (right, &right_pixels)] {
        let command = image(rect, source);
        let allocation = pixels(&command).as_ptr();
        payloads += command.payload_bytes();
        assert!(store.record(presentation, SIZE, command).is_empty());
        let state = &store.surfaces[&presentation];
        assert_eq!(
            pixels(state.journal.last().unwrap()).as_ptr(),
            allocation,
            "coverage accumulation must retain the consumed image allocation"
        );
        assert_eq!(state.journal_payload_bytes, payloads);
        assert_eq!(state.replayable, rect == right);
        if rect == left {
            assert!(!state.coverage.is_empty());
            assert_eq!(state.poison, Some(XRasterFallbackCause::UnsupportedCommand));
        } else {
            assert!(state.coverage.is_empty());
            assert_eq!(state.poison, None);
        }
    }
    assert_eq!(store.surfaces[&presentation].journal.len(), 2);
    let class = doubled(&mut store, presentation);
    let mut expected = Vec::new();
    for _ in 0..2 {
        expected.extend_from_slice(&left_pixels[..8]);
        expected.extend_from_slice(&right_pixels[..8]);
    }
    assert_doubled(
        &store.surfaces[&presentation].variants[&class].snapshot,
        &expected,
    );
}

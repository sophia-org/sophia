use sophia_draw::*;
use sophia_protocol::{ContentGrant, ContentResourceId};
fn panel() -> Batch {
    Batch {
        transaction: 1,
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width: 240,
        height: 24,
        commands: vec![
            Command::Rect {
                x: 0,
                y: 0,
                width: 240,
                height: 24,
                rgb: [20, 30, 40],
            },
            Command::Text {
                x: 4,
                y: 2,
                size: 16,
                rgb: [240, 240, 240],
                text: "Sophia 42".into(),
            },
        ],
    }
}
#[test]
fn bounded_encoding_rejects_truncation_trailing_data_and_oversized_work() {
    let batch = panel();
    let bytes = batch.encode().unwrap();
    assert_eq!(Batch::decode(&bytes).unwrap(), batch);
    for end in 0..bytes.len() {
        assert!(Batch::decode(&bytes[..end]).is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(Batch::decode(&trailing).is_err());
    for width in [0, 2049, u16::MAX] {
        let mut b = batch.clone();
        b.width = width;
        assert!(b.encode().is_err());
    }
    let mut b = batch.clone();
    b.commands = vec![batch.commands[0].clone(); MAX_COMMANDS + 1];
    assert!(b.encode().is_err());
    let mut b = batch;
    b.commands.push(Command::Text {
        x: 0,
        y: 0,
        size: 16,
        rgb: [0; 3],
        text: "\n".into(),
    });
    assert!(b.encode().is_err());
}
#[test]
fn vello_output_is_deterministic_premultiplied_and_value_changes_only_its_resource() {
    let mut raster = Rasterizer::default();
    let mut batch = panel();
    let a = raster.render(&batch, || false).unwrap();
    assert_eq!(a.len(), batch.pixel_bytes());
    assert_eq!(a, raster.render(&batch, || false).unwrap());
    assert_eq!(&a[..4], &[40, 30, 20, 255]);
    assert!(a.chunks_exact(4).any(|p| p[0] > 40));
    assert!(
        a.chunks_exact(4)
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
    );
    if let Command::Text { text, .. } = &mut batch.commands[1] {
        *text = "Sophia 43".into();
    }
    assert_ne!(a, raster.render(&batch, || false).unwrap());
    assert_eq!(raster.render(&batch, || true), Err(Error::Cancelled));
}

#[test]
fn generated_resources_obey_negotiated_row_chunking_and_pixel_limits() {
    let mut b = panel();
    b.width = 1920;
    let mut limits = sophia_protocol::ContentLimits::prototype(b.grant);
    let description = b.description(&limits).unwrap();
    assert_eq!(description.chunk_count, 3);
    assert_eq!(description.layout(&limits).unwrap().total_bytes, 184320);
    limits.max_resource_bytes = 100;
    assert!(b.description(&limits).is_err());
    limits.max_chunk_bytes = 4;
    assert!(b.description(&limits).is_err());
    assert!(Batch::decode(&vec![0; MAX_BATCH_BYTES + 1]).is_err());
}

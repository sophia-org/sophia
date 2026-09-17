use sophia_engine::*;
use sophia_protocol::*;

fn image(epoch: u64, generation: u64) -> CompositorContentIdentity {
    let grant = ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    };
    let geometry = Rect {
        x: 0,
        y: (epoch as i32 - 1) * 8,
        width: 8,
        height: 8,
    };
    CompositorContentIdentity {
        node: CompositorNodeId::ShellContent {
            grant,
            output: OutputId::from_raw(1),
            candidate: 1,
            surface: 0,
            placement: 0,
        },
        generation,
        output_size_px: Size {
            width: 64,
            height: 64,
        },
        geometry_px: geometry,
        size_px: Size {
            width: 8,
            height: 8,
        },
        stride: 32,
        format: u32::from_le_bytes(*b"AR24"),
        resource: ContentResourceBegin {
            grant,
            resource: ContentResourceId { id: 1, generation },
            width_px: 8,
            height_px: 8,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 256,
        },
        source_bytes: 256,
    }
}

#[test]
fn equal_client_local_ids_cannot_hide_first_components_damage() {
    let list = |generation| CompositorDisplayList {
        output: OutputId::from_raw(1),
        commands: vec![
            CompositorDisplayCommand::ContentImage(image(1, generation)),
            CompositorDisplayCommand::ContentImage(image(2, 1)),
        ],
    };
    let previous = list(1);
    assert!(compositor_display_list_damage(&previous, &previous).is_empty());
    assert_ne!(image(1, 1).node, image(2, 1).node);
    let current = list(2);
    let damage = compositor_display_list_damage(&previous, &current);
    assert!(
        !damage.is_empty(),
        "the unchanged second component must not mask the changed first"
    );
    assert!(
        damage
            .rects
            .iter()
            .all(|rect| *rect == image(1, 1).geometry_px)
    );
}

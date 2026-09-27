#![cfg(feature = "native-session")]

use sophia_config::ShellComponentEdge;
use sophia_protocol::ContentPixelRect;
use sophia_session::{
    SHELL_GPU_PROOF_DEFAULT_TIMEOUT, SHELL_GPU_PROOF_MAX_RENDERS, SHELL_GPU_PROOF_MAX_TIMEOUT,
    SHELL_GPU_PROOF_MIN_TIMEOUT, ShellGpuContentProof, ShellGpuProofEnd, ShellGpuProofError,
    ShellGpuProofExtent, ShellGpuProofOutcome, ShellGpuProofPixels, ShellGpuProofSurface,
    shell_gpu_proof_content_limits,
};
use std::time::Duration;

const EDGES: [ShellComponentEdge; 4] = [
    ShellComponentEdge::Top,
    ShellComponentEdge::Bottom,
    ShellComponentEdge::Left,
    ShellComponentEdge::Right,
];

fn proof(edge: ShellComponentEdge, width: u32, height: u32) -> ShellGpuContentProof {
    ShellGpuContentProof {
        client: "/opt/shell/client".into(),
        client_args: vec!["--any".into(), "value with spaces".into()],
        config: Some("/etc/shell/config".into()),
        seat: "seat0".into(),
        render_node: "/dev/dri/renderD128".into(),
        expected_device: Some("226:128@0000:03:00.0".into()),
        output: ShellGpuProofExtent {
            width: 800,
            height: 600,
        },
        surface: ShellGpuProofSurface {
            edge,
            width,
            height,
        },
        outcomes: vec![
            ShellGpuProofOutcome::PresentedSynthetic,
            ShellGpuProofOutcome::RendererFailed,
            ShellGpuProofOutcome::PresentedSynthetic,
        ],
        end: ShellGpuProofEnd::StopClient,
        pixels: ShellGpuProofPixels::Contract,
        discrete_input: false,
        timeout: SHELL_GPU_PROOF_DEFAULT_TIMEOUT,
    }
}

fn on_output(
    edge: ShellComponentEdge,
    output: (u32, u32),
    surface: (u32, u32),
) -> ShellGpuContentProof {
    let mut parameters = proof(edge, surface.0, surface.1);
    parameters.output = ShellGpuProofExtent {
        width: output.0,
        height: output.1,
    };
    parameters
}

#[test]
fn surfaces_fit_and_sit_against_each_edge() {
    let output = ShellGpuProofExtent {
        width: 800,
        height: 600,
    };
    for (edge, width, height, x, y, thickness) in [
        (ShellComponentEdge::Top, 800, 24, 0, 0, 24),
        (ShellComponentEdge::Bottom, 800, 64, 0, 536, 64),
        (ShellComponentEdge::Left, 48, 600, 0, 0, 48),
        (ShellComponentEdge::Right, 32, 600, 768, 0, 32),
    ] {
        let parameters = proof(edge, width, height);
        parameters.validate().unwrap();
        assert_eq!(parameters.surface.thickness(), thickness, "{edge:?}");
        assert_eq!(
            parameters.surface.placement(output),
            ContentPixelRect {
                x,
                y,
                width,
                height
            },
            "{edge:?}"
        );
    }
}

#[test]
fn a_surface_larger_than_the_output_does_not_fit_any_edge() {
    for edge in EDGES {
        for (width, height) in [(801, 24), (24, 601), (801, 601)] {
            assert_eq!(
                proof(edge, width, height).validate(),
                Err(ShellGpuProofError::SurfaceOutsideOutput),
                "{edge:?} {width}x{height}"
            );
        }
    }
}

#[test]
fn zero_dimensions_are_refused() {
    for (field, name) in [
        (0, "output width"),
        (1, "output height"),
        (2, "surface width"),
        (3, "surface height"),
    ] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        match field {
            0 => parameters.output.width = 0,
            1 => parameters.output.height = 0,
            2 => parameters.surface.width = 0,
            _ => parameters.surface.height = 0,
        }
        assert_eq!(
            parameters.validate(),
            Err(ShellGpuProofError::ZeroExtent { name })
        );
    }
}

#[test]
fn resource_width_and_height_limits_hold_at_and_one_past() {
    let limits = shell_gpu_proof_content_limits();
    let width = limits.max_width_px;
    on_output(ShellComponentEdge::Top, (width + 1, 4096), (width, 1))
        .validate()
        .unwrap();
    assert_eq!(
        on_output(ShellComponentEdge::Top, (width + 1, 4096), (width + 1, 1)).validate(),
        Err(ShellGpuProofError::ResourceExtent {
            name: "surface width",
            value: width + 1,
            limit: width
        })
    );
    let height = limits.max_height_px;
    on_output(ShellComponentEdge::Left, (16, height + 1), (1, height))
        .validate()
        .unwrap();
    assert_eq!(
        on_output(ShellComponentEdge::Left, (16, height + 1), (1, height + 1)).validate(),
        Err(ShellGpuProofError::ResourceExtent {
            name: "surface height",
            value: height + 1,
            limit: height
        })
    );
    assert!(matches!(
        on_output(ShellComponentEdge::Top, (u32::MAX, u32::MAX), (u32::MAX, 1)).validate(),
        Err(ShellGpuProofError::ResourceExtent { .. })
    ));
}

#[test]
fn thickness_across_each_edge_holds_at_and_one_past_the_limit() {
    let limits = shell_gpu_proof_content_limits();
    let limit = limits.max_panel_extent.min(limits.max_reservation_extent);
    for edge in EDGES {
        let across = |thickness| match edge {
            ShellComponentEdge::Top | ShellComponentEdge::Bottom => (16, thickness),
            ShellComponentEdge::Left | ShellComponentEdge::Right => (thickness, 16),
        };
        on_output(edge, (1024, 1024), across(limit))
            .validate()
            .unwrap();
        assert_eq!(
            on_output(edge, (1024, 1024), across(limit + 1)).validate(),
            Err(ShellGpuProofError::Thickness {
                thickness: limit + 1,
                limit
            }),
            "{edge:?}"
        );
    }
}

#[test]
fn resource_bytes_hold_at_and_one_row_past_the_limit() {
    let limits = shell_gpu_proof_content_limits();
    let width = limits.max_width_px;
    let pixels = limits.max_resource_bytes / 4;
    assert_eq!(pixels % u64::from(width), 0, "limit is not whole rows");
    let height = u32::try_from(pixels / u64::from(width)).unwrap();
    let thickness = limits.max_panel_extent.min(limits.max_reservation_extent);
    assert!(
        height < thickness,
        "rows past the byte limit must still fit"
    );
    let output = (width, limits.max_height_px);
    let at = on_output(ShellComponentEdge::Top, output, (width, height));
    assert_eq!(at.surface.bytes(), Some(limits.max_resource_bytes));
    at.validate().unwrap();
    let past = on_output(ShellComponentEdge::Top, output, (width, height + 1));
    assert_eq!(
        past.validate(),
        Err(ShellGpuProofError::ResourceBytes {
            bytes: past.surface.bytes(),
            limit: limits.max_resource_bytes
        })
    );
    let overflowing = ShellGpuProofSurface {
        edge: ShellComponentEdge::Top,
        width: u32::MAX,
        height: 2,
    };
    assert_eq!(overflowing.bytes(), None);
}

#[test]
fn coverage_holds_at_and_one_row_past_the_limit() {
    let percent = shell_gpu_proof_content_limits().max_content_coverage_percent;
    // On a 100x100 output one full-width row is exactly one percent.
    on_output(ShellComponentEdge::Top, (100, 100), (100, percent))
        .validate()
        .unwrap();
    assert_eq!(
        on_output(ShellComponentEdge::Top, (100, 100), (100, percent + 1)).validate(),
        Err(ShellGpuProofError::Coverage {
            percent_limit: percent
        })
    );
}

#[test]
fn render_count_is_bounded() {
    for count in [0, SHELL_GPU_PROOF_MAX_RENDERS + 1] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.outcomes = vec![ShellGpuProofOutcome::RendererFailed; count];
        assert_eq!(
            parameters.validate(),
            Err(ShellGpuProofError::RenderCount { count })
        );
    }
    for count in [1, SHELL_GPU_PROOF_MAX_RENDERS] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.outcomes = vec![ShellGpuProofOutcome::PresentedSynthetic; count];
        parameters.validate().unwrap();
    }
}

#[test]
fn timeout_is_bounded() {
    for timeout in [
        Duration::ZERO,
        SHELL_GPU_PROOF_MIN_TIMEOUT - Duration::from_millis(1),
        SHELL_GPU_PROOF_MAX_TIMEOUT + Duration::from_millis(1),
    ] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.timeout = timeout;
        assert_eq!(
            parameters.validate(),
            Err(ShellGpuProofError::Timeout { timeout })
        );
    }
    for timeout in [SHELL_GPU_PROOF_MIN_TIMEOUT, SHELL_GPU_PROOF_MAX_TIMEOUT] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.timeout = timeout;
        parameters.validate().unwrap();
    }
}

#[test]
fn paths_seat_and_device_pin_are_checked() {
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.client = "client".into();
    assert_eq!(
        parameters.validate(),
        Err(ShellGpuProofError::RelativePath { name: "client" })
    );
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.config = Some("config.kdl".into());
    assert_eq!(
        parameters.validate(),
        Err(ShellGpuProofError::RelativePath { name: "config" })
    );
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.config = None;
    parameters.validate().unwrap();
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.render_node = "dev/dri/renderD128".into();
    assert_eq!(
        parameters.validate(),
        Err(ShellGpuProofError::RelativePath {
            name: "render node"
        })
    );
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.seat.clear();
    assert_eq!(parameters.validate(), Err(ShellGpuProofError::EmptySeat));
    for pin in ["226:128@none", "1:3@0000:01:00.0"] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.expected_device = Some(pin.into());
        parameters.validate().unwrap();
    }
    for pin in [
        "",
        "226:128",
        "226@none",
        "226:0128@none",
        "226:128@",
        "a:1@none",
        "1:2@x y",
    ] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.expected_device = Some(pin.into());
        assert_eq!(
            parameters.validate(),
            Err(ShellGpuProofError::MalformedExpectedDevice),
            "{pin:?}"
        );
    }
}

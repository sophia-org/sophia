#![cfg(feature = "native-session")]

use sophia_config::ShellComponentEdge;
use sophia_protocol::ContentPixelRect;
use sophia_session::{
    SHELL_GPU_PROOF_DEFAULT_TIMEOUT, SHELL_GPU_PROOF_MAX_EXTENT, SHELL_GPU_PROOF_MAX_RENDERS,
    SHELL_GPU_PROOF_MAX_TIMEOUT, SHELL_GPU_PROOF_MIN_TIMEOUT, ShellGpuContentProof,
    ShellGpuProofEnd, ShellGpuProofError, ShellGpuProofExtent, ShellGpuProofOutcome,
    ShellGpuProofSurface,
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
        discrete_input: false,
        timeout: SHELL_GPU_PROOF_DEFAULT_TIMEOUT,
    }
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
        for (width, height) in [(801, 24), (48, 601), (801, 601)] {
            assert_eq!(
                proof(edge, width, height).validate(),
                Err(ShellGpuProofError::SurfaceOutsideOutput),
                "{edge:?} {width}x{height}"
            );
        }
        // Exactly the output is the largest surface that fits.
        proof(edge, 800, 600).validate().unwrap();
    }
}

#[test]
fn zero_oversized_and_overflowing_dimensions_are_refused() {
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.surface.height = 0;
    assert_eq!(
        parameters.validate(),
        Err(ShellGpuProofError::ZeroExtent {
            name: "surface height"
        })
    );
    let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
    parameters.output.width = 0;
    assert_eq!(
        parameters.validate(),
        Err(ShellGpuProofError::ZeroExtent {
            name: "output width"
        })
    );
    for value in [SHELL_GPU_PROOF_MAX_EXTENT + 1, u32::MAX] {
        let mut parameters = proof(ShellComponentEdge::Top, 800, 24);
        parameters.output.height = value;
        assert_eq!(
            parameters.validate(),
            Err(ShellGpuProofError::OversizedExtent {
                name: "output height",
                value
            })
        );
        let mut parameters = proof(ShellComponentEdge::Left, 48, 600);
        parameters.output = ShellGpuProofExtent {
            width: value,
            height: value,
        };
        parameters.surface.width = value;
        assert!(matches!(
            parameters.validate(),
            Err(ShellGpuProofError::OversizedExtent { .. })
        ));
    }
    let mut largest = proof(ShellComponentEdge::Top, SHELL_GPU_PROOF_MAX_EXTENT, 24);
    largest.output.width = SHELL_GPU_PROOF_MAX_EXTENT;
    largest.validate().unwrap();
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

//! Numeric evidence at the boundary between content and chrome geometry.
//! Keep the operation and its inputs: the Engine error alone cannot distinguish
//! an authority constraint from a WM allocation. No client metadata belongs here.
use sophia_engine::ChromeLayoutError;
use sophia_protocol::{OutputId, Rect, Size, SurfaceConstraints, SurfaceId};

pub(crate) const CODES: &[&str] = &[
    "chrome_coordinate_overflow",
    "chrome_allocation_too_small",
    "chrome_invalid_clearance",
];

#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    not(feature = "native-session"),
    expect(
        dead_code,
        reason = "Only native Session constructs conversion contexts; all builds retain their records."
    )
)]
pub(crate) enum Operation {
    SnapshotConstraints,
    SnapshotGeometry,
    AllocationBounds,
    ProposalContent,
    ReconciledOuter,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::SnapshotConstraints => "snapshot_constraints",
            Self::SnapshotGeometry => "snapshot_geometry",
            Self::AllocationBounds => "allocation_bounds",
            Self::ProposalContent => "proposal_content",
            Self::ReconciledOuter => "reconciled_outer",
        }
    }

    fn origin(self) -> &'static str {
        match self {
            Self::SnapshotConstraints | Self::SnapshotGeometry => "authority_facts",
            Self::AllocationBounds => "output_bounds",
            Self::ProposalContent => "wm_proposal",
            Self::ReconciledOuter => "reconciled_content",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Context {
    pub operation: Operation,
    pub surface: SurfaceId,
    pub output: Option<OutputId>,
    pub clearance: i32,
    pub geometry: Rect,
    pub constraints: Option<SurfaceConstraints>,
    pub requested_size: Option<Size>,
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub context: Context,
    pub cause: ChromeLayoutError,
}

impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c = self.context;
        write!(
            f,
            "failure_code={} operation={} origin={} surface={} surface_generation={} output={} clearance={} x={} y={} width={} height={}",
            code(self.cause),
            c.operation.name(),
            c.operation.origin(),
            c.surface.index(),
            c.surface.generation(),
            c.output.map_or(0, OutputId::raw),
            c.clearance,
            c.geometry.x,
            c.geometry.y,
            c.geometry.width,
            c.geometry.height,
        )?;
        for (label, size) in [
            ("min", c.constraints.and_then(|v| v.min_size)),
            ("max", c.constraints.and_then(|v| v.max_size)),
            ("requested", c.requested_size),
        ] {
            match size {
                Some(size) => write!(
                    f,
                    " {label}_width={} {label}_height={}",
                    size.width, size.height
                )?,
                None => write!(f, " {label}_width=none {label}_height=none")?,
            }
        }
        Ok(())
    }
}

#[cfg(feature = "native-session")]
pub(crate) fn report_failure(context: Context, cause: ChromeLayoutError) -> Failure {
    let failure = Failure { context, cause };
    crate::session_eprintln!("sophia_wm_geometry_failure schema=1 status=failed {failure}");
    failure
}

fn code(error: ChromeLayoutError) -> &'static str {
    match error {
        ChromeLayoutError::CoordinateOverflow => CODES[0],
        ChromeLayoutError::AllocationTooSmall => CODES[1],
        ChromeLayoutError::InvalidClearance => CODES[2],
    }
}

pub(crate) fn failure_code(error: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    error
        .downcast_ref::<Failure>()
        .map(|e| code(e.cause))
        .or_else(|| error.downcast_ref::<ChromeLayoutError>().map(|e| code(*e)))
}

pub(super) fn field(key: &str, value: &str) -> bool {
    match key {
        "schema" => value == "1",
        "status" => value == "failed",
        "failure_code" => CODES.contains(&value),
        "operation" => matches!(
            value,
            "snapshot_constraints"
                | "snapshot_geometry"
                | "allocation_bounds"
                | "proposal_content"
                | "reconciled_outer"
        ),
        "origin" => matches!(
            value,
            "authority_facts" | "output_bounds" | "wm_proposal" | "reconciled_content"
        ),
        "surface" | "surface_generation" | "output" => value.parse::<u64>().is_ok(),
        "clearance" | "x" | "y" | "width" | "height" => value.parse::<i32>().is_ok(),
        "min_width" | "min_height" | "max_width" | "max_height" | "requested_width"
        | "requested_height" => value == "none" || value.parse::<i32>().is_ok(),
        _ => false,
    }
}

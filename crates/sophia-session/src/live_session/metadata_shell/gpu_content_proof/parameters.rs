//! Typed parameters of the shell GPU content proof.
//!
//! Every expectation the proof checks comes from these parameters rather than
//! from a particular client. [`ShellGpuContentProof::validate`] is pure: it
//! runs before the proof reads the render inventory or touches a device, and
//! refuses any geometry the proof's negotiated content limits would refuse
//! later.

use sophia_config::ShellComponentEdge;
use sophia_protocol::{ContentGrant, ContentLimits, ContentPixelRect};
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// Upper bound on verified renders in one proof.
pub const SHELL_GPU_PROOF_MAX_RENDERS: usize = 16;
pub const SHELL_GPU_PROOF_MIN_TIMEOUT: Duration = Duration::from_secs(1);
pub const SHELL_GPU_PROOF_MAX_TIMEOUT: Duration = Duration::from_secs(120);
pub const SHELL_GPU_PROOF_DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// Bytes per pixel of the one content pixel format the limits admit.
pub const SHELL_GPU_PROOF_BYTES_PER_PIXEL: u32 = 4;

/// The content limits the proof's transport negotiates. The proof reserves no
/// session limits, so negotiation grants the prototype profile; `run` refuses
/// if the negotiated profile ever differs from this one.
pub fn shell_gpu_proof_content_limits() -> ContentLimits {
    ContentLimits::prototype(ContentGrant::default())
}

/// The synthetic output the proof publishes to the client, in pixels at scale 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellGpuProofExtent {
    pub width: u32,
    pub height: u32,
}

/// The one edge-anchored surface the client must request and render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellGpuProofSurface {
    pub edge: ShellComponentEdge,
    pub width: u32,
    pub height: u32,
}

impl ShellGpuProofSurface {
    /// The extent across the anchored edge: the reservation the proof grants.
    pub const fn thickness(self) -> u32 {
        match self.edge {
            ShellComponentEdge::Top | ShellComponentEdge::Bottom => self.height,
            ShellComponentEdge::Left | ShellComponentEdge::Right => self.width,
        }
    }

    /// Bytes of one full-surface resource in the admitted pixel format.
    pub fn bytes(self) -> Option<u64> {
        let row = self.width.checked_mul(SHELL_GPU_PROOF_BYTES_PER_PIXEL)?;
        u64::from(row).checked_mul(u64::from(self.height))
    }

    /// Where the surface sits on the output: against its edge, with no margin.
    /// This matches Session's placement of an edge-anchored allocation.
    pub fn placement(self, output: ShellGpuProofExtent) -> ContentPixelRect {
        let x = match self.edge {
            ShellComponentEdge::Right => output.width.saturating_sub(self.width),
            _ => 0,
        };
        let y = match self.edge {
            ShellComponentEdge::Bottom => output.height.saturating_sub(self.height),
            _ => 0,
        };
        ContentPixelRect {
            x: i32::try_from(x).unwrap_or(i32::MAX),
            y: i32::try_from(y).unwrap_or(i32::MAX),
            width: self.width,
            height: self.height,
        }
    }
}

/// The synthetic answer the proof gives one verified render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellGpuProofOutcome {
    /// Prepared, then Presented. No native output is taken over, so this is a
    /// synthetic presentation, never native presentation evidence.
    PresentedSynthetic,
    /// RendererFailed. When this answers the final render, the proof also
    /// checks that the renderer-owned lease survives the client's disconnect.
    RendererFailed,
}

impl ShellGpuProofOutcome {
    pub const fn record_name(self) -> &'static str {
        match self {
            Self::PresentedSynthetic => "presented_synthetic",
            Self::RendererFailed => "renderer_failed",
        }
    }
}

/// How the proof ends after the final verified render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellGpuProofEnd {
    /// The client must exit by itself; a further submission is refused.
    ClientExits,
    /// The proof terminates the supervised client.
    StopClient,
}

impl ShellGpuProofEnd {
    pub const fn record_name(self) -> &'static str {
        match self {
            Self::ClientExits => "client_exits",
            Self::StopClient => "stop_client",
        }
    }
}

/// What the proof requires of each candidate's pixels, beyond the content
/// contract the transport already enforces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellGpuProofPixels {
    /// No requirement beyond the contract: any number of placements and
    /// resources, including solid fills. The records carry byte counts and a
    /// checksum over the placed resources, and claim nothing about the pixels.
    Contract,
    /// Peer requirement for a client that rasters its whole surface into one
    /// resource: exactly one surface, whose first placement's resource has the
    /// allocation's exact size and bytes that are neither empty nor uniform.
    ///
    /// This proves only that the requested pixel pattern crossed the content
    /// path. It does not prove the pixels came from a GPU: a CPU peer can send
    /// the same bytes. GPU execution is supported by other evidence -- the
    /// protected device grant recorded here, together with the client's own
    /// adapter and render evidence, which its external verifier checks.
    FullSurfaceRaster,
}

impl ShellGpuProofPixels {
    pub const fn record_name(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::FullSurfaceRaster => "full_surface_raster",
        }
    }
}

/// One isolated run of a shell client through the protected GPU grant and
/// the content seam, without DRM master or native presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellGpuContentProof {
    /// Absolute path of the client executable.
    pub client: PathBuf,
    /// Arguments the client is executed with, passed through unchanged.
    pub client_args: Vec<OsString>,
    /// Exported as `SOPHIA_SHELL_CONFIG`, the component launch contract.
    pub config: Option<PathBuf>,
    pub seat: String,
    pub render_node: PathBuf,
    /// Pinned `MAJOR:MINOR@PCI_BUS_ID` (or `@none`) the grant must select.
    pub expected_device: Option<String>,
    pub output: ShellGpuProofExtent,
    pub surface: ShellGpuProofSurface,
    /// One synthetic answer per verified render, in order.
    pub outcomes: Vec<ShellGpuProofOutcome>,
    pub end: ShellGpuProofEnd,
    pub pixels: ShellGpuProofPixels,
    /// Whether content admission grants discrete input.
    pub discrete_input: bool,
    pub timeout: Duration,
}

/// Why proof parameters were refused before any device access.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ShellGpuProofError {
    RelativePath {
        name: &'static str,
    },
    EmptySeat,
    ZeroExtent {
        name: &'static str,
    },
    /// A surface dimension exceeds the resource width or height limit.
    ResourceExtent {
        name: &'static str,
        value: u32,
        limit: u32,
    },
    SurfaceOutsideOutput,
    /// The extent across the edge exceeds the panel or reservation limit.
    Thickness {
        thickness: u32,
        limit: u32,
    },
    /// One full-surface resource would exceed the resource byte limit.
    ResourceBytes {
        bytes: Option<u64>,
        limit: u64,
    },
    /// The surface covers more of the output than the coverage limit allows.
    Coverage {
        percent_limit: u32,
    },
    RenderCount {
        count: usize,
    },
    Timeout {
        timeout: Duration,
    },
    MalformedExpectedDevice,
}

impl fmt::Display for ShellGpuProofError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath { name } => write!(formatter, "{name} must be an absolute path"),
            Self::EmptySeat => formatter.write_str("seat must not be empty"),
            Self::ZeroExtent { name } => write!(formatter, "{name} must not be zero"),
            Self::ResourceExtent { name, value, limit } => {
                write!(
                    formatter,
                    "{name} {value} exceeds the resource limit {limit}"
                )
            }
            Self::SurfaceOutsideOutput => formatter.write_str("surface does not fit the output"),
            Self::Thickness { thickness, limit } => write!(
                formatter,
                "surface thickness {thickness} across its edge exceeds the limit {limit}"
            ),
            Self::ResourceBytes { bytes, limit } => match bytes {
                Some(bytes) => write!(
                    formatter,
                    "surface needs {bytes} resource bytes, above the limit {limit}"
                ),
                None => formatter.write_str("surface resource byte size overflows"),
            },
            Self::Coverage { percent_limit } => write!(
                formatter,
                "surface covers more than {percent_limit}% of the output"
            ),
            Self::RenderCount { count } => write!(
                formatter,
                "render count {count} is outside 1..={SHELL_GPU_PROOF_MAX_RENDERS}"
            ),
            Self::Timeout { timeout } => write!(
                formatter,
                "timeout {timeout:?} is outside {SHELL_GPU_PROOF_MIN_TIMEOUT:?}..={SHELL_GPU_PROOF_MAX_TIMEOUT:?}"
            ),
            Self::MalformedExpectedDevice => formatter
                .write_str("expected device must be MAJOR:MINOR@PCI_BUS_ID or MAJOR:MINOR@none"),
        }
    }
}

impl std::error::Error for ShellGpuProofError {}

impl ShellGpuContentProof {
    /// Check every parameter against the proof's content limits without
    /// touching the filesystem or a device.
    pub fn validate(&self) -> Result<(), ShellGpuProofError> {
        let limits = shell_gpu_proof_content_limits();
        for (name, path) in [
            ("client", Some(&self.client)),
            ("config", self.config.as_ref()),
            ("render node", Some(&self.render_node)),
        ] {
            if path.is_some_and(|path| !path.is_absolute()) {
                return Err(ShellGpuProofError::RelativePath { name });
            }
        }
        if self.seat.is_empty() {
            return Err(ShellGpuProofError::EmptySeat);
        }
        for (name, value) in [
            ("output width", self.output.width),
            ("output height", self.output.height),
            ("surface width", self.surface.width),
            ("surface height", self.surface.height),
        ] {
            if value == 0 {
                return Err(ShellGpuProofError::ZeroExtent { name });
            }
        }
        for (name, value, limit) in [
            ("surface width", self.surface.width, limits.max_width_px),
            ("surface height", self.surface.height, limits.max_height_px),
        ] {
            if value > limit {
                return Err(ShellGpuProofError::ResourceExtent { name, value, limit });
            }
        }
        if self.surface.width > self.output.width || self.surface.height > self.output.height {
            return Err(ShellGpuProofError::SurfaceOutsideOutput);
        }
        // Allocation refuses a panel thicker than the panel extent and a
        // reservation above the reservation extent; the proof grants a
        // reservation equal to the thickness, so both apply.
        let thickness_limit = limits.max_panel_extent.min(limits.max_reservation_extent);
        if self.surface.thickness() > thickness_limit {
            return Err(ShellGpuProofError::Thickness {
                thickness: self.surface.thickness(),
                limit: thickness_limit,
            });
        }
        let bytes = self.surface.bytes();
        if bytes.is_none_or(|bytes| bytes > limits.max_resource_bytes) {
            return Err(ShellGpuProofError::ResourceBytes {
                bytes,
                limit: limits.max_resource_bytes,
            });
        }
        let area = u64::from(self.surface.width) * u64::from(self.surface.height);
        let output_area = u64::from(self.output.width) * u64::from(self.output.height);
        if u128::from(area) * 100
            > u128::from(output_area) * u128::from(limits.max_content_coverage_percent)
        {
            return Err(ShellGpuProofError::Coverage {
                percent_limit: limits.max_content_coverage_percent,
            });
        }
        if !(1..=SHELL_GPU_PROOF_MAX_RENDERS).contains(&self.outcomes.len()) {
            return Err(ShellGpuProofError::RenderCount {
                count: self.outcomes.len(),
            });
        }
        if !(SHELL_GPU_PROOF_MIN_TIMEOUT..=SHELL_GPU_PROOF_MAX_TIMEOUT).contains(&self.timeout) {
            return Err(ShellGpuProofError::Timeout {
                timeout: self.timeout,
            });
        }
        if self
            .expected_device
            .as_deref()
            .is_some_and(|device| !expected_device_is_well_formed(device))
        {
            return Err(ShellGpuProofError::MalformedExpectedDevice);
        }
        Ok(())
    }
}

fn expected_device_is_well_formed(device: &str) -> bool {
    let Some((numbers, bus)) = device.split_once('@') else {
        return false;
    };
    let Some((major, minor)) = numbers.split_once(':') else {
        return false;
    };
    let canonical = |text: &str| {
        !text.is_empty()
            && text.bytes().all(|byte| byte.is_ascii_digit())
            && text
                .parse::<u32>()
                .is_ok_and(|value| value.to_string() == text)
    };
    canonical(major)
        && canonical(minor)
        && !bus.is_empty()
        && bus
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.'))
}

//! Typed parameters of the shell GPU content proof.
//!
//! Every expectation the proof checks comes from these parameters, so a shell
//! of any shape can be proven without Sophia knowing which client it is.
//! [`ShellGpuContentProof::validate`] is pure: it runs before the proof reads
//! the render inventory or touches a device, and refuses anything it cannot
//! bound.

use sophia_config::ShellComponentEdge;
use sophia_protocol::ContentPixelRect;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// Upper bound on verified renders in one proof.
pub const SHELL_GPU_PROOF_MAX_RENDERS: usize = 16;
/// Upper bound on any one output or surface dimension, in pixels. It keeps a
/// surface's byte size well inside `u32` and its thickness inside the `u16`
/// reservation extent.
pub const SHELL_GPU_PROOF_MAX_EXTENT: u32 = 16_384;
pub const SHELL_GPU_PROOF_MIN_TIMEOUT: Duration = Duration::from_secs(1);
pub const SHELL_GPU_PROOF_MAX_TIMEOUT: Duration = Duration::from_secs(120);
pub const SHELL_GPU_PROOF_DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

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
        // Both are bounded by SHELL_GPU_PROOF_MAX_EXTENT once validated.
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
    /// Whether content admission grants discrete input.
    pub discrete_input: bool,
    pub timeout: Duration,
}

/// Why proof parameters were refused before any device access.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ShellGpuProofError {
    RelativePath { name: &'static str },
    EmptySeat,
    ZeroExtent { name: &'static str },
    OversizedExtent { name: &'static str, value: u32 },
    SurfaceOutsideOutput,
    RenderCount { count: usize },
    Timeout { timeout: Duration },
    MalformedExpectedDevice,
}

impl fmt::Display for ShellGpuProofError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath { name } => write!(formatter, "{name} must be an absolute path"),
            Self::EmptySeat => formatter.write_str("seat must not be empty"),
            Self::ZeroExtent { name } => write!(formatter, "{name} must not be zero"),
            Self::OversizedExtent { name, value } => write!(
                formatter,
                "{name} {value} exceeds {SHELL_GPU_PROOF_MAX_EXTENT}"
            ),
            Self::SurfaceOutsideOutput => formatter.write_str("surface does not fit the output"),
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
    /// Check every parameter without touching the filesystem or a device.
    pub fn validate(&self) -> Result<(), ShellGpuProofError> {
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
            if value > SHELL_GPU_PROOF_MAX_EXTENT {
                return Err(ShellGpuProofError::OversizedExtent { name, value });
            }
        }
        if self.surface.width > self.output.width || self.surface.height > self.output.height {
            return Err(ShellGpuProofError::SurfaceOutsideOutput);
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

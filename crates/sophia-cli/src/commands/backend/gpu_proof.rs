use super::super::prelude::arg_value;
use sophia_config::ShellComponentEdge;
use sophia_session::{
    SHELL_GPU_PROOF_DEFAULT_TIMEOUT, ShellGpuContentProof, ShellGpuProofEnd, ShellGpuProofExtent,
    ShellGpuProofOutcome, ShellGpuProofPixels, ShellGpuProofSurface,
};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

const COMMAND: &str = "shell-gpu-content-proof";
const ARM_ENV: &str = "SOPHIA_SHELL_GPU_PROOF_ARM";
const EXPECTED_DEVICE_ENV: &str = "SOPHIA_SHELL_GPU_EXPECTED_DEVICE";

pub(super) fn try_run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.first().map(String::as_str) == Some("sophia-shell-gpu-proof-exec") {
        let client = arg_value(args, "--client").ok_or("proof child requires --client")?;
        sophia_session::exec_shell_gpu_proof_client(
            std::path::Path::new(&client),
            &client_args(args),
        )?;
        return Ok(true);
    }
    if args.first().map(String::as_str) != Some(COMMAND) {
        return Ok(false);
    }
    if std::env::var_os(ARM_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err(format!("set {ARM_ENV}=1 to run the shell GPU content proof").into());
    }
    let expected_device = match std::env::var(EXPECTED_DEVICE_ENV) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let proof = parse(args, expected_device)?;
    // Refuse bad parameters here, before the proof reads any device state.
    proof
        .validate()
        .map_err(|error| format!("invalid {COMMAND} parameters: {error}"))?;
    sophia_session::run_shell_gpu_content_proof(&proof)?;
    Ok(true)
}

fn client_args(args: &[String]) -> Vec<OsString> {
    args.iter()
        .filter_map(|arg| arg.strip_prefix("--client-arg="))
        .map(OsString::from)
        .collect()
}

fn required(args: &[String], key: &str, shape: &str) -> Result<String, String> {
    arg_value(args, key).ok_or_else(|| format!("{COMMAND} requires {key}={shape}"))
}

fn parse(
    args: &[String],
    expected_device: Option<String>,
) -> Result<ShellGpuContentProof, Box<dyn std::error::Error>> {
    let client = required(args, "--client", "/absolute/path")?;
    let output = extent(&required(args, "--output", "WxH")?, "--output")?;
    let surface = extent(&required(args, "--surface", "WxH")?, "--surface")?;
    let edge = match required(args, "--edge", "top|bottom|left|right")?.as_str() {
        "top" => ShellComponentEdge::Top,
        "bottom" => ShellComponentEdge::Bottom,
        "left" => ShellComponentEdge::Left,
        "right" => ShellComponentEdge::Right,
        other => return Err(format!("unknown --edge {other:?}").into()),
    };
    let outcomes = required(args, "--outcomes", "presented[,renderer-failed...]")?
        .split(',')
        .map(|outcome| match outcome {
            "presented" => Ok(ShellGpuProofOutcome::PresentedSynthetic),
            "renderer-failed" => Ok(ShellGpuProofOutcome::RendererFailed),
            other => Err(format!("unknown --outcomes entry {other:?}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let end = match required(args, "--end", "client-exits|stop-client")?.as_str() {
        "client-exits" => ShellGpuProofEnd::ClientExits,
        "stop-client" => ShellGpuProofEnd::StopClient,
        other => return Err(format!("unknown --end {other:?}").into()),
    };
    let pixels = match required(args, "--pixels", "contract|full-surface-raster")?.as_str() {
        "contract" => ShellGpuProofPixels::Contract,
        "full-surface-raster" => ShellGpuProofPixels::FullSurfaceRaster,
        other => return Err(format!("unknown --pixels {other:?}").into()),
    };
    let discrete_input = match required(args, "--discrete-input", "granted|denied")?.as_str() {
        "granted" => true,
        "denied" => false,
        other => return Err(format!("unknown --discrete-input {other:?}").into()),
    };
    let timeout = match arg_value(args, "--timeout-ms") {
        Some(value) => Duration::from_millis(
            value
                .parse()
                .map_err(|_| format!("--timeout-ms is not a number: {value:?}"))?,
        ),
        None => SHELL_GPU_PROOF_DEFAULT_TIMEOUT,
    };
    Ok(ShellGpuContentProof {
        transport: sophia_config::ShellTransportSelection::parse(&required(
            args,
            "--transport",
            "current-ipc|9p2000.L",
        )?)?,
        client: PathBuf::from(client),
        client_args: client_args(args),
        config: arg_value(args, "--config").map(PathBuf::from),
        seat: arg_value(args, "--seat").unwrap_or_else(|| "seat0".into()),
        render_node: arg_value(args, "--render-node")
            .unwrap_or_else(|| "/dev/dri/renderD128".into())
            .into(),
        expected_device,
        output,
        surface: ShellGpuProofSurface {
            edge,
            width: surface.width,
            height: surface.height,
        },
        outcomes,
        end,
        pixels,
        discrete_input,
        timeout,
    })
}

fn extent(value: &str, key: &str) -> Result<ShellGpuProofExtent, String> {
    let malformed = || format!("{key} must be WxH in pixels: {value:?}");
    let (width, height) = value.split_once('x').ok_or_else(malformed)?;
    let dimension = |text: &str| {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(malformed());
        }
        text.parse::<u32>()
            .map_err(|_| format!("{key} dimension overflows: {value:?}"))
    };
    Ok(ShellGpuProofExtent {
        width: dimension(width)?,
        height: dimension(height)?,
    })
}

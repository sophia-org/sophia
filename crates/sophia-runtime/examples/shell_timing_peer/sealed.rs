//! Sealed sources for the paced peer, through the pinned SDK adapter. The
//! workload, resource IDs and pixel bytes match the byte path; only the
//! transfer differs. There is no byte fallback: a closed or byte-only
//! channel fails the run.
use sophia_image_producer::{ResourceStatusKind, create_mapped_isb};
use sophia_protocol::{ContentLimits, ContentResourceId};
use sophia_shell_client::{
    ImageChannelState, ImagePath, OfferState, ProducerMetadata, ProducerRoleBounds,
    ShellConnection, ShellImageConfig,
};
use std::{
    os::fd::OwnedFd,
    path::Path,
    time::{Duration, Instant},
};

use super::Result;

const KEYS: [&str; 6] = [
    "max_resource_bytes",
    "max_width",
    "max_height",
    "max_admitted_outputs",
    "transfer_slots",
    "transfer_timeout_ms",
];

/// The role bounds and transfer timeout come from the launch configuration, never from fixture
/// constants, and must equal the content limits the host then sends.
#[derive(Clone, Copy)]
pub struct SealedConfig {
    pub bounds: ProducerRoleBounds,
    pub transfer_timeout_ms: u32,
}

pub fn parse(value: &serde_json::Value) -> Result<SealedConfig> {
    let object = value.as_object().ok_or("image must be an object")?;
    if object.len() != KEYS.len() || object.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("image requires exactly {}", KEYS.join(", ")).into());
    }
    let number = |key: &str| -> Result<u64> {
        value[key]
            .as_u64()
            .filter(|n| *n != 0)
            .ok_or_else(|| format!("invalid image {key}").into())
    };
    let small = |key: &str| -> Result<u32> { Ok(u32::try_from(number(key)?)?) };
    Ok(SealedConfig {
        bounds: ProducerRoleBounds {
            max_resource_bytes: number("max_resource_bytes")?,
            max_width: small("max_width")?,
            max_height: small("max_height")?,
            max_admitted_outputs: small("max_admitted_outputs")?,
            transfer_slots: small("transfer_slots")?,
        },
        transfer_timeout_ms: small("transfer_timeout_ms")?,
    })
}

/// The endpoint node sits beside the role socket in its export directory.
pub fn image_config(
    config: SealedConfig,
    socket: &Path,
    origin_ms: u64,
    wait: Duration,
) -> Result<ShellImageConfig> {
    let endpoint_dir = socket
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or("file endpoint has no directory")?;
    Ok(ShellImageConfig {
        endpoint_dir: endpoint_dir.to_owned(),
        bounds: config.bounds,
        transfer_timeout_ms: config.transfer_timeout_ms,
        startup_deadline_ms: origin_ms + wait.as_millis() as u64,
        formats_capacity: 64,
    })
}

/// The SDK publishes these bounds against the host's image limits; the host
/// derives its own from the same content limits.
pub fn check_limits(config: SealedConfig, limits: &ContentLimits) -> Result<()> {
    let bounds = config.bounds;
    if bounds.max_resource_bytes != limits.max_resource_bytes
        || bounds.max_width != limits.max_width_px
        || bounds.max_height != limits.max_height_px
        || bounds.max_admitted_outputs != limits.max_outputs
        || bounds.transfer_slots != limits.max_open_transfers
        || config.transfer_timeout_ms != limits.transfer_timeout_ms
    {
        return Err("image bounds differ from content limits".into());
    }
    Ok(())
}

pub fn wait_ready(client: &mut ShellConnection, wait: Duration) -> Result<()> {
    let deadline = Instant::now() + wait;
    loop {
        client.poll_io()?;
        let adapter = client.image_adapter().ok_or("image adapter absent")?;
        match (adapter.channel(), adapter.path()) {
            (Some(ImageChannelState::Ready), Some(ImagePath::Descriptor)) => return Ok(()),
            (Some(ImageChannelState::Closed), _) | (_, Some(ImagePath::Error)) => {
                return Err("sealed image channel unavailable".into());
            }
            _ => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("sealed image channel not ready".into());
        }
        client.wait_for_io(left)?;
    }
}

/// Pixel preparation, kept before the offer is journalled like the byte path.
pub fn prepare(width: u32, height: u32, max_bytes: u64, generation: u64) -> Result<OwnedFd> {
    let mut source = create_mapped_isb(width, height, max_bytes).map_err(|e| format!("{e:?}"))?;
    super::canvas::fill_pixels(source.pixels_mut(), generation);
    source.finish().map_err(|e| format!("{e:?}").into())
}

pub fn begin(
    client: &mut ShellConnection,
    metadata: ProducerMetadata,
    source: OwnedFd,
) -> Result<u64> {
    client
        .image_begin(metadata, source, Instant::now())
        .map(|admission| admission.offer)
        .map_err(|e| format!("image begin refused: {:?}", e.error).into())
}

/// Upload acceptance needs exact Accepted status and a Retained release.
/// Only then is the offer forgotten; the resource stays until Retire. The
/// producer ignores a status after SourceReleased, so a release first fails.
pub fn settle(
    client: &mut ShellConnection,
    offer: u64,
    resource: ContentResourceId,
    wait: Duration,
) -> Result<()> {
    let deadline = Instant::now() + wait;
    loop {
        client.poll_io()?;
        let info = client
            .image_adapter()
            .and_then(|adapter| adapter.offer(offer))
            .ok_or("image offer lost")?;
        if info.resource != resource.id || info.resource_generation != resource.generation {
            return Err("image offer names another resource".into());
        }
        match info.state {
            OfferState::Settled
                if info.status == Some(ResourceStatusKind::Accepted) && info.disposition == 1 =>
            {
                client
                    .image_forget(offer)
                    .map_err(|e| format!("image forget refused: {e:?}"))?;
                return Ok(());
            }
            OfferState::Begun | OfferState::Admitted | OfferState::Sent => {}
            state => {
                return Err(format!(
                    "image offer ended {state:?} status {:?} disposition {}",
                    info.status, info.disposition
                )
                .into());
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!(
                "image offer did not settle: {:?} status {:?} disposition {}",
                info.state, info.status, info.disposition
            )
            .into());
        }
        client.wait_for_io(left)?;
    }
}

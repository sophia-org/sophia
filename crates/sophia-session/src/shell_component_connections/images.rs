//! Session policy for CPU images and content accounting clocks.
use super::Connection;
use sophia_protocol::shell_files::{
    ShellImageDiscovery, ShellImageFormats, ShellImageLimits, ShellImageRoleBounds,
};
use sophia_runtime::{ShellIsbImageOwner, ShellTransportError};
use std::sync::OnceLock;
use std::time::Instant;

/// Content clocks must survive component replacement: a worker may complete
/// against a retired store after its role-specific service has been dropped.
pub(crate) fn content_now_msec() -> u64 {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    u64::try_from(STARTED.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(super) fn configure(connection: &mut Connection) -> Result<(), ShellTransportError> {
    let limits = connection
        .limits
        .as_ref()
        .ok_or(ShellTransportError::MissingCapability)?;
    const VALIDATION_BYTES: u32 = 65_536;
    if connection.images.is_none() {
        // The worker survives reduced reconnect grants. Its hard resource cap
        // is the role contract ceiling; each admission checks the current grant.
        let maximum = sophia_protocol::ContentLimits::prototype(limits.grant).max_resource_bytes;
        connection.images = Some(
            ShellIsbImageOwner::new(maximum as usize, VALIDATION_BYTES as usize)
                .map_err(|error| ShellTransportError::Io(error.to_string()))?,
        );
    }
    configure_transport(&mut connection.transport, limits)
}

pub(crate) fn configure_transport(
    transport: &mut sophia_runtime::ShellComponentTransport,
    limits: &sophia_protocol::ContentLimits,
) -> Result<(), ShellTransportError> {
    configure_with(limits, |d, l, f, b| {
        transport.configure_image_channel(d, l, f, b)
    })
}

#[cfg(feature = "native-session")]
pub(crate) fn configure_legacy(
    transport: &mut sophia_runtime::ShellSessionTransport,
    limits: &sophia_protocol::ContentLimits,
) -> Result<(), ShellTransportError> {
    configure_with(limits, |d, l, f, b| {
        transport.configure_image_channel(d, l, f, b)
    })
}

fn configure_with(
    limits: &sophia_protocol::ContentLimits,
    configure: impl FnOnce(
        ShellImageDiscovery,
        ShellImageLimits,
        ShellImageFormats,
        ShellImageRoleBounds,
    ) -> Result<(), ShellTransportError>,
) -> Result<(), ShellTransportError> {
    let offers = limits.max_open_transfers.min(4);
    let generation = limits.grant.connection_epoch;
    // The source is charged outside the resource envelope in the registry.
    // Publish space for one default ring as well as the role's destinations.
    let ring_bytes = sophia_runtime::OwnedRingGeometry::new(
        1,
        usize::try_from(limits.max_resource_bytes)
            .map_err(|_| ShellTransportError::MissingCapability)?,
    )
    .map_err(|error| ShellTransportError::Io(error.to_string()))?
    .source_bytes() as u64;
    const VALIDATION_BYTES: u32 = 65_536;
    configure(
        ShellImageDiscovery {
            connection_epoch: generation,
            channel_generation: generation,
            endpoint: sophia_runtime::image_channel::IMAGE_ENDPOINT_BASENAME.into(),
            source_kinds: 1,
            max_offers: offers,
            max_packets_per_turn: 1,
            hello_timeout_ms: 250,
            validation_timeout_ms: limits.transfer_timeout_ms,
            validation_chunk_bytes: VALIDATION_BYTES,
            capture_interest_timeout_ms: 500,
            formats_generation: 1,
            limits_generation: limits.limits_generation,
        },
        ShellImageLimits {
            channel_generation: generation,
            limits_generation: limits.limits_generation,
            max_resource_bytes: limits.max_resource_bytes,
            max_sealed_backing_bytes: limits.max_resident_bytes
                + limits.max_retiring_bytes
                + ring_bytes,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: offers,
            max_validation_jobs: offers,
            max_capture_jobs: 0,
            max_admitted_outputs: limits.max_outputs,
            max_format_rows_per_output: 256,
            max_width: limits.max_width_px,
            max_height: limits.max_height_px,
            validation_timeout_ms: limits.transfer_timeout_ms,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: VALIDATION_BYTES,
            max_packets_per_turn: 1,
        },
        ShellImageFormats {
            channel_generation: generation,
            formats_generation: 1,
            limits_generation: limits.limits_generation,
            rows: Vec::new(),
        },
        ShellImageRoleBounds {
            max_resource_bytes: limits.max_resource_bytes,
            max_width: limits.max_width_px,
            max_height: limits.max_height_px,
            max_admitted_outputs: limits.max_outputs,
            transfer_slots: limits.max_open_transfers,
        },
    )
}

pub(super) fn visit_worker<'a>(
    connection: &'a Connection,
    visit: &mut impl FnMut(rustix::event::PollFd<'a>),
) {
    if let Some(owner) = &connection.images
        && owner.pending() != 0
    {
        visit(rustix::event::PollFd::from_borrowed_fd(
            owner.readiness(),
            rustix::event::PollFlags::IN,
        ));
    }
}

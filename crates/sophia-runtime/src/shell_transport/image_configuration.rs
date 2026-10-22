//! Explicit owner admission of the authenticated image transport.
use super::{ShellComponentTransport, ShellTransportError, files};
use sophia_protocol::shell_files::{
    SHELL_IMAGE_SOURCE_DMA_BUF, ShellImageDiscovery, ShellImageFormats, ShellImageLimits,
    ShellImageRoleBounds,
};

impl ShellComponentTransport {
    /// Enables the owned-ring image channel for a future authenticated file epoch.
    /// The owner supplies its actual budgets; this does not grant a capability
    /// until the peer requests it and ordinary content admission succeeds.
    pub fn configure_image_channel(
        &mut self,
        discovery: ShellImageDiscovery,
        limits: ShellImageLimits,
        formats: ShellImageFormats,
        role_bounds: ShellImageRoleBounds,
    ) -> Result<(), ShellTransportError> {
        if self.wire.is_some() || self.negotiation.is_some() || !self.authentication_required {
            return Err(ShellTransportError::WrongContentGrant);
        }
        // A refuse-only channel still publishes coherent owned-copy budgets,
        // never a zero-budget fake grant. DMA-BUF is additive to the ring and
        // needs the capture owner's complete limits group and format rows.
        let owned = crate::owned_ring_protocol::SOURCE_KIND;
        if discovery.source_kinds != owned
            && !(cfg!(feature = "experimental-shell-capture")
                && discovery.source_kinds == owned | SHELL_IMAGE_SOURCE_DMA_BUF
                && limits.max_capture_jobs != 0
                && !formats.rows.is_empty())
        {
            return Err(ShellTransportError::MissingCapability);
        }
        discovery
            .coherent(
                &limits,
                &formats,
                discovery.connection_epoch,
                discovery.connection_epoch,
                &role_bounds,
            )
            .map_err(|error| {
                ShellTransportError::Io(format!("invalid image configuration: {error:?}"))
            })?;
        if discovery.endpoint != crate::image_channel::IMAGE_ENDPOINT_BASENAME {
            return Err(ShellTransportError::Io(
                "image endpoint differs from bound basename".into(),
            ));
        }
        self.image_config = Some(files::ShellImageChannelConfig {
            discovery,
            limits,
            formats,
            role_bounds,
        });
        Ok(())
    }
}

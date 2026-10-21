//! Explicit owner admission of the authenticated image transport.
use super::{ShellComponentTransport, ShellTransportError, files};
use sophia_protocol::shell_files::{
    ShellImageDiscovery, ShellImageFormats, ShellImageLimits, ShellImageRoleBounds,
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
        // DMA-BUF needs the later capture owner. A refuse-only channel still
        // publishes coherent owned-copy budgets, never a zero-budget fake grant.
        if discovery.source_kinds != crate::owned_ring_protocol::SOURCE_KIND {
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

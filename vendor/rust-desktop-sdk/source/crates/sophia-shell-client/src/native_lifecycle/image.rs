//! Transfer a verified ISB offer into the native resource registry.
use super::*;
use sophia_image_producer::{OfferState, ResourceStatusKind};

impl NativeLauncherSession {
    /// Negotiated bounds for constructing the image adapter and its sources.
    pub fn content_limits(&self) -> &ContentLimits {
        &self.lifecycle.limits
    }

    /// Enable ISB discovery without transferring ownership of the role loop.
    pub fn enable_image(
        &mut self,
        config: crate::ShellImageConfig,
        origin: std::time::Instant,
        origin_ms: u64,
    ) -> Result<(), ShellClientError> {
        self.lifecycle.service(&self.connection)?;
        self.connection.enable_image(config, origin, origin_ms)
    }

    pub fn image_readiness(&self) -> Result<Option<crate::ImageReadiness<'_>>, ShellClientError> {
        self.connection.image_readiness()
    }

    pub fn image_adapter(&self) -> Option<&crate::files::image_adapter::ImageAdapter> {
        self.connection.image_adapter()
    }

    /// The connection returns the source descriptor on refusal before custody.
    pub fn image_begin(
        &mut self,
        metadata: crate::ProducerMetadata,
        source: std::os::fd::OwnedFd,
        now: std::time::Instant,
    ) -> Result<crate::ImageBeginAdmission, crate::ImageBeginError> {
        if self.lifecycle.service(&self.connection).is_err() {
            return Err(crate::ImageBeginError {
                error: crate::ImageAdapterError::Closed,
                source: Some(source),
            });
        }
        self.connection.image_begin(metadata, source, now)
    }

    pub fn image_cancel(
        &mut self,
        offer: u64,
        now: std::time::Instant,
    ) -> Result<Ticket, ShellClientError> {
        self.connection.image_cancel(offer, now)
    }

    /// Forget a settled offer that will not be adopted.
    pub fn image_forget(&mut self, offer: u64) -> Result<(), crate::ImageAdapterError> {
        self.connection.image_forget(offer)
    }

    /// Adopt verified Accepted/Retained storage and consume its settled offer.
    pub fn adopt_image(&mut self, offer: u64) -> Result<ContentResourceId, ShellClientError> {
        self.connection
            .adopt_native_image(&mut self.lifecycle, offer)
    }
}

impl ShellConnection {
    /// Register an ISB as an Accepted native resource and forget its producer
    /// offer in one local operation. The offer must belong to this connection
    /// and the lifecycle's content grant, with producer-verified Accepted and
    /// Settled/Retained. No caller-supplied status can authorize adoption.
    ///
    /// Failure consumes neither the offer nor a registry slot. A duplicate
    /// resource identity is refused even if it comes from a different offer.
    /// ChannelClosed does not invalidate an already retained resource; role
    /// disconnect or a stale grant does. Retirement and ResourceReleased still
    /// govern the resource after adoption. Continue delivering observations in
    /// order: the original Accepted is an idempotent no-op after adoption,
    /// including while Retiring; a contradictory status fails the lifecycle.
    pub fn adopt_native_image(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        offer: u64,
    ) -> Result<ContentResourceId, ShellClientError> {
        self.wire.require_native_launcher()?;
        lifecycle.service(self)?;
        let invalid = || ShellClientError::InvalidArgument("native image not adoptable");
        let adapter = self.image.as_ref().ok_or_else(invalid)?;
        let info = adapter.offer(offer).ok_or_else(invalid)?;
        let metadata = adapter.ticket_metadata(offer).ok_or_else(invalid)?;
        let limits = &lifecycle.limits;
        if info.state != OfferState::Settled
            || info.status != Some(ResourceStatusKind::Accepted)
            || info.disposition != 1
            || metadata.admission_epoch != limits.grant.content_grant_epoch
            || metadata.width > limits.max_width_px
            || metadata.height > limits.max_height_px
            || metadata.declared_bytes > limits.max_resource_bytes
        {
            return Err(invalid());
        }
        // Shape and source-kind admissibility were checked by the producer on
        // begin and exact ImageAdmitted. Recheck the lifecycle's own bounds here.
        let resource = ContentResourceId {
            id: metadata.resource,
            generation: metadata.resource_generation,
        };
        if lifecycle.resource_index(resource).is_some() {
            return Err(ShellClientError::InvalidArgument(
                "native resource already tracked",
            ));
        }
        let index = lifecycle
            .resources
            .iter()
            .position(Option::is_none)
            .ok_or(ShellClientError::QueueSaturated)?;
        // Nothing fallible follows forget. Forget prevents re-adoption after
        // ResourceReleased has freed the lifecycle slot.
        self.image_forget(offer).map_err(|_| invalid())?;
        lifecycle.resources[index] = Some(resources::Resource {
            id: resource,
            state: NativeResourceState::Accepted,
            ticket: None,
            image_offer: offer,
        });
        Ok(resource)
    }
}

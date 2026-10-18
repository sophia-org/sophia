//! Transfer a verified ISB offer into the native resource registry.
use super::*;
use sophia_image_producer::{OfferState, ResourceStatusKind};

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
            image_offer: Some(offer),
        });
        Ok(resource)
    }
}

use sophia_protocol::{
    AuthorityKind, BufferSource, NamespaceId, Region, Size, SurfaceTransaction,
    SurfaceTransactionReadiness, TransactionId,
};

use crate::{XAuthorityAccessError, XResourceId, XWindowTable};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XDrawingUpdateKind {
    PresentPixmap,
    ShmPutImage,
    CoreDraw,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XDrawingUpdate {
    pub transaction: TransactionId,
    pub requester_namespace: NamespaceId,
    pub target_window: XResourceId,
    pub kind: XDrawingUpdateKind,
    pub buffer: BufferSource,
    /// Pixel extent of the drawing window before any descendant-to-toplevel
    /// presentation projection.
    ///
    /// This is what the content was asked to fill. It is not a measurement of
    /// the buffer: a client that has not answered its last configure presents
    /// the pixmap it already has, and the two part company for as long as that
    /// takes.
    pub presentation_extent: Option<Size>,
    /// Measured extent of the raster being presented, where the producer knows
    /// it. `None` leaves the content extent to follow the presentation extent,
    /// which is correct only for a raster the authority itself sized.
    pub raster_extent: Option<Size>,
    pub damage: Region,
    pub previous_committed_generation: u64,
    pub timeout_msec: u32,
}

impl XDrawingUpdate {
    pub fn present_pixmap(
        transaction: TransactionId,
        requester_namespace: NamespaceId,
        target_window: XResourceId,
        pixmap: u32,
        damage: Region,
        previous_committed_generation: u64,
        timeout_msec: u32,
    ) -> Self {
        Self {
            transaction,
            requester_namespace,
            target_window,
            kind: XDrawingUpdateKind::PresentPixmap,
            buffer: BufferSource::XPixmap { pixmap },
            presentation_extent: None,
            raster_extent: None,
            damage,
            previous_committed_generation,
            timeout_msec,
        }
    }

    pub fn present_buffer(
        transaction: TransactionId,
        requester_namespace: NamespaceId,
        target_window: XResourceId,
        buffer: BufferSource,
        presentation_extent: Size,
        raster_extent: Size,
        damage: Region,
        previous_committed_generation: u64,
        timeout_msec: u32,
    ) -> Self {
        Self {
            transaction,
            requester_namespace,
            target_window,
            kind: XDrawingUpdateKind::PresentPixmap,
            buffer,
            presentation_extent: Some(presentation_extent),
            raster_extent: Some(raster_extent),
            damage,
            previous_committed_generation,
            timeout_msec,
        }
    }

    pub fn shm_put_image(
        transaction: TransactionId,
        requester_namespace: NamespaceId,
        target_window: XResourceId,
        handle: u64,
        damage: Region,
        previous_committed_generation: u64,
        timeout_msec: u32,
    ) -> Self {
        Self {
            transaction,
            requester_namespace,
            target_window,
            kind: XDrawingUpdateKind::ShmPutImage,
            buffer: BufferSource::CpuBuffer { handle },
            presentation_extent: None,
            raster_extent: None,
            damage,
            previous_committed_generation,
            timeout_msec,
        }
    }

    pub fn core_draw(
        transaction: TransactionId,
        requester_namespace: NamespaceId,
        target_window: XResourceId,
        handle: u64,
        damage: Region,
        previous_committed_generation: u64,
        timeout_msec: u32,
    ) -> Self {
        Self {
            transaction,
            requester_namespace,
            target_window,
            kind: XDrawingUpdateKind::CoreDraw,
            buffer: BufferSource::CpuBuffer { handle },
            presentation_extent: None,
            raster_extent: None,
            damage,
            previous_committed_generation,
            timeout_msec,
        }
    }
}

pub fn surface_transaction_from_drawing_update(
    windows: &XWindowTable,
    update: XDrawingUpdate,
) -> Result<SurfaceTransaction, XAuthorityAccessError> {
    if !update.transaction.is_valid() {
        return Err(XAuthorityAccessError::InvalidResource);
    }
    if !update.requester_namespace.is_valid() {
        return Err(XAuthorityAccessError::InvalidNamespace);
    }
    if !update.target_window.is_valid() {
        return Err(XAuthorityAccessError::InvalidResource);
    }
    if matches!(update.buffer, BufferSource::None) {
        return Err(XAuthorityAccessError::InvalidResource);
    }

    let window = windows
        .get(update.target_window)
        .ok_or(XAuthorityAccessError::UnknownResource)?;

    if window.namespace != update.requester_namespace {
        return Err(XAuthorityAccessError::CrossNamespaceDenied);
    }
    if !window.surface.is_valid() {
        return Err(XAuthorityAccessError::InvalidSurface);
    }

    let presentation_extent = update.presentation_extent.unwrap_or(Size {
        width: window.geometry.width,
        height: window.geometry.height,
    });
    // Without a measurement the authority sized this raster itself, so it spans
    // what it was asked to fill. With one, it is whatever the client presented.
    let raster_extent = update.raster_extent.unwrap_or(presentation_extent);
    if presentation_extent.width <= 0
        || presentation_extent.height <= 0
        || raster_extent.width <= 0
        || raster_extent.height <= 0
    {
        return Err(XAuthorityAccessError::InvalidResource);
    }

    Ok(SurfaceTransaction {
        transaction: update.transaction,
        authority: AuthorityKind::SophiaX,
        // Filled in by the runtime, which is what holds the window's input
        // shape; this builder sees only the window table.
        input_region: None,
        surface: window.surface,
        namespace: Some(window.namespace),
        target_geometry: window.interior_geometry(),
        content: sophia_protocol::SurfaceContentSet::singleton(update.buffer, raster_extent),
        presentation_extent,
        damage: update.damage,
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: update.timeout_msec,
        previous_committed_generation: update.previous_committed_generation,
    })
}

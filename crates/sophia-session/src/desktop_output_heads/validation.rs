//! Complete topology heads for validation.
//!
//! A topology test names the primary plane of every enabled CRTC with a
//! framebuffer allocated for the test, because amdgpu refuses to enable a CRTC
//! whose primary plane is off, and a plane-less test would be judged against
//! whatever a previous owner left bound. Each head's mode blob and framebuffer
//! belong to the card of that exact head: a mirror group or a second GPU has
//! other cards, so nothing here resolves a card from an output.

use std::io;

use sophia_backend_live::{
    LibdrmNativeAtomicHead, LibdrmNativeOutputTiming, LibdrmNativePrimaryPlaneObjects,
    LibdrmNativePrimaryPlanePropertyHandles, LibdrmNativePrimaryPlaneSelection,
    LibdrmNativeValidationBufferDevice, LibdrmNativeValidationFramebuffer, RealAtomicScanoutCard,
    resolve_native_connector_mode,
};
use sophia_protocol::OutputId;

use super::{
    LiveNativeOutputTopologyHardware, NativeOutputComposedHead, NativeOutputHeadUnavailable,
    NativeOutputTopologyHardware,
};

/// One complete validation head and the framebuffer it names, tied to the head
/// index whose card allocated it.
#[derive(Debug)]
pub struct NativeOutputValidationHead<B> {
    head: LibdrmNativeAtomicHead,
    framebuffer: LibdrmNativeValidationFramebuffer<B>,
    index: usize,
}

impl<B> NativeOutputValidationHead<B> {
    pub const fn index(&self) -> usize {
        self.index
    }

    pub const fn framebuffer(&self) -> &LibdrmNativeValidationFramebuffer<B> {
        &self.framebuffer
    }
}

impl<B> AsRef<LibdrmNativeAtomicHead> for NativeOutputValidationHead<B> {
    fn as_ref(&self) -> &LibdrmNativeAtomicHead {
        &self.head
    }
}

/// Per-head-index resources: each call names the head whose card owns the
/// resource, never an output.
pub trait NativeOutputValidationResources {
    type Buffer;

    fn allocate_validation_framebuffer(
        &self,
        index: usize,
        width: u32,
        height: u32,
    ) -> io::Result<LibdrmNativeValidationFramebuffer<Self::Buffer>>;

    fn release_validation_framebuffer(
        &self,
        index: usize,
        framebuffer: LibdrmNativeValidationFramebuffer<Self::Buffer>,
    ) -> io::Result<()>;

    fn release_validation_mode_blob(&self, index: usize, blob: u64) -> io::Result<()>;
}

/// Builds one complete head from a blob already created on head `index`'s card,
/// allocating its framebuffer there. A framebuffer that cannot be allocated
/// releases the blob on that same card.
pub fn compose_native_output_validation_head<R>(
    resources: &R,
    index: usize,
    selection: LibdrmNativePrimaryPlaneSelection,
    properties: LibdrmNativePrimaryPlanePropertyHandles,
    mode_blob: u64,
    timing: LibdrmNativeOutputTiming,
) -> Result<
    NativeOutputComposedHead<NativeOutputValidationHead<R::Buffer>>,
    NativeOutputHeadUnavailable,
>
where
    R: NativeOutputValidationResources,
{
    let framebuffer = match resources.allocate_validation_framebuffer(
        index,
        timing.width,
        timing.height,
    ) {
        Ok(framebuffer) => framebuffer,
        Err(error) => {
            report_release(resources.release_validation_mode_blob(index, mode_blob));
            tracing::warn!(schema = 1, index, %error, "native validation framebuffer unavailable");
            return Err(NativeOutputHeadUnavailable::FramebufferUnavailable);
        }
    };
    let head = LibdrmNativeAtomicHead::new(
        LibdrmNativePrimaryPlaneObjects::new(
            selection.connector_handle(),
            selection.crtc_handle(),
            selection.plane_handle(),
            framebuffer.framebuffer(),
            mode_blob,
            framebuffer.size(),
        ),
        properties,
    );
    Ok(NativeOutputComposedHead {
        head: NativeOutputValidationHead {
            head,
            framebuffer,
            index,
        },
        mode_blob,
    })
}

/// Releases a composed validation head: its framebuffer, then its blob, both on
/// the card of the head that created them, attempting both.
pub fn release_native_output_validation_head<R>(
    resources: &R,
    composed: NativeOutputComposedHead<NativeOutputValidationHead<R::Buffer>>,
) where
    R: NativeOutputValidationResources,
{
    let NativeOutputComposedHead { head, mode_blob } = composed;
    report_release(resources.release_validation_framebuffer(head.index, head.framebuffer));
    report_release(resources.release_validation_mode_blob(head.index, mode_blob));
}

/// Failing to release is worth a line but not worth failing work that already
/// completed.
fn report_release(result: io::Result<()>) {
    if let Err(error) = result {
        tracing::warn!(schema = 1, %error, "sophia_native_output_validation release failed");
    }
}

type LiveValidationBuffer = <RealAtomicScanoutCard as LibdrmNativeValidationBufferDevice>::Buffer;

impl NativeOutputValidationResources for LiveNativeOutputTopologyHardware<'_> {
    type Buffer = LiveValidationBuffer;

    fn allocate_validation_framebuffer(
        &self,
        index: usize,
        width: u32,
        height: u32,
    ) -> io::Result<LibdrmNativeValidationFramebuffer<Self::Buffer>> {
        LibdrmNativeValidationFramebuffer::allocate(self.scanout.card(index), width, height)
    }

    fn release_validation_framebuffer(
        &self,
        index: usize,
        framebuffer: LibdrmNativeValidationFramebuffer<Self::Buffer>,
    ) -> io::Result<()> {
        framebuffer.release(self.scanout.card(index))
    }

    fn release_validation_mode_blob(&self, index: usize, blob: u64) -> io::Result<()> {
        sophia_backend_live::LibdrmNativePrimaryPlaneResourceDevice::destroy_mode_blob(
            self.scanout.card(index),
            blob,
        )
    }
}

impl NativeOutputTopologyHardware for LiveNativeOutputTopologyHardware<'_> {
    type Head = NativeOutputValidationHead<LiveValidationBuffer>;

    fn compose_head(
        &self,
        _output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        let Some(index) = self.head_for_connector(connector) else {
            return Err(NativeOutputHeadUnavailable::MissingSelection);
        };
        let selection = self.scanout.selection(index);
        let card = self.scanout.card(index);
        let properties = self.properties(index)?;

        // A timing this connector never advertised is a configuration error, and it
        // fails here rather than as an opaque kernel refusal later.
        let Ok(Some(mode)) =
            resolve_native_connector_mode(card, selection.connector_handle(), timing)
        else {
            return Err(NativeOutputHeadUnavailable::UnknownTiming);
        };
        let Ok(mode_blob) =
            sophia_backend_live::LibdrmNativePrimaryPlaneResourceDevice::create_mode_blob(
                card, mode,
            )
        else {
            return Err(NativeOutputHeadUnavailable::ModeBlobRefused);
        };
        if mode_blob == 0 {
            return Err(NativeOutputHeadUnavailable::ModeBlobRefused);
        }
        compose_native_output_validation_head(self, index, selection, properties, mode_blob, timing)
    }

    /// Every validation head releases through `release_head` on its own card.
    /// A bare blob carries no head index, so it cannot name its card; the
    /// resolver never hands one back.
    fn release_mode_blob(&self, output: OutputId, blob: u64) {
        tracing::warn!(
            schema = 1,
            output = output.raw(),
            blob,
            "validation blob released without its head"
        );
    }

    fn release_head(&self, _output: OutputId, composed: NativeOutputComposedHead<Self::Head>) {
        release_native_output_validation_head(self, composed);
    }
}

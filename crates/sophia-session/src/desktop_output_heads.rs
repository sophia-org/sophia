use std::fmt;

use sophia_backend_live::{
    LibdrmNativeAtomicHead, LibdrmNativeAtomicTopologyHead, LibdrmNativeCurrentFramebufferHead,
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming,
    LibdrmNativePrimaryPlanePropertyHandles, LibdrmNativePrimaryPlaneResourceDevice,
    LibdrmNativePrimaryPlaneSelection, LiveProductionNativeScanout,
    compose_native_head_from_current_framebuffer, discover_native_primary_plane_property_handles,
    resolve_native_connector_mode,
};
use sophia_protocol::{OutputId, Size};

use crate::desktop_output_topology::NativeOutputActivationPlan;

/// Why one output could not contribute a head.
///
/// Each arm is a different fact about the host, and the resolver keeps them apart
/// because they send an operator to different places: a missing route is a
/// selection problem, missing properties are a driver problem, and an unknown
/// timing is a configuration problem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeOutputHeadUnavailable {
    /// The output has no connector and CRTC route on this host.
    MissingSelection,
    /// Connector, CRTC, or plane properties an atomic modeset needs are absent.
    MissingProperties,
    /// The connector does not advertise the requested timing.
    UnknownTiming,
    /// The mode exists, but the kernel would not create a blob for it.
    ModeBlobRefused,
    /// No framebuffer of the right size exists for this output yet.
    ///
    /// Carries what the CRTC actually scans out, because "displays nothing" and
    /// "displays the wrong size" want different fixes: the first needs a buffer
    /// allocated, the second needs one allocated *at the new mode's size*. Only
    /// applying hits this; validation names no framebuffer at all, which is exactly
    /// why a topology can be checked before one exists.
    NeedsFramebuffer { have: Option<(u32, u32)> },
}

/// One head plus the mode blob it names.
///
/// The blob is returned rather than kept by the composer because its lifetime
/// belongs to the resolution that requested it, not to the hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeOutputComposedHead<H> {
    pub head: H,
    pub mode_blob: u64,
}

/// The hardware side of resolving a plan into heads.
///
/// The resolver decides which outputs participate and at which timing; this
/// composes the KMS objects that express those decisions. The split is what keeps
/// the decisions testable without a DRM device, and it keeps DRM handle types out
/// of the resolver entirely — `Head` is opaque here.
pub trait NativeOutputTopologyHardware {
    /// A composed head. Real hardware yields `LibdrmNativeAtomicTopologyHead`.
    type Head;

    /// Composes one head for `connector` at `timing`, creating the mode blob it
    /// needs.
    ///
    /// Keyed by connector rather than by output because a mirror group is several
    /// heads behind one output: keying by output would compose the same head twice
    /// and leave the group's other connector dark. The output travels alongside
    /// because blobs are released against the card that owns it.
    fn compose_head(
        &self,
        output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable>;

    /// Releases a blob this hardware created for `output`. Called for every blob
    /// the resolver took, including on the failure path.
    ///
    /// The output is named because a blob belongs to the card that created it, and
    /// a session can span more than one card. Releasing against the wrong device
    /// would leak the blob and disturb an unrelated one.
    fn release_mode_blob(&self, output: OutputId, blob: u64);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeOutputHeadResolveError {
    /// The plan named an output the capability set does not describe.
    MissingCapability(u64),
    /// One output could not be composed, so the whole topology fails closed.
    Unavailable {
        output: u64,
        cause: NativeOutputHeadUnavailable,
    },
    /// Every target in the plan is disabled, so there is no topology to validate.
    /// A backstop: reconciliation refuses an all-disabled candidate before a plan
    /// exists, so nothing in production reaches this today.
    NoEnabledOutputs,
}

impl fmt::Display for NativeOutputHeadResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCapability(output) => {
                write!(formatter, "native output {output} has no DRM capability")
            }
            Self::Unavailable { output, cause } => {
                let reason = match cause {
                    NativeOutputHeadUnavailable::MissingSelection => "has no connector route",
                    NativeOutputHeadUnavailable::MissingProperties => {
                        "is missing atomic modeset properties"
                    }
                    NativeOutputHeadUnavailable::UnknownTiming => {
                        "does not advertise the requested timing"
                    }
                    NativeOutputHeadUnavailable::ModeBlobRefused => "was refused a mode blob",
                    NativeOutputHeadUnavailable::NeedsFramebuffer { have: None } => {
                        "scans out nothing, so there is no framebuffer to reuse"
                    }
                    NativeOutputHeadUnavailable::NeedsFramebuffer { have: Some((w, h)) } => {
                        return write!(
                            formatter,
                            "native output {output} scans out a {w}x{h} framebuffer, which is not \
the requested mode's size"
                        );
                    }
                };
                write!(formatter, "native output {output} {reason}")
            }
            Self::NoEnabledOutputs => {
                formatter.write_str("native activation plan enables no output")
            }
        }
    }
}

impl std::error::Error for NativeOutputHeadResolveError {}

/// Heads resolved from one plan, owning the mode blobs they name.
///
/// The blobs are kernel resources created for this resolution and belong to it.
/// Tying them together means a caller cannot submit heads whose blobs have already
/// been released, and cannot leak them by taking an early return.
#[derive(Debug)]
pub struct NativeOutputTopologyHeads<'a, H>
where
    H: NativeOutputTopologyHardware,
{
    hardware: &'a H,
    heads: Vec<H::Head>,
    blobs: Vec<(OutputId, u64)>,
}

impl<H> NativeOutputTopologyHeads<'_, H>
where
    H: NativeOutputTopologyHardware,
{
    pub fn heads(&self) -> &[H::Head] {
        &self.heads
    }

    pub fn len(&self) -> usize {
        self.heads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heads.is_empty()
    }
}

impl<H> Drop for NativeOutputTopologyHeads<'_, H>
where
    H: NativeOutputTopologyHardware,
{
    fn drop(&mut self) {
        for (output, blob) in self.blobs.drain(..) {
            self.hardware.release_mode_blob(output, blob);
        }
    }
}

/// Resolves an activation plan into the heads that express it.
///
/// Only enabled targets become heads. A disabled output is expressed by having no
/// head rather than by an inactive one, which matches how disablement reaches
/// policy: by omission from the complete snapshot.
///
/// This resolves the requested topology only. Rollback heads are deliberately not
/// produced here. Restoring a previous topology needs that topology's own mode
/// blobs, still alive at the moment rollback runs, which is after an apply has
/// already failed; creating them alongside every candidate would spend kernel
/// resources on every plan for a path most plans never take.
///
/// Failure is all-or-nothing, and a partial resolution releases every blob it
/// created on the way out. A topology missing one head is a different desktop, not
/// a degraded version of the requested one.
pub fn resolve_native_output_topology_heads<'a, H>(
    plan: &NativeOutputActivationPlan,
    capabilities: &[LibdrmNativeOutputCapability],
    hardware: &'a H,
) -> Result<NativeOutputTopologyHeads<'a, H>, NativeOutputHeadResolveError>
where
    H: NativeOutputTopologyHardware,
{
    // Held as the owner from the first blob onward, so every early return below
    // runs its Drop and releases what has been created so far.
    let mut resolved = NativeOutputTopologyHeads {
        hardware,
        heads: Vec::new(),
        blobs: Vec::new(),
    };

    for target in plan.targets() {
        let requested = target.requested();
        if !requested.enabled {
            continue;
        }
        let output = target.output();
        let raw = output.raw();

        if !capabilities
            .iter()
            .any(|capability| capability.output() == output)
        {
            return Err(NativeOutputHeadResolveError::MissingCapability(raw));
        }

        let timing = LibdrmNativeOutputTiming::new(
            requested.mode.width,
            requested.mode.height,
            requested.mode.refresh_millihz,
        );
        match hardware.compose_head(output, &requested.connector, timing) {
            Ok(composed) => {
                resolved.blobs.push((output, composed.mode_blob));
                resolved.heads.push(composed.head);
            }
            Err(cause) => {
                return Err(NativeOutputHeadResolveError::Unavailable { output: raw, cause });
            }
        }
    }

    if resolved.heads.is_empty() {
        return Err(NativeOutputHeadResolveError::NoEnabledOutputs);
    }
    Ok(resolved)
}

/// The hardware side of resolving a plan into heads that can actually be applied.
///
/// Separate from `NativeOutputTopologyHardware` because the two answer different
/// questions. A topology head asks whether the kernel would accept this desktop; a
/// scanout head asks what to put on screen, so it carries a framebuffer and can
/// fail for reasons validation never encounters.
pub trait NativeOutputScanoutHardware {
    /// A composed head. Real hardware yields `LibdrmNativeAtomicHead`.
    type Head;

    /// Composes the head this connector should run under the new topology.
    fn compose_apply_head(
        &self,
        output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable>;

    /// Composes the head that restores what this output runs today.
    ///
    /// Sourced from current state rather than derived from the apply head, because
    /// rollback is not the inverse of apply: restoring a mode needs that mode's own
    /// blob, and a buffer sized for it.
    fn compose_rollback_head(
        &self,
        output: OutputId,
        connector: &str,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable>;

    fn release_mode_blob(&self, output: OutputId, blob: u64);
}

/// Apply and rollback heads resolved together, owning every blob they name.
///
/// They are resolved together deliberately. A rollback that is sourced after an
/// apply has failed is sourced from a desktop that is already wrong; taking it
/// while the previous topology is still running is what makes it a restore rather
/// than a guess.
#[derive(Debug)]
pub struct NativeOutputScanoutHeads<'a, H>
where
    H: NativeOutputScanoutHardware,
{
    hardware: &'a H,
    apply: Vec<H::Head>,
    rollback: Vec<H::Head>,
    blobs: Vec<(OutputId, u64)>,
}

impl<H> NativeOutputScanoutHeads<'_, H>
where
    H: NativeOutputScanoutHardware,
{
    pub fn apply(&self) -> &[H::Head] {
        &self.apply
    }

    pub fn rollback(&self) -> &[H::Head] {
        &self.rollback
    }

    pub fn len(&self) -> usize {
        self.apply.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apply.is_empty()
    }
}

impl<H> Drop for NativeOutputScanoutHeads<'_, H>
where
    H: NativeOutputScanoutHardware,
{
    fn drop(&mut self) {
        for (output, blob) in self.blobs.drain(..) {
            self.hardware.release_mode_blob(output, blob);
        }
    }
}

/// Resolves an activation plan into heads that can be applied, plus their rollback.
///
/// Fails closed exactly as the validation resolver does, and for the same reason: a
/// desktop missing one head is a different desktop. Here the stakes are higher,
/// because a partial apply is visible.
pub fn resolve_native_output_scanout_heads<'a, H>(
    plan: &NativeOutputActivationPlan,
    capabilities: &[LibdrmNativeOutputCapability],
    hardware: &'a H,
) -> Result<NativeOutputScanoutHeads<'a, H>, NativeOutputHeadResolveError>
where
    H: NativeOutputScanoutHardware,
{
    let mut resolved = NativeOutputScanoutHeads {
        hardware,
        apply: Vec::new(),
        rollback: Vec::new(),
        blobs: Vec::new(),
    };

    for target in plan.targets() {
        let requested = target.requested();
        if !requested.enabled {
            continue;
        }
        let output = target.output();
        let raw = output.raw();

        if !capabilities
            .iter()
            .any(|capability| capability.output() == output)
        {
            return Err(NativeOutputHeadResolveError::MissingCapability(raw));
        }

        let timing = LibdrmNativeOutputTiming::new(
            requested.mode.width,
            requested.mode.height,
            requested.mode.refresh_millihz,
        );
        let composed = hardware
            .compose_apply_head(output, &requested.connector, timing)
            .map_err(|cause| NativeOutputHeadResolveError::Unavailable { output: raw, cause })?;
        resolved.blobs.push((output, composed.mode_blob));
        resolved.apply.push(composed.head);

        let restore = hardware
            .compose_rollback_head(output, &requested.connector)
            .map_err(|cause| NativeOutputHeadResolveError::Unavailable { output: raw, cause })?;
        resolved.blobs.push((output, restore.mode_blob));
        resolved.rollback.push(restore.head);
    }

    if resolved.apply.is_empty() {
        return Err(NativeOutputHeadResolveError::NoEnabledOutputs);
    }
    Ok(resolved)
}

/// Composes heads from the live scanout's own KMS objects.
///
/// Every output the session drives already has a selection naming its connector,
/// CRTC, and plane, and the card that owns them. This reads those rather than
/// re-deriving an assignment, so a validated topology names exactly the objects the
/// session runs on.
pub struct LiveNativeOutputTopologyHardware<'a> {
    scanout: &'a LiveProductionNativeScanout,
    capabilities: &'a [LibdrmNativeOutputCapability],
}

impl<'a> LiveNativeOutputTopologyHardware<'a> {
    pub const fn new(
        scanout: &'a LiveProductionNativeScanout,
        capabilities: &'a [LibdrmNativeOutputCapability],
    ) -> Self {
        Self {
            scanout,
            capabilities,
        }
    }

    /// The head driving a named connector.
    ///
    /// Capabilities carry both the connector's name and its DRM id, and they are
    /// readable by the time a plan exists, so this is where a configured name
    /// becomes an exact head without reintroducing the id-to-name circularity that
    /// the grouping had.
    fn head_for_connector(&self, connector: &str) -> Option<usize> {
        let capability = self
            .capabilities
            .iter()
            .find(|capability| capability.connector_key() == connector)?;
        // Connector numbers are local to a DRM device. Only the capability's
        // bound opaque head disambiguates equal numbers on admitted GPUs.
        self.scanout.head_index_for_head(capability.head()?)
    }
}

impl LiveNativeOutputTopologyHardware<'_> {
    fn properties(
        &self,
        index: usize,
    ) -> Result<LibdrmNativePrimaryPlanePropertyHandles, NativeOutputHeadUnavailable> {
        let selection = self.scanout.selection(index);
        discover_native_primary_plane_property_handles(
            self.scanout.card(index),
            selection.connector_handle(),
            selection.crtc_handle(),
            selection.plane_handle(),
        )
        .properties
        .ok_or(NativeOutputHeadUnavailable::MissingProperties)
    }

    /// Builds a scanout head from what the CRTC already displays, releasing the
    /// blob if it cannot. A blob outliving the head it was made for is a leak.
    fn compose_from_current_framebuffer(
        &self,
        output: OutputId,
        selection: LibdrmNativePrimaryPlaneSelection,
        properties: LibdrmNativePrimaryPlanePropertyHandles,
        mode_blob: u64,
        scanout: Size,
    ) -> Result<NativeOutputComposedHead<LibdrmNativeAtomicHead>, NativeOutputHeadUnavailable> {
        let Some(index) = self.scanout.primary_head_index(output) else {
            self.release(output, mode_blob);
            return Err(NativeOutputHeadUnavailable::MissingSelection);
        };
        match compose_native_head_from_current_framebuffer(
            self.scanout.card(index),
            selection,
            properties,
            mode_blob,
            scanout,
        ) {
            LibdrmNativeCurrentFramebufferHead::Composed(head) => {
                Ok(NativeOutputComposedHead { head, mode_blob })
            }
            LibdrmNativeCurrentFramebufferHead::SizeMismatch { width, height } => {
                self.release(output, mode_blob);
                Err(NativeOutputHeadUnavailable::NeedsFramebuffer {
                    have: Some((width, height)),
                })
            }
            LibdrmNativeCurrentFramebufferHead::NoFramebuffer
            | LibdrmNativeCurrentFramebufferHead::Unreadable => {
                self.release(output, mode_blob);
                Err(NativeOutputHeadUnavailable::NeedsFramebuffer { have: None })
            }
        }
    }
}

impl NativeOutputTopologyHardware for LiveNativeOutputTopologyHardware<'_> {
    type Head = LibdrmNativeAtomicTopologyHead;

    fn compose_head(
        &self,
        output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        let Some(index) = self.head_for_connector(connector) else {
            return Err(NativeOutputHeadUnavailable::MissingSelection);
        };
        let _ = output;
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
        let Ok(mode_blob) = card.create_mode_blob(mode) else {
            return Err(NativeOutputHeadUnavailable::ModeBlobRefused);
        };
        if mode_blob == 0 {
            return Err(NativeOutputHeadUnavailable::ModeBlobRefused);
        }

        Ok(NativeOutputComposedHead {
            head: LibdrmNativeAtomicTopologyHead::from_selection(selection, mode_blob, properties),
            mode_blob,
        })
    }

    fn release_mode_blob(&self, output: OutputId, blob: u64) {
        self.release(output, blob);
    }
}

impl LiveNativeOutputTopologyHardware<'_> {
    /// Release against the card that created it. Failing to release is worth a line
    /// but not worth failing work that already completed.
    fn release(&self, output: OutputId, blob: u64) {
        let Some(index) = self.scanout.primary_head_index(output) else {
            return;
        };
        if let Err(error) = self.scanout.card(index).destroy_mode_blob(blob) {
            tracing::warn!(
                schema = 1,
                blob,
                %error,
                "sophia_native_output_mode_blob release failed"
            );
        }
    }
}

impl NativeOutputScanoutHardware for LiveNativeOutputTopologyHardware<'_> {
    type Head = LibdrmNativeAtomicHead;

    fn compose_apply_head(
        &self,
        output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        let Some(index) = self.head_for_connector(connector) else {
            return Err(NativeOutputHeadUnavailable::MissingSelection);
        };
        let selection = self.scanout.selection(index);
        let card = self.scanout.card(index);
        let properties = self.properties(index)?;

        let Ok(Some(mode)) =
            resolve_native_connector_mode(card, selection.connector_handle(), timing)
        else {
            return Err(NativeOutputHeadUnavailable::UnknownTiming);
        };
        let Ok(mode_blob) = card.create_mode_blob(mode) else {
            return Err(NativeOutputHeadUnavailable::ModeBlobRefused);
        };

        // The only framebuffer that exists before anything is rendered is the one
        // already on screen, so this can apply a topology whose scanout size matches
        // what is displayed and nothing else. A mode change needs a buffer allocated
        // at the new size, which is renderer work, and declining here says so rather
        // than letting the kernel refuse with EINVAL.
        let (Ok(width), Ok(height)) = (i32::try_from(timing.width), i32::try_from(timing.height))
        else {
            self.release(output, mode_blob);
            return Err(NativeOutputHeadUnavailable::NeedsFramebuffer { have: None });
        };
        self.compose_from_current_framebuffer(
            output,
            selection,
            properties,
            mode_blob,
            Size { width, height },
        )
    }

    fn compose_rollback_head(
        &self,
        output: OutputId,
        connector: &str,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        let Some(index) = self.head_for_connector(connector) else {
            return Err(NativeOutputHeadUnavailable::MissingSelection);
        };
        let selection = self.scanout.selection(index);
        let card = self.scanout.card(index);
        let properties = self.properties(index)?;

        // The mode this output runs today, not the one being requested. A rollback
        // built from the requested mode would restore the failure.
        let Ok(mode_blob) = card.create_mode_blob_for_selection(selection) else {
            return Err(NativeOutputHeadUnavailable::ModeBlobRefused);
        };
        self.compose_from_current_framebuffer(
            output,
            selection,
            properties,
            mode_blob,
            selection.size(),
        )
    }

    fn release_mode_blob(&self, output: OutputId, blob: u64) {
        self.release(output, blob);
    }
}

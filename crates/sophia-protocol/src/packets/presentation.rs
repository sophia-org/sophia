use crate::{OutputId, Rect, SurfaceId, WmActionId, WmModifierMask};

pub const POLICY_MAX_PRESENTATION_OUTPUTS: usize = crate::POLICY_MAX_OUTPUTS;
pub const POLICY_MAX_SURFACE_INSTANCES: usize = crate::POLICY_MAX_SURFACES;
pub const POLICY_MAX_PRESENTATION_REGIONS: usize = crate::POLICY_MAX_SURFACES;
pub const POLICY_MAX_PRESENTATION_BINDINGS: usize = crate::POLICY_MAX_BINDINGS;

/// An admitted presentation changes how applications are shown, never their
/// allocations or the protected shell/trust layers above them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum PolicyPresentationMode {
    Overlay = 1,
    ReplaceApplications = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPresentationOutput {
    pub output: OutputId,
    pub generation: u64,
    pub coverage: Rect,
    pub mode: PolicyPresentationMode,
}

/// Instance identity belongs to the admitted WM connection. Source content
/// generations are resolved by Engine and do not change this interaction id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicySurfaceInstance {
    pub id: u64,
    pub generation: u64,
    pub output: OutputId,
    pub source: SurfaceId,
    pub destination: Rect,
    pub clip: Rect,
    pub opacity_millis: u16,
    pub z_index: u16,
    pub action: Option<WmActionId>,
}

/// Engine selects the palette and stroke treatment; WM supplies spatial intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum PolicyPresentationRegionRole {
    Backdrop = 1,
    Frame = 2,
    Emphasis = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPresentationRegion {
    pub id: u64,
    pub generation: u64,
    pub output: OutputId,
    pub geometry: Rect,
    pub clip: Rect,
    pub z_index: u16,
    pub role: PolicyPresentationRegionRole,
    pub action: Option<WmActionId>,
}

/// These bindings are eligible only in the matching presented modal scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPresentationBinding {
    pub action: WmActionId,
    pub keycode: u32,
    pub modifiers: WmModifierMask,
}

/// A complete WM presentation publication. Absence withdraws presentation.
///
/// The connection epoch is supplied by the authenticated projection envelope,
/// not repeated in every client-controlled record. All ids are unique across
/// instances and regions; z order is unique within each output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyPresentation {
    pub generation: u64,
    pub keyboard_output: Option<OutputId>,
    pub outputs: Vec<PolicyPresentationOutput>,
    pub instances: Vec<PolicySurfaceInstance>,
    pub regions: Vec<PolicyPresentationRegion>,
    pub bindings: Vec<PolicyPresentationBinding>,
}

/// Reduced identity captured from an actually presented publication. Keyboard
/// actions use target id/generation zero; pointer actions name both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPresentationIdentity {
    pub publication_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    pub presentation_epoch: u64,
    pub target_id: u64,
    pub target_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum PolicyPresentationOutcome {
    Presented = 1,
    Revoked = 2,
    Withdrawn = 3,
}

/// Rendering completion and input revocation are separate from the ordinary
/// projection settlement. Each receipt names one output's actual identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyPresentationReceipt {
    pub connection_epoch: u64,
    pub publication_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    pub presentation_epoch: u64,
    pub outcome: PolicyPresentationOutcome,
}

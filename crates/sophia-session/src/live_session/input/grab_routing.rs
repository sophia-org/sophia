//! Which surface a held pointer grab routes to.

use sophia_protocol::{ClientAdmissionId, SurfaceId};

/// The surface an event goes to while an explicit pointer grab is held.
///
/// X11's rule is owner_events: the event goes to the window under the pointer
/// when that window belongs to the grabbing client, and to the grab window
/// otherwise. The authority already applies that per window, choosing between
/// the window under the pointer and the grab window -- but it can only choose
/// within the surface named here, so naming the anchor unconditionally settled
/// the question before the authority ever saw it.
///
/// A menu is where that shows. A toolkit grabs on the window that was clicked
/// and only then creates and maps its popup, so the lease anchors to a surface
/// the pointer is about to leave. Every click inside the open menu reached the
/// window beneath it, the menu never received the press that dismisses it, and
/// it stayed mapped over whatever came next.
///
/// Same client only, and that is what keeps a drag working: once the pointer
/// leaves the client's surfaces there is no eligible hit, so the anchor stands
/// and grab ordering survives a drag outside its own geometry.
pub(super) fn grab_routed_surface(
    hit: Option<SurfaceId>,
    lease_target: SurfaceId,
    lease_admission: ClientAdmissionId,
    admission_of: impl Fn(SurfaceId) -> Option<ClientAdmissionId>,
) -> SurfaceId {
    hit.filter(|surface| admission_of(*surface) == Some(lease_admission))
        .unwrap_or(lease_target)
}

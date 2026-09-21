use sophia_protocol::{
    AxisSpan, BufferSource, ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance,
    ClientAuthenticationMethod, DeviceId, InputEventKind, LayoutNodeKind, NamespaceCapabilities,
    NamespaceContext, NamespaceId, NamespacePortalCapability, NamespaceProfile, OutputEdge,
    OutputId, OutputReservation, OutputTopologyEntry, OutputTopologySnapshot, Point,
    PolicyPresentationState, PortalBrokerRequestPacket, PortalDecision, PortalGrant,
    PortalGrantState, PortalRequest, PortalTransfer, PortalTransferKind, Rect, Region,
    RoutedInputRequest, SeatId, Size, SurfaceConstraints, SurfaceId, SurfacePlacementPreference,
    SurfacePresentationRole, SurfaceRasterClass, SurfaceRasterRequirements, SurfaceRasterTransform,
    TransactionId,
};
use sophia_x_authority::*;
include!("x11_wire/transport_events.rs");
include!("x11_wire/core_decode.rs");
include!("x11_wire/graphics_decode.rs");
include!("x11_wire/core_dispatch.rs");
include!("x11_wire/extensions_dispatch.rs");
include!("x11_wire/xtest_decode.rs");
include!("x11_wire/xtest_admission_socket.rs");
include!("x11_wire/render_picture_lifetime.rs");
include!("x11_wire/render_clip_reset.rs");
include!("x11_wire/withdrawn_state.rs");
include!("x11_wire/rendering_dispatch.rs");
include!("x11_wire/properties_dispatch.rs");
include!("x11_wire/selection_cut_buffer.rs");
include!("x11_wire/output_and_draw.rs");
include!("x11_wire/image_readback.rs");
include!("x11_wire/density_fidelity.rs");
include!("x11_wire/put_image_replay.rs");
include!("x11_wire/put_image_pixels.rs");
include!("x11_wire/graceful_disconnect.rs");
include!("x11_wire/text_and_scroll.rs");
include!("x11_wire/resources_frontend.rs");
include!("x11_wire/admission_frontend.rs");
include!("x11_wire/extension_enumeration_socket.rs");
include!("x11_wire/no_operation_socket.rs");
include!("x11_wire/lookup_color.rs");
include!("x11_wire/extension_minor_classification.rs");
include!("x11_wire/setup_failure_containment.rs");
include!("x11_wire/xfixes_stalled_watcher.rs");
include!("x11_wire/xfixes_namespace_isolation.rs");
include!("x11_wire/clipboard_frontend.rs");
include!("x11_wire/socket_observation.rs");
include!("x11_wire/map_hierarchy.rs");
include!("x11_wire/output_reservation_socket.rs");
include!("x11_wire/routed_service.rs");
include!("x11_wire/focus_routing.rs");
include!("x11_wire/pointer_queries.rs");
include!("x11_wire/xi_list_input_devices.rs");
include!("x11_wire/xi_virtual_source.rs");
include!("x11_wire/glx_pbuffer.rs");
include!("x11_wire/dri3_plane_bounds_socket.rs");
include!("x11_wire/drawing_completions.rs");
include!("x11_wire/support_requests.rs");
include!("x11_wire/support_extensions.rs");

#[test]
fn present_feedback_phases_accept_copy_and_flip_order_once() {
    let mut copy = XPresentFeedbackPhases::default();
    assert!(copy.observe_idle());
    assert!(!copy.finished());
    assert!(!copy.observe_idle());
    assert!(copy.observe_complete());
    assert!(copy.finished());
    assert!(!copy.observe_complete());

    let mut flip = XPresentFeedbackPhases::default();
    assert!(flip.observe_complete());
    assert!(!flip.finished());
    assert!(flip.observe_idle());
    assert!(flip.finished());
}

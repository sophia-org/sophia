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
include!("x11_wire/setup_and_glx.rs");
include!("x11_wire/focus_timestamps.rs");
include!("x11_wire/focus_viewability.rs");
include!("x11_wire/atom_lifetime.rs");
include!("x11_wire/window_backgrounds.rs");
include!("x11_wire/core_decode.rs");
include!("x11_wire/graphics_decode.rs");
include!("x11_wire/core_dispatch.rs");
include!("x11_wire/dri3_dispatch.rs");
include!("x11_wire/extensions_dispatch.rs");
include!("x11_wire/xkb_xi_dispatch.rs");
include!("x11_wire/render_dispatch.rs");
include!("x11_wire/xfixes_dispatch.rs");
include!("x11_wire/shape_dispatch.rs");
include!("x11_wire/render_sampling_dispatch.rs");
include!("x11_wire/xtest_decode.rs");
include!("x11_wire/xtest_admission_socket.rs");
include!("x11_wire/event_delivery_socket.rs");
include!("x11_wire/active_window_socket.rs");
include!("x11_wire/render_picture_lifetime.rs");
include!("x11_wire/render_clip_reset.rs");
include!("x11_wire/withdrawn_state.rs");
include!("x11_wire/rendering_dispatch.rs");
include!("x11_wire/properties_dispatch.rs");
include!("x11_wire/colormap_dispatch.rs");
include!("x11_wire/wm_hints_dispatch.rs");
include!("x11_wire/selection_cut_buffer.rs");
include!("x11_wire/output_and_draw.rs");
include!("x11_wire/destroy_window_outputs.rs");
include!("x11_wire/put_image_outputs.rs");
include!("x11_wire/root_and_stacking.rs");
include!("x11_wire/include_inferiors.rs");
include!("x11_wire/border_geometry.rs");
include!("x11_wire/replay_stacking.rs");
include!("x11_wire/cursor_errors.rs");
include!("x11_wire/text_fill_style.rs");
include!("x11_wire/win_gravity.rs");
include!("x11_wire/gravity_notify.rs");
include!("x11_wire/image_readback.rs");
include!("x11_wire/density_fidelity.rs");
include!("x11_wire/put_image_replay.rs");
include!("x11_wire/put_image_pixels.rs");
include!("x11_wire/graceful_disconnect.rs");
include!("x11_wire/text_and_scroll.rs");
include!("x11_wire/resources_frontend.rs");
include!("x11_wire/frontend_transport.rs");
include!("x11_wire/drawing_frontend.rs");
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
include!("x11_wire/flooding_client.rs");
include!("x11_wire/big_requests.rs");
include!("x11_wire/send_event_marking.rs");
include!("x11_wire/value_mask_bits.rs");
include!("x11_wire/colormap_static.rs");
include!("x11_wire/color_allocations.rs");
include!("x11_wire/server_controls.rs");
include!("x11_wire/hierarchy_requests.rs");
include!("x11_wire/client_lifetime.rs");
include!("x11_wire/input_maps.rs");
include!("x11_wire/reparent_notify.rs");
include!("x11_wire/colormap_notify.rs");
include!("x11_wire/window_background_tile.rs");
include!("x11_wire/window_attribute_refusals.rs");
include!("x11_wire/bit_gravity.rs");
include!("x11_wire/send_event_routing.rs");
include!("x11_wire/toplevel_placement.rs");
include!("x11_wire/focus_routing.rs");
include!("x11_wire/pointer_queries.rs");
include!("x11_wire/popup_pointer_target.rs");
include!("x11_wire/key_focus_subtree.rs");
include!("x11_wire/key_focus_sentinels.rs");
include!("x11_wire/xi_list_input_devices.rs");
include!("x11_wire/xi_virtual_source.rs");
include!("x11_wire/glx_pbuffer.rs");
include!("x11_wire/dri3_plane_bounds_socket.rs");
include!("x11_wire/drawing_completions.rs");
include!("x11_wire/configure_redirect.rs");
include!("x11_wire/support_requests.rs");
include!("x11_wire/support_extensions.rs");
include!("x11_wire/support_render_shape.rs");

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

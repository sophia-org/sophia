#![cfg(all(test, unix))]

use super::*;
use crate::XAuthorityControlKind;
use sophia_protocol::{DeviceId, Point};
use std::sync::mpsc::sync_channel;

include!("tests/socket_core.rs");
include!("tests/review_private_deadline.rs");
include!("tests/private_lifecycle_integration.rs");
include!("tests/stalled_recipients.rs");
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_routed_input.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/x11_wire/partial_dispatch_departure.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_device_announcements.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_transitions.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_private_press.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_producer_handles.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_review_completion.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_control_outcomes.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_registry_records.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_operation_work.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_operation_credits.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_poisoned_authority.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_keyboard_debt.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_release_custody.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_release_answers.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_release_order.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_parked_holds.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_ordered_delivery.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_applied_claims.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_frame_delivery.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_handover_endpoints.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_places_and_homes.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_connection_binding.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_ending_reports.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_notice_passes.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_worker_spawn.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_capsule_admission.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_close_output.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_stop_and_wire.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_continuation_places.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_body_joins.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_destination_evidence.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_commitment_evidence.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_worker_context.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_departure_cleanup.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_admitted_numbers.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/routing_unreadable_endings.rs"
));
include!("tests/private_runner.rs");
include!("tests/private_runner_barrier.rs");
include!("tests/private_service.rs");
include!("tests/private_service_egress.rs");
include!("tests/private_destruction.rs");
include!("tests/private_destruction_deferral.rs");
include!("tests/private_worker_attachment.rs");
include!("tests/private_worker_attachment_exits.rs");
include!("tests/private_failed_retention.rs");
include!("tests/private_deferred_cleanup.rs");
include!("tests/private_idle_reclaim.rs");
include!("tests/private_deferred_cleanup_service.rs");
#[path = "../../tests/support/private_exclusive_bind.rs"]
mod private_exclusive_bind;
#[path = "../../tests/support/private_maintenance_scheduler.rs"]
mod private_maintenance_scheduler;
#[path = "../../tests/support/private_retained_drive.rs"]
mod private_retained_drive;

#[path = "../../tests/support/m3_acceptance.rs"]
pub(super) mod m3_acceptance;
include!("tests/private_producer_service.rs");
include!("tests/private_producer_port.rs");
include!("tests/private_producer_exits.rs");
include!("../../tests/support/private_producer_exit_controls.rs");
include!("../../tests/support/private_execution_lifetime.rs");
include!("tests/private_producer_runner.rs");
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_cleanup_supervision.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_key_service.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_transient_service.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_request_deferral.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_item_credit.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_refused_request.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_frozen_completion.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_freeze_binding.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_frozen_runner.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_frozen_service.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_state_only.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_departed_release.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_ingress_refusals.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_reserved_chord.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_refused_execution.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_control_backlog.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_frozen_transient.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/dispatch_ticket_failure.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/xi_fixed_point.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/xi_source_delivery.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/peer_write_failure.rs"
));

include!("tests/private_applied_state.rs");

include!("tests/private_applied_registry.rs");

include!("tests/private_applied_focus.rs");

include!("tests/private_native.rs");
include!("tests/private_xkb_selection.rs");
include!("tests/private_keyboard_preparation.rs");

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_terminal_service.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_live_native_disposal.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_live_recipient.rs"
));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/private_invocation_completion.rs"
));

include!("tests/ordered_codec.rs");
include!("tests/dispatch_surface_generation.rs");

#[path = "../../tests/support/private_control_cleanup.rs"]
mod private_control_cleanup;

#[cfg(unix)]
#[path = "../../tests/support/private_control_effect_groups.rs"]
mod private_control_effect_groups;

#[cfg(unix)]
#[path = "../../tests/support/private_control_peers.rs"]
mod private_control_peers;

#[cfg(unix)]
#[path = "../../tests/support/private_control_protocol.rs"]
mod private_control_protocol;

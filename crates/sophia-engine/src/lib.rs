mod prelude {
    pub(crate) use core::fmt;
    pub(crate) use std::collections::{BTreeMap, BTreeSet};
    pub(crate) use std::io;
    pub(crate) use std::sync::mpsc::{Receiver, TryRecvError};

    pub(crate) use sophia_portal::{
        MAX_NOTIFICATION_ACTION_LEN, MAX_NOTIFICATION_ACTIONS, MAX_NOTIFICATION_BODY_LEN,
        MAX_NOTIFICATION_SUMMARY_LEN, NotificationRequest, NotificationUrgency, PortalCommand,
    };
    pub(crate) use sophia_protocol::{
        AxisSpan, BrokerHealthPacket, BufferSource, ChromeActionKind, ChromeActionRequest,
        ChromeDescriptor, CommittedSurfaceState, DamageFrame, DeviceId, DisplayLabel,
        FrameSnapshot, InputEventKind, InputEventPacket, InputRoute, InputRouteOutcome,
        LayerSnapshot, LayoutNodeSnapshot, LayoutTransaction, MAX_CHROME_LABEL_LEN, OutputEdge,
        OutputHeadMapping, OutputId, OutputReservation, OutputTransform, Point, PortalTransferId,
        Rect, Region, RenderCommand, RenderCommandKind, ResizeSyncCapability, RoutedInputRequest,
        SanitizedChromeMetadata, SeatId, Size, SurfaceContentSet, SurfaceId,
        SurfaceOutputReservations, SurfacePresentationRole, SurfaceTransaction,
        SurfaceTransactionKey, SurfaceTransactionReadiness, TransactionCommit, TransactionId,
        TransactionOutcome, WmRequestKind, WmRequestPacket, WorkspaceId,
    };
    pub(crate) use sophia_runtime::{
        RuntimeScanoutState, SessionRuntimeCommand, SessionRuntimeLoop, SessionRuntimeObservation,
        SessionRuntimeObservationError, SessionRuntimeState, SophiaErrorExt, SophiaErrorKind,
    };
    pub(crate) use tracing::{debug, instrument, trace, warn};
}

mod backend_assembly;
mod chrome;
mod composition_cohort;
mod composition_plan;
mod compositor_graphics;
mod cursor;
mod drm;
mod engine;
mod error;
mod frame;
mod head;
mod input;
mod layout_epoch;
mod live_backend;
mod output;
mod output_power;
mod output_topology_transaction;
mod overview;
mod policy_projection;
mod raster_requirements;
mod render;
mod runtime_driver;
mod session;
mod shell_work_area;
mod shortcut;
mod surface_admission;
mod surface_content_stream;
mod tab_groups;
mod transaction_presentation;
mod visual_state;

pub use backend_assembly::*;
pub use chrome::*;
pub use composition_cohort::*;
pub use composition_plan::*;
pub use compositor_graphics::*;
pub use cursor::*;
pub use drm::*;
pub use engine::*;
pub use error::*;
pub use frame::*;
pub use head::*;
pub use input::*;
pub use layout_epoch::*;
pub use live_backend::*;
pub use output::*;
pub use output_power::*;
pub use output_topology_transaction::*;
pub use overview::*;
pub use policy_projection::*;
pub use raster_requirements::*;
pub use render::*;
pub use runtime_driver::*;
pub use session::*;
pub use shell_work_area::*;
pub use shortcut::*;
pub use surface_admission::*;
pub use surface_content_stream::*;
pub use tab_groups::*;
pub use transaction_presentation::*;
pub use visual_state::*;

pub use sophia_protocol::ToplevelActionCapabilityRef;
pub use sophia_runtime::{RuntimeScanoutState, SessionRuntimeObservation};

mod tab_chrome;
pub use tab_chrome::*;

mod translation;
pub use translation::*;

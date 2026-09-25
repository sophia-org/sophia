use sophia_portal::{ClipboardPortal, PortalCommand};
use sophia_protocol::{
    AuthorityKind, BufferSource, IpcCodecError, IpcMessageKind, NamespaceId, PortalDecision,
    PortalGrant, PortalGrantState, PortalTransfer, PortalTransferId, PortalTransferKind, Rect,
    Region, SOPHIA_IPC_MAGIC, Size, SurfaceConstraints, SurfaceId, SurfaceRasterClass,
    SurfaceRasterRequirements, SurfaceRasterTransform, SurfaceTransactionReadiness, TransactionId,
    encode_frame, encode_portal_clipboard_payload_frame,
};
use sophia_x_authority::*;

include!("authority/raster_fallback.rs");
include!("authority/resources.rs");
include!("authority/software_present.rs");
include!("authority/selection_and_codec.rs");
include!("authority/runtime_and_socket.rs");
include!("authority/support.rs");

#[cfg(unix)]
use std::net::Shutdown;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(all(unix, test))]
use std::sync::mpsc::channel;
#[cfg(unix)]
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError, sync_channel,
};
#[cfg(unix)]
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{ErrorKind, IoSlice, IoSliceMut, Read, Write},
    mem::MaybeUninit,
    num::NonZeroUsize,
    os::fd::{AsFd, OwnedFd},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(unix)]
use crate::{
    X_ATOM_NAME_NET_WM_STRUT, X_ATOM_NAME_NET_WM_STRUT_PARTIAL, X_ATOM_NAME_WM_DELETE_WINDOW,
    X_ATOM_NAME_WM_PROTOCOLS, X_SETUP_CLIENT_PREFIX_LEN, X_SETUP_DEFAULT_RESOURCE_ID_MASK,
    X_SETUP_DEFAULT_ROOT, X11DispatchObservation, X11ObservedDispatchFailure,
    X11ObservedRequestStage, XAtomTable, XAuthorityBackpressureFailure,
    XAuthorityBackpressureTelemetry, XAuthorityBackpressureTelemetryKind,
    XAuthorityClientControlAck, XAuthorityClientControlCommand, XAuthorityClientInputDelivery,
    XAuthorityClientInputEvent, XAuthorityClientMetadataCandidate, XAuthorityControlAck,
    XAuthorityControlCommand, XAuthorityControlOutcome, XAuthorityDri3FenceImport,
    XAuthorityDri3PixmapImport, XAuthorityInputDeliveryId, XAuthorityInputDeliveryOutcome,
    XAuthorityInputEvent, XAuthorityKeyEvent, XAuthorityObservedTransactionBatch,
    XAuthorityPointerEvent, XAuthorityPointerEventKind, XAuthorityPresentSubmission,
    XAuthorityResponsePacket, XAuthorityRouteLeaseRelease, XAuthorityRouteLeaseUpdate,
    XAuthorityRouteLeaseUpdateKind, XAuthorityRoutedInput, XAuthorityRoutedInputMode,
    XAuthorityRuntime, XAuthoritySurfaceRouteObservation, XByteOrder, XClientEvent,
    XDispatchContext, XDispatchResult, XPresentCompletionMode, XPropertyTable,
    XRasterFallbackCause, XResourceId, XServerFrontendAdmissionError,
    XServerFrontendAdmissionPolicy, XServerFrontendAdmissionRequest, XServerFrontendClientId,
    XServerFrontendConfig, XServerFrontendPeerCredentials, XServerFrontendPixmapAllocator,
    XServerFrontendRenderDeviceError, XServerFrontendRenderDeviceProvider,
    XServerFrontendRouteError, XServerFrontendServiceCommand, XServerFrontendSetupAuthorization,
    XSetupFailure, XSetupRequest, XSetupSuccess, XWireClientContext,
    apply_engine_presentation_state, decode_x11_core_request, dispatch_x11_parse_error,
    dispatch_x11_wire_request, encode_x_client_event, encode_x11_setup_failure,
    encode_x11_setup_success, parse_x11_setup_request, try_emit_x_authority_observation,
    x_output_reservations_for_window, x11_setup_request_total_len,
};
#[cfg(all(unix, test))]
use sophia_protocol::RoutedInputRequest;
#[cfg(unix)]
use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, InputEventKind, MetadataDisclosureRule, NamespaceId,
    Rect, SeatId, Size, SurfaceId, SurfaceOutputReservations, TransactionId,
};

include!("x11_socket/routing/broker.rs");
include!("x11_socket/routing/recovery.rs");
include!("x11_socket/routing/focus.rs");
include!("x11_socket/routing/registry.rs");
include!("x11_socket/routing/subscriptions.rs");
include!("x11_socket/routing/selection_subscriptions.rs");
include!("x11_socket/routing/keyboard.rs");
include!("x11_socket/routing/input.rs");
include!("x11_socket/frontend/service.rs");
include!("x11_socket/frontend/clipboard.rs");
include!("x11_socket/frontend/setup.rs");
include!("x11_socket/state.rs");
include!("x11_socket/device_bundles.rs");
include!("x11_socket/connection/raster_telemetry.rs");
include!("x11_socket/connection/server.rs");
include!("x11_socket/connection/protocol_routing.rs");
include!("x11_socket/connection/pixmap_publication.rs");
include!("x11_socket/connection/dispatch.rs");
include!("x11_socket/connection/event_state.rs");
include!("x11_socket/connection/focus.rs");
include!("x11_socket/connection/writers.rs");

#[cfg(unix)]
const X11_CLIENT_RESOURCE_RANGE_SIZE: u32 = X_SETUP_DEFAULT_RESOURCE_ID_MASK + 1;
#[cfg(unix)]
const X11_MAX_CLIENT_RESOURCE_RANGES: u16 = (u32::MAX / X11_CLIENT_RESOURCE_RANGE_SIZE) as u16;
#[cfg(unix)]
/// One ordered X11 socket write and the descriptors attached to its first byte.
///
/// Protocol dispatch remains byte-only and data-oriented. Native descriptor
/// ownership starts at this Unix-socket boundary and ends after the record is
/// sent or rejected, so descriptors cannot leak into authority runtime state.
#[cfg(unix)]
#[derive(Debug)]
pub struct X11SocketOutputRecord {
    bytes: Vec<u8>,
    fds: Vec<OwnedFd>,
}

#[cfg(unix)]
impl X11SocketOutputRecord {
    pub fn new(bytes: Vec<u8>, fds: Vec<OwnedFd>) -> Result<Self, X11SetupSocketError> {
        if bytes.is_empty() {
            return Err(X11SetupSocketError::new(
                "X11 socket output record cannot be empty",
            ));
        }
        if fds.len() > sophia_protocol::DMA_BUF_MAX_PLANES {
            return Err(X11SetupSocketError::new(format!(
                "X11 socket output record carried {} file descriptors; maximum is {}",
                fds.len(),
                sophia_protocol::DMA_BUF_MAX_PLANES,
            )));
        }
        Ok(Self { bytes, fds })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn fd_count(&self) -> usize {
        self.fds.len()
    }
}

#[cfg(unix)]
impl TryFrom<Vec<u8>> for X11SocketOutputRecord {
    type Error = X11SetupSocketError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(bytes, Vec::new())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct X11SetupSocketError {
    message: String,
    client_disconnect: bool,
    client_failure: bool,
    service_shutdown: bool,
}

impl X11SetupSocketError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            client_disconnect: false,
            client_failure: false,
            service_shutdown: false,
        }
    }

    fn client_disconnect(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            client_disconnect: true,
            client_failure: false,
            service_shutdown: false,
        }
    }

    fn client_failure(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            client_disconnect: false,
            client_failure: true,
            service_shutdown: false,
        }
    }

    fn service_shutdown(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            client_disconnect: false,
            client_failure: false,
            service_shutdown: true,
        }
    }

    fn with_cleanup_failures(mut self, failures: Vec<String>) -> Self {
        if !failures.is_empty() {
            self.message.push_str("; frontend cleanup failures: ");
            self.message.push_str(&failures.join("; "));
        }
        self
    }
}

impl core::fmt::Display for X11SetupSocketError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for X11SetupSocketError {}

/// The setup allocation retained for one connected X11 client.
///
/// The range is a connection lease, not a namespace boundary: in a classic
/// shared-X session other trusted clients may still reference a resource after
/// its creator made it. It is retained so disconnect cleanup can reclaim only
/// resources whose XIDs this client was allowed to create.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XServerFrontendClientLease {
    client: XServerFrontendClientId,
    resource_id_range: crate::XWireClientResourceRange,
}

#[cfg(all(unix, target_os = "linux"))]
fn x11_peer_credentials(
    stream: &UnixStream,
) -> Result<Option<XServerFrontendPeerCredentials>, X11SetupSocketError> {
    let credentials = rustix::net::sockopt::socket_peercred(stream).map_err(|error| {
        X11SetupSocketError::new(format!("failed to read X11 peer credentials: {error}"))
    })?;
    let process_id = u32::try_from(credentials.pid.as_raw_pid()).map_err(|_| {
        X11SetupSocketError::new("X11 peer process ID is outside the supported range")
    })?;
    Ok(Some(XServerFrontendPeerCredentials {
        process_start_time: sophia_linux_peer::socket_peer_start_time(
            std::os::fd::AsFd::as_fd(stream),
            process_id,
        )
        .ok(),
        process_id,
        user_id: credentials.uid.as_raw(),
        group_id: credentials.gid.as_raw(),
    }))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn x11_peer_credentials(
    _stream: &UnixStream,
) -> Result<Option<XServerFrontendPeerCredentials>, X11SetupSocketError> {
    Ok(None)
}

#[cfg(unix)]
struct XServerFrontendAdmissionLease {
    policy: Arc<dyn XServerFrontendAdmissionPolicy>,
    context: Option<ClientAdmissionContext>,
}

#[cfg(unix)]
impl XServerFrontendAdmissionLease {
    fn new(
        policy: Arc<dyn XServerFrontendAdmissionPolicy>,
        context: ClientAdmissionContext,
    ) -> Self {
        Self {
            policy,
            context: Some(context),
        }
    }

    fn context(&self) -> ClientAdmissionContext {
        self.context
            .expect("active X11 admission lease must retain its context")
    }

    fn revoke(&mut self) -> Result<(), XServerFrontendAdmissionError> {
        let Some(context) = self.context.take() else {
            return Ok(());
        };
        self.policy.revoke(context)
    }
}

#[cfg(unix)]
impl Drop for XServerFrontendAdmissionLease {
    fn drop(&mut self) {
        let _ = self.revoke();
    }
}

/// A trace callback used by a bounded concurrent frontend worker.
/// Returns a receipt only after that authority observation has been enqueued.
/// Diagnostic-only observers return `None`, as do requests producing no batch.
#[cfg(unix)]
pub type X11CoreTraceObserver = dyn Fn(X11DispatchObservation) -> Result<Option<TransactionId>, X11SetupSocketError>
    + Send
    + Sync
    + 'static;

/// Receives value-free production backpressure transitions from routed workers.
#[cfg(unix)]
pub type XAuthorityBackpressureObserver =
    dyn Fn(XAuthorityBackpressureTelemetry) + Send + Sync + 'static;

#[cfg(unix)]
struct X11CoreClientWorkerCompletion {
    worker_id: u64,
    result: Result<(), X11SetupSocketError>,
}

#[cfg(unix)]
#[derive(Debug)]
struct X11CoreClientWorker {
    thread: std::thread::JoinHandle<()>,
    shutdown: UnixStream,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct X11CoreClientWorkerAdmission {
    worker_id: u64,
    admission: ClientAdmissionId,
}

fn is_x11_client_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof
    )
}

// Only transport errors identifying a departed peer are connection-local.
// Locks, codecs and shared authority failures must retain their fatal class.
fn x11_peer_write_error(context: &str, error: std::io::Error) -> X11SetupSocketError {
    let message = format!("{context}: {error}");
    if is_x11_client_disconnect(&error) {
        X11SetupSocketError::client_disconnect(message)
    } else {
        X11SetupSocketError::new(message)
    }
}

#[path = "x11_socket/tests.rs"]
mod routing_tests;
include!("x11_socket/connection/observations.rs");
include!("x11_socket/connection/io.rs");
#[path = "../tests/support/present_layout_comparison.rs"]
mod present_layout_comparison_tests;

#[cfg(all(test, unix))]
#[path = "../tests/support/present_progress.rs"]
mod present_progress_tests;

#[cfg(all(test, unix))]
#[path = "x11_socket/tests/input_recovery.rs"]
mod input_recovery_tests;

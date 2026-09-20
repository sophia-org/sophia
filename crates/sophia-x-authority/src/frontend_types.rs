use std::os::fd::OwnedFd;

use sophia_input_authority::InstanceId;
use sophia_protocol::{
    BufferHandle, ClientAdmissionContext, ClientAuthenticationMethod, DmaBufDescriptor, Rect, Size,
};

/// Monotonically assigned identity for one live X11 client connection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct XServerFrontendClientId(pub(crate) u64);

impl XServerFrontendClientId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Eq, PartialEq, Default)]
pub enum XServerFrontendSetupAuthorization {
    #[default]
    UnauthenticatedLocal,
    MitMagicCookie([u8; 16]),
    /// Private-instance mixed mode. Empty/empty setup remains ordinary local
    /// access. Any supplied authorization must match this instance's cookie
    /// under SOPHIA-PRIVATE-INPUT-1; a wrong or partial credential is refused.
    /// Verification supplies admission evidence, not an input grant.
    PrivateInputCookie {
        instance: InstanceId,
        cookie: [u8; 32],
    },
}

impl core::fmt::Debug for XServerFrontendSetupAuthorization {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnauthenticatedLocal => formatter.write_str("UnauthenticatedLocal"),
            Self::MitMagicCookie(_) => formatter.write_str("MitMagicCookie([redacted])"),
            Self::PrivateInputCookie { instance, .. } => formatter
                .debug_struct("PrivateInputCookie")
                .field("instance", instance)
                .field("cookie", &"[redacted]")
                .finish(),
        }
    }
}

impl XServerFrontendSetupAuthorization {
    pub(crate) fn permits(&self, request: &crate::XSetupRequest) -> bool {
        self.verified_authentication(request).is_some()
    }

    pub(crate) fn verified_authentication(
        &self,
        request: &crate::XSetupRequest,
    ) -> Option<(
        ClientAuthenticationMethod,
        Option<XServerFrontendVerifiedPrivateInputAuthorization>,
    )> {
        let name = &request.authorization_protocol_name;
        let data = &request.authorization_data;
        match self {
            Self::UnauthenticatedLocal => Some((ClientAuthenticationMethod::TrustedLocal, None)),
            Self::MitMagicCookie(expected) => (name == b"MIT-MAGIC-COOKIE-1"
                && authorization_data_eq(data, expected))
            .then_some((ClientAuthenticationMethod::MitMagicCookie1, None)),
            Self::PrivateInputCookie { .. } if name.is_empty() && data.is_empty() => {
                Some((ClientAuthenticationMethod::TrustedLocal, None))
            }
            Self::PrivateInputCookie { instance, cookie } => (name == b"SOPHIA-PRIVATE-INPUT-1"
                && authorization_data_eq(data, cookie))
            .then_some((
                ClientAuthenticationMethod::TrustedLocal,
                Some(XServerFrontendVerifiedPrivateInputAuthorization {
                    instance: *instance,
                }),
            )),
        }
    }
}

/// Sanitized evidence produced only after this frontend verified the named
/// private cookie. Contains no credential or caller-supplied instance identity.
/// InstanceId alone is a name, not authority. The admission policy must bind
/// any later grant to this currently admitted connection and its own instance;
/// copying this observation does not delegate authorization to another peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XServerFrontendVerifiedPrivateInputAuthorization {
    instance: InstanceId,
}

impl XServerFrontendVerifiedPrivateInputAuthorization {
    pub const fn instance(self) -> InstanceId {
        self.instance
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct XServerFrontendPeerCredentials {
    /// Optional provenance evidence; unavailable kernels keep ordinary placement.
    pub process_start_time: Option<u64>,
    pub process_id: u32,
    pub user_id: u32,
    pub group_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XServerFrontendAdmissionRequest {
    pub peer_credentials: Option<XServerFrontendPeerCredentials>,
    /// Existing transport provenance. TrustedLocal in mixed private mode is
    /// not an input grant; only verified_private_input carries named evidence.
    pub setup_authentication: ClientAuthenticationMethod,
    pub verified_private_input: Option<XServerFrontendVerifiedPrivateInputAuthorization>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XServerFrontendAdmissionError {
    Denied,
    Unavailable,
}

impl core::fmt::Display for XServerFrontendAdmissionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Denied => formatter.write_str("X11 client admission denied"),
            Self::Unavailable => formatter.write_str("X11 client admission unavailable"),
        }
    }
}

impl std::error::Error for XServerFrontendAdmissionError {}

pub trait XServerFrontendAdmissionPolicy: Send + Sync + 'static {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError>;

    fn revoke(&self, context: ClientAdmissionContext) -> Result<(), XServerFrontendAdmissionError>;
}

/// Why a connection was not given the means to inject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XServerFrontendInjectionError {
    /// The instance issues no injection at all. The ordinary answer: XTEST
    /// exists in the protocol and not in this instance.
    Unavailable,
    /// This admission exists and may not inject. Distinguished from the above
    /// because one describes the server and the other describes the client,
    /// and a record that collapsed them could not say which.
    Denied,
}

impl core::fmt::Display for XServerFrontendInjectionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("X11 synthetic input unavailable"),
            Self::Denied => formatter.write_str("X11 synthetic input denied"),
        }
    }
}

impl std::error::Error for XServerFrontendInjectionError {}

/// What an admitted connection may do to the seat, and nothing else.
///
/// Deliberately not a handle to the authority. An adapter holds this and has
/// no expression for issuing itself a grant, registering a device or naming
/// another connection's work: those belong to whoever issued this.
pub trait XTestInjector: Send + 'static {
    /// Install the slot this connection parks on.
    ///
    /// Once, and before the first submission: a slot installed after a
    /// request exists could be armed too late to catch that request's
    /// completion, and the client would wait for an answer that already went
    /// nowhere. Reports false if one is already installed.
    fn report_completions_to(&self, barrier: crate::PrivateRequestBarrier) -> bool;
}

/// Who decides whether a connection may inject, and issues the means.
///
/// Decided once, at setup, for the same reason discovery and every request
/// path share one decision: a client refused injection must find XTEST absent
/// from QueryExtension and ListExtensions, and two decisions in two places
/// could disagree. The disagreement would be a client that can see an
/// extension it may not use.
pub trait XServerFrontendInjectionPolicy: Send + Sync + 'static {
    fn issue(
        &self,
        context: ClientAdmissionContext,
        device: sophia_protocol::DeviceId,
    ) -> Result<Box<dyn XTestInjector>, XServerFrontendInjectionError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XServerFrontendRenderDeviceError {
    Unavailable,
    OpenFailed,
}

impl core::fmt::Display for XServerFrontendRenderDeviceError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("X11 render device is unavailable"),
            Self::OpenFailed => formatter.write_str("X11 render device open failed"),
        }
    }
}

impl std::error::Error for XServerFrontendRenderDeviceError {}

/// Cached explicit import layouts measured on the provider's fixed device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XServerFrontendDmaBufImportFormat {
    pub format: u32,
    pub modifiers: Vec<u64>,
}

/// Exact render-node identity after the backend has normalized card to render.
/// Equality is conservative evidence; a device number alone is not sufficient.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XRenderDeviceIdentity {
    pub device: u64,
    pub inode: u64,
    pub device_number: u64,
}

pub trait XServerFrontendRenderDeviceProvider: Send + Sync + 'static {
    /// Captured during bundle construction, outside authority locks.
    fn render_device_identity(&self) -> Option<XRenderDeviceIdentity> {
        None
    }

    fn open_render_device_fd(&self) -> Result<OwnedFd, XServerFrontendRenderDeviceError>;

    /// Returns cached measurements; this callback must not perform GPU work.
    fn dma_buf_import_formats(&self) -> Vec<XServerFrontendDmaBufImportFormat> {
        Vec::new()
    }
}

/// The extent and depth a pixmap needs backing at, and the identity to stamp it
/// with.
///
/// The handle is issued here rather than by the allocator. Buffer identity is
/// one space shared with every buffer a client imports, and an allocator
/// counting from one of its own would hand out a name another buffer already
/// answers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XServerFrontendPixmapAllocation {
    pub size: Size,
    pub depth: u8,
    pub handle: u64,
}

/// A buffer allocated for a client, and the descriptors that reach it.
#[derive(Debug)]
pub struct XServerFrontendAllocatedPixmap {
    pub descriptor: DmaBufDescriptor,
    /// One per plane, in plane order, and the same count the descriptor states.
    pub plane_fds: Vec<OwnedFd>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XServerFrontendPixmapAllocationError {
    /// No allocator is configured. A frontend without one still serves the
    /// client-allocated path; it simply cannot originate a buffer.
    Unavailable,
    /// The extent or depth is not one the allocator can back.
    UnsupportedTarget,
    /// The device refused the allocation or its descriptors could not be
    /// exported.
    AllocationFailed,
    /// The handle names no backing this provider owns. An imported client
    /// buffer never enters the store, so naming one here is a mistake to
    /// report rather than a write to absorb.
    UnknownBacking,
}

impl core::fmt::Display for XServerFrontendPixmapAllocationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("X11 pixmap allocator is unavailable"),
            Self::UnsupportedTarget => {
                formatter.write_str("X11 pixmap allocation target is unsupported")
            }
            Self::AllocationFailed => formatter.write_str("X11 pixmap allocation failed"),
            Self::UnknownBacking => {
                formatter.write_str("X11 pixmap backing is not owned by the allocator")
            }
        }
    }
}

impl std::error::Error for XServerFrontendPixmapAllocationError {}

/// Originates a buffer for a pixmap the client did not allocate.
///
/// DRI3 has two halves. In one the client allocates and the authority wraps what
/// it is handed; in the other the client expects the server to own the storage
/// and asks for its descriptors back. This is the second half, kept as a request
/// the authority makes rather than a capability it holds: the authority owns no
/// device, no allocator state, and no renderer handle, exactly as it owns none
/// for the render device it hands to `DRI3 Open`.
pub trait XServerFrontendPixmapAllocator: Send + Sync + 'static {
    fn allocate_pixmap_buffer(
        &self,
        request: XServerFrontendPixmapAllocation,
    ) -> Result<XServerFrontendAllocatedPixmap, XServerFrontendPixmapAllocationError>;

    /// Whether this provider keeps pixmap backings a GL client can sample.
    ///
    /// Immutable for the life of the frontend. `GetFBConfigs` and
    /// `QueryExtensionsString` are answered once per client, so a value that
    /// changed mid-session would leave clients holding configurations the
    /// server no longer honours.
    fn supports_pixmap_textures(&self) -> bool {
        false
    }

    /// Publishes pixels into a backing this provider owns.
    ///
    /// Revisions are monotonic per handle: a provider that has already
    /// published a newer one reports success without rewriting, so a late or
    /// duplicated update is absorbed rather than retried.
    fn update_pixmap_buffer(
        &self,
        request: XServerFrontendPixmapUpdate,
    ) -> Result<(), XServerFrontendPixmapAllocationError> {
        let _ = request;
        Err(XServerFrontendPixmapAllocationError::Unavailable)
    }

    /// Drops a backing this provider owns.
    ///
    /// Only provider-owned backings reach here; an imported client buffer is
    /// not the provider's to free.
    fn release_pixmap_buffer(
        &self,
        handle: BufferHandle,
    ) -> Result<(), XServerFrontendPixmapAllocationError> {
        let _ = handle;
        Err(XServerFrontendPixmapAllocationError::Unavailable)
    }
}

/// One tightly packed rectangle of a pixmap's pixels.
///
/// `bytes` carries `rect.height` rows of `rect.width` pixels with nothing
/// between them, so a patch has no stride of its own and cannot disagree with
/// the backing about padding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XServerFrontendPixmapPatch {
    pub rect: Rect,
    pub bytes: Vec<u8>,
}

/// Pixels to publish into a provider-owned pixmap backing.
///
/// Bounded by the limits the CPU patch path already enforces:
/// `X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS` rectangles and
/// `X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES` in total.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XServerFrontendPixmapUpdate {
    pub handle: BufferHandle,
    pub revision: u64,
    pub size: Size,
    pub format: u32,
    pub patches: Vec<XServerFrontendPixmapPatch>,
}

fn authorization_data_eq(actual: &[u8], expected: &[u8]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    actual
        .iter()
        .zip(expected)
        .fold(0u8, |difference, (actual, expected)| {
            difference | (actual ^ expected)
        })
        == 0
}

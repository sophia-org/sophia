use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sophia_protocol::{NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile};

use crate::{
    X11SetupSocketError, XServerFrontendAdmissionPolicy, XServerFrontendPixmapAllocator,
    XServerFrontendRenderDeviceProvider, XServerFrontendSetupAuthorization,
};

const DEFAULT_MAX_CONCURRENT_CLIENTS: NonZeroUsize = match NonZeroUsize::new(16) {
    Some(value) => value,
    None => unreachable!(),
};

#[derive(Clone)]
pub struct XServerFrontendConfig {
    socket_path: PathBuf,
    namespace: NamespaceContext,
    setup_authorization: XServerFrontendSetupAuthorization,
    admission_policy: Option<Arc<dyn XServerFrontendAdmissionPolicy>>,
    render_device_provider: Option<Arc<dyn XServerFrontendRenderDeviceProvider>>,
    pixmap_allocator: Option<Arc<dyn XServerFrontendPixmapAllocator>>,
    device_bundle: Option<Arc<crate::XServerFrontendDeviceBundle>>,
    max_concurrent_clients: NonZeroUsize,
    output_topology: sophia_protocol::OutputTopologySnapshot,
    xkb_config: crate::XkbRmlvoConfig,
    defer_policy_maps: bool,
    /// Directories searched for core fonts, before the built-in element.
    ///
    /// Session configuration, never client configuration: `SetFontPath` is
    /// refused, so nothing a client sends can add a directory here.
    font_path: Vec<PathBuf>,
}

impl core::fmt::Debug for XServerFrontendConfig {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("XServerFrontendConfig")
            .field("socket_path", &self.socket_path)
            .field("namespace", &self.namespace)
            .field("setup_authorization", &self.setup_authorization)
            .field("has_admission_policy", &self.admission_policy.is_some())
            .field(
                "has_render_device_provider",
                &self.render_device_provider.is_some(),
            )
            .field("has_pixmap_allocator", &self.pixmap_allocator.is_some())
            .field("max_concurrent_clients", &self.max_concurrent_clients)
            .field("output_topology", &self.output_topology)
            .field("xkb_config", &self.xkb_config)
            .field("defer_policy_maps", &self.defer_policy_maps)
            .field("font_path", &self.font_path)
            .finish()
    }
}

impl XServerFrontendConfig {
    pub fn new(
        socket_path: impl Into<PathBuf>,
        namespace: NamespaceId,
    ) -> Result<Self, X11SetupSocketError> {
        let namespace = NamespaceContext::new(
            namespace,
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .ok_or_else(|| {
            X11SetupSocketError::new("Sophia X Server Frontend namespace must be valid")
        })?;
        Self::new_with_namespace_context(socket_path, namespace)
    }

    pub fn new_with_namespace_context(
        socket_path: impl Into<PathBuf>,
        namespace: NamespaceContext,
    ) -> Result<Self, X11SetupSocketError> {
        let socket_path = socket_path.into();
        if socket_path.as_os_str().is_empty() {
            return Err(X11SetupSocketError::new(
                "Sophia X Server Frontend socket path must not be empty",
            ));
        }
        if !namespace.is_valid() {
            return Err(X11SetupSocketError::new(
                "Sophia X Server Frontend namespace must be valid",
            ));
        }
        Ok(Self {
            socket_path,
            namespace,
            setup_authorization: XServerFrontendSetupAuthorization::default(),
            admission_policy: None,
            render_device_provider: None,
            pixmap_allocator: None,
            device_bundle: None,
            max_concurrent_clients: DEFAULT_MAX_CONCURRENT_CLIENTS,
            output_topology: sophia_protocol::OutputTopologySnapshot::deterministic(),
            xkb_config: crate::XkbRmlvoConfig::default(),
            defer_policy_maps: false,
            // No directories by default. A frontend serves text from the
            // built-in element until a session configures a path, which keeps
            // every test and probe independent of what the host has installed.
            font_path: Vec::new(),
        })
    }

    pub fn with_setup_authorization(
        mut self,
        setup_authorization: XServerFrontendSetupAuthorization,
    ) -> Self {
        self.setup_authorization = setup_authorization;
        self
    }

    pub fn with_admission_policy(
        mut self,
        admission_policy: Arc<dyn XServerFrontendAdmissionPolicy>,
    ) -> Self {
        self.admission_policy = Some(admission_policy);
        self
    }

    pub fn with_render_device_provider(
        mut self,
        provider: Arc<dyn XServerFrontendRenderDeviceProvider>,
    ) -> Self {
        self.render_device_provider = Some(provider);
        self
    }

    pub fn with_pixmap_allocator(
        mut self,
        allocator: Arc<dyn XServerFrontendPixmapAllocator>,
    ) -> Self {
        self.pixmap_allocator = Some(allocator);
        self
    }

    pub fn with_device_bundle(mut self, bundle: Arc<crate::XServerFrontendDeviceBundle>) -> Self {
        self.device_bundle = Some(bundle);
        self
    }

    pub(crate) fn device_bundle(&self) -> Option<Arc<crate::XServerFrontendDeviceBundle>> {
        self.device_bundle.clone()
    }

    pub fn with_max_concurrent_clients(mut self, max_concurrent_clients: NonZeroUsize) -> Self {
        self.max_concurrent_clients = max_concurrent_clients;
        self
    }

    pub fn with_output_topology(
        mut self,
        output_topology: sophia_protocol::OutputTopologySnapshot,
    ) -> Result<Self, X11SetupSocketError> {
        output_topology.validate().map_err(|error| {
            X11SetupSocketError::new(format!("invalid Engine output topology: {error:?}"))
        })?;
        self.output_topology = output_topology;
        Ok(self)
    }

    pub fn output_topology(&self) -> &sophia_protocol::OutputTopologySnapshot {
        &self.output_topology
    }

    pub fn with_xkb_config(
        mut self,
        xkb_config: crate::XkbRmlvoConfig,
    ) -> Result<Self, X11SetupSocketError> {
        xkb_config.validate().map_err(|error| {
            X11SetupSocketError::new(format!("invalid XKB configuration: {error}"))
        })?;
        self.xkb_config = xkb_config;
        Ok(self)
    }

    pub const fn xkb_config(&self) -> &crate::XkbRmlvoConfig {
        &self.xkb_config
    }

    pub fn with_policy_map_deferred(mut self, deferred: bool) -> Self {
        self.defer_policy_maps = deferred;
        self
    }

    /// Replace the font path this frontend serves.
    ///
    /// An empty path is not an error: the built-in element always answers, so
    /// a session configured with no directories still renders text.
    #[must_use]
    pub fn with_font_path(mut self, font_path: Vec<PathBuf>) -> Self {
        self.font_path = font_path;
        self
    }

    pub fn font_path(&self) -> &[PathBuf] {
        &self.font_path
    }

    pub const fn policy_map_deferred(&self) -> bool {
        self.defer_policy_maps
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub const fn namespace(&self) -> NamespaceId {
        self.namespace.id
    }

    pub const fn namespace_context(&self) -> NamespaceContext {
        self.namespace
    }

    pub const fn setup_authorization(&self) -> &XServerFrontendSetupAuthorization {
        &self.setup_authorization
    }

    pub(crate) fn admission_policy(&self) -> Option<Arc<dyn XServerFrontendAdmissionPolicy>> {
        self.admission_policy.clone()
    }

    pub(crate) fn render_device_provider(
        &self,
    ) -> Option<Arc<dyn XServerFrontendRenderDeviceProvider>> {
        self.render_device_provider.clone()
    }

    pub(crate) fn pixmap_allocator(&self) -> Option<Arc<dyn XServerFrontendPixmapAllocator>> {
        self.pixmap_allocator.clone()
    }

    pub const fn max_concurrent_clients(&self) -> NonZeroUsize {
        self.max_concurrent_clients
    }
}

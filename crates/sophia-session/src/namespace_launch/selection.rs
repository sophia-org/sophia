//! Trusted selection and resource binding shared by every application launcher.
//! Load and capture on startup/configuration workers; resolution uses those
//! immutable inputs. Preparation rechecks the group and socket before spawn.
use super::*;
use crate::application_catalog::ApplicationLaunchCommand;
use sophia_config::namespace_policy::{NamespacePolicyConfig, RecipePortalCapability};
use sophia_portal::recipient_object::PortalObjectRecipient;
use sophia_protocol::{NamespaceContext, NamespacePortalCapability, NamespaceProfile};
use sophia_runtime::{ConfinedXApplicationLaunch, ProtectionDomainSpec};
use std::{io, path::Path};

pub struct ApplicationRecipeSet {
    policy: NamespacePolicyConfig,
    recipes: BTreeMap<String, Arc<LoadedNamespaceRecipe>>,
}
impl ApplicationRecipeSet {
    pub fn load(policy: &NamespacePolicyConfig, root: &Path) -> io::Result<Self> {
        let recipes = policy
            .recipes
            .iter()
            .map(|(name, recipe)| {
                Ok((
                    name.clone(),
                    Arc::new(LoadedNamespaceRecipe::load(recipe, root)?),
                ))
            })
            .collect::<io::Result<_>>()?;
        Ok(Self {
            policy: policy.clone(),
            recipes,
        })
    }

    /// None means that trusted policy did not select a recipe. A refused
    /// metadata request is an error, never permission for the legacy launcher.
    pub fn application(
        &self,
        identity: &str,
        command: &ApplicationLaunchCommand,
    ) -> Result<Option<Arc<LoadedNamespaceRecipe>>, String> {
        self.policy
            .select_application(identity, command.namespace_recipe.as_deref())
            .map_err(|e| e.to_string())?
            .map(|recipe| self.loaded(&recipe.name))
            .transpose()
    }

    /// The caller must separately hold administrative launch admission.
    /// This checks policy permission only; the recipe name is not a capability.
    pub fn developer(&self, name: &str) -> Result<Arc<LoadedNamespaceRecipe>, String> {
        let recipe = self
            .policy
            .select_developer(name)
            .map_err(|e| e.to_string())?;
        self.loaded(&recipe.name)
    }
    fn loaded(&self, name: &str) -> Result<Arc<LoadedNamespaceRecipe>, String> {
        self.recipes
            .get(name)
            .cloned()
            .ok_or_else(|| "selected recipe absent".into())
    }
}

/// Captured from this Session's already-bound listener and cookie. Group None
/// denotes shared X; a numbered group denotes the exact configured confinement.
pub struct RecipeXTarget {
    paths: ConfinedXApplicationLaunch,
    context: NamespaceContext,
    group: Option<u32>,
    socket: CapturedX11Socket,
    display: u32,
    cookie: PathBuf,
    domain: ProtectionDomainSpec,
}
impl RecipeXTarget {
    pub fn capture(
        context: NamespaceContext,
        group: Option<u32>,
        service: sophia_protocol::namespace_view::ServiceReference,
        paths: &ConfinedXApplicationLaunch,
    ) -> Result<Self, String> {
        if group == Some(0)
            || context.profile
                != if group.is_some() {
                    NamespaceProfile::Confined
                } else {
                    NamespaceProfile::ClassicShared
                }
        {
            return Err("X group and namespace profiles differ".into());
        }
        let spec = paths.namespace_client_spec().map_err(|e| e.to_string())?;
        let socket = CapturedX11Socket::capture(
            service,
            &paths
                .group_directory
                .join(format!("X{}", paths.display_number)),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            paths: paths.clone(),
            context,
            group,
            socket,
            display: paths.display_number,
            cookie: paths.cookie_file.clone(),
            domain: spec.protection_domain.expect("X domain"),
        })
    }
    pub(super) fn verify(&self) -> Result<(), String> {
        self.paths
            .namespace_client_spec()
            .map_err(|e| e.to_string())?;
        if !self
            .socket
            .matches_source(
                &self
                    .paths
                    .group_directory
                    .join(format!("X{}", self.display)),
            )
            .map_err(|e| e.to_string())?
        {
            return Err("captured X listener changed before launch".into());
        }
        Ok(())
    }
}

/// Service identities and owners come from Session, never application metadata.
/// Namespace capability checks do not approve an individual portal transfer.
pub struct RecipeLaunchTarget {
    pub context: NamespaceContext,
    pub principal: u64,
    pub x11: Option<Arc<RecipeXTarget>>,
    pub inspection: Option<(
        ApprovedEndpoint,
        sophia_runtime::inspection::InspectionAcceptor,
    )>,
    pub portal: (ApprovedEndpoint, Arc<PortalObjectRecipient>),
    pub bubblewrap: PathBuf,
    pub host_control: Option<PathBuf>,
    pub host_inspection: Option<PathBuf>,
}

pub struct ResolvedApplicationLaunch {
    request: ClientLaunchRequest,
    services: NamespaceApplicationServices,
}
impl ResolvedApplicationLaunch {
    pub fn start(
        mut self,
        service: &NamespaceLaunchService,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
        diagnostic: crate::diagnostics::application::LaunchContext,
    ) -> Result<ApplicationProcess, String> {
        self.request.diagnostic =
            crate::diagnostics::application::prepare(&self.request.launch.program, diagnostic);
        Ok(ApplicationProcess::namespace(
            service.start(self.request, launches, wake, now)?,
            self.services,
        ))
    }
}

impl RecipeLaunchTarget {
    pub fn resolve(
        &self,
        recipe: Arc<LoadedNamespaceRecipe>,
        command: &ApplicationLaunchCommand,
    ) -> Result<ResolvedApplicationLaunch, String> {
        if self.principal == 0
            || !self.context.id.is_valid()
            || command
                .namespace_recipe
                .as_deref()
                .is_some_and(|n| n != recipe.policy().name)
        {
            return Err("application recipe identity mismatch".into());
        }
        let policy = recipe.policy();
        if policy
            .request
            .iter()
            .any(|c| !self.context.capabilities.allows_request(capability(*c)))
            || policy
                .publish
                .iter()
                .any(|c| !self.context.capabilities.allows_publish(capability(*c)))
        {
            return Err("recipe participation exceeds its namespace".into());
        }
        let (portal, objects) = &self.portal;
        if objects.namespace() != self.context.id
            || !portal.service.is_valid()
            || portal.service.protocol != EndpointProtocol::NineP2000L
            || portal.service.interface != "portal-client"
            || portal.service.version != 1
        {
            return Err("recipe portal owner mismatch".into());
        }
        let mut approved = BTreeMap::from([("portal".into(), portal.clone())]);
        let mut services = NamespaceApplicationServices {
            inspection: None,
            portal: Some((portal.service.clone(), objects.clone())),
        };
        let mut x11 = Vec::new();
        let mut launch = command.arguments.iter().fold(
            ProcessLaunchSpec::new(&command.executable),
            |launch, arg| launch.arg(arg),
        );
        match policy.profile {
            NamespaceRecipeProfile::SharedX | NamespaceRecipeProfile::ConfinedX { .. } => {
                let target = self.x11.as_ref().ok_or("recipe X group absent")?;
                let group = match policy.profile {
                    NamespaceRecipeProfile::ConfinedX { group } => Some(group),
                    _ => None,
                };
                if target.context != self.context
                    || target.group != group
                    || self.inspection.is_some()
                {
                    return Err("recipe X group mismatch".into());
                }
                if group.is_some() {
                    launch = launch
                        .protection_domain(target.domain.clone())
                        .env("XAUTHORITY", sophia_runtime::CONFINED_X_AUTHORITY_FILE);
                } else {
                    launch = launch.env("XAUTHORITY", &target.cookie);
                    for (name, path) in [
                        (
                            sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV,
                            &self.host_control,
                        ),
                        (
                            sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV,
                            &self.host_inspection,
                        ),
                    ] {
                        launch = launch.env_remove(name);
                        if let Some(path) = path {
                            launch = launch.env(name, path);
                        }
                    }
                }
                launch = launch.env("DISPLAY", format!(":{}", target.display));
                approved.insert(
                    "x-group".into(),
                    ApprovedEndpoint {
                        service: target.socket.service().clone(),
                        audience: portal.audience,
                        availability:
                            sophia_protocol::namespace_view::ServiceAvailability::Available,
                    },
                );
                x11.push(target.socket.try_clone().map_err(|e| e.to_string())?);
            }
            NamespaceRecipeProfile::Observer => {
                let (endpoint, owner) = self
                    .inspection
                    .as_ref()
                    .ok_or("observer inspection owner absent")?;
                if self.x11.is_some()
                    || self.context.profile != NamespaceProfile::Confined
                    || self.context.capabilities.publish_bits() != 0
                    || self.context.capabilities.request_bits()
                        & !sophia_protocol::NamespaceCapabilities::NONE
                            .with_request(NamespacePortalCapability::ScreenCapture)
                            .request_bits()
                        != 0
                    || !endpoint.service.is_valid()
                    || endpoint.service.protocol != EndpointProtocol::NineP2000L
                    || endpoint.service.interface != "inspection"
                    || endpoint.service.version != 1
                    || endpoint.service.generation != owner.generation()
                    || !owner.is_live()
                {
                    return Err("observer service owner mismatch".into());
                }
                launch = launch.protection_domain(
                    ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
                        .map_err(|e| e.to_string())?
                        .bubblewrap_path(&self.bubblewrap),
                );
                approved.insert("inspection".into(), endpoint.clone());
                services.inspection = Some((endpoint.service.clone(), owner.clone()));
            }
        }
        launch = launch
            .process_group()
            .stdin_null()
            .env_remove("ENV")
            .env_remove("BASH_ENV");
        if let Some(directory) = &command.working_directory {
            launch = launch.current_dir(directory);
        }
        Ok(ResolvedApplicationLaunch {
            request: ClientLaunchRequest {
                x11_target: self.x11.clone(),
                diagnostic: None,
                launch,
                recipe,
                namespace: self.context.id,
                principal: self.principal,
                approved,
                x11,
            },
            services,
        })
    }
}

#[path = "../../tests/support/namespace_application_selection.rs"]
mod tests;

fn capability(value: RecipePortalCapability) -> NamespacePortalCapability {
    match value {
        RecipePortalCapability::Clipboard => NamespacePortalCapability::Clipboard,
        RecipePortalCapability::DragAndDrop => NamespacePortalCapability::DragAndDrop,
        RecipePortalCapability::FileHandoff => NamespacePortalCapability::FileHandoff,
        RecipePortalCapability::ScreenCapture => NamespacePortalCapability::ScreenCapture,
        RecipePortalCapability::ScreenRecording => NamespacePortalCapability::ScreenRecording,
        RecipePortalCapability::UriOpen => NamespacePortalCapability::UriOpen,
        RecipePortalCapability::Notification => NamespacePortalCapability::Notification,
    }
}

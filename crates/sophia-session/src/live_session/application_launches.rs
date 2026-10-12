//! One Session policy/resource owner for catalog, shortcut and startup launches.
use super::*;
use crate::application_catalog::{ApplicationLaunchCommand, CatalogProcessEnvironment};
use crate::diagnostics::application::LaunchContext;
use crate::namespace_launch::{
    ApplicationProcess, ApplicationRecipeSet, NamespaceLaunchService, RecipeLaunchTarget,
    RecipeXTarget,
};
use crate::namespace_views::ApprovedEndpoint;
use sophia_config::namespace_policy::{NamespacePolicyConfig, NamespaceRecipeProfile};
use sophia_protocol::namespace_view::{EndpointProtocol, ServiceAvailability, ServiceReference};
use std::{io, path::Path};

#[derive(Default)]
pub(super) struct LiveApplicationLaunches {
    prepared: Option<Prepared>,
}
struct Prepared {
    policy: NamespacePolicyConfig,
    recipes: ApplicationRecipeSet,
    service: NamespaceLaunchService,
    targets: BTreeMap<String, RecipeLaunchTarget>,
    // Fixed desktop roles use small Session-owned principals. Application
    // identities occupy a separate range and are burned even on failed launch.
    next_principal: u64,
}

pub(super) struct ApplicationLaunchTurn<'a> {
    pub applications: &'a mut LiveApplicationLaunches,
    pub authentication: &'a mut crate::launch_authentication::LaunchCoordinator,
    pub wake: sophia_wake::Notifier,
}

pub(super) fn validate_configuration(
    policy: &NamespacePolicyConfig,
    normal: bool,
    factotum: bool,
    wm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if policy.recipes.is_empty() {
        return Ok(());
    }
    if !normal || !factotum {
        return Err(
            "namespace recipes require a normal Session with Factotum launch authentication".into(),
        );
    }
    if !wm
        && policy
            .recipes
            .values()
            .any(|recipe| recipe.profile == NamespaceRecipeProfile::Observer)
    {
        return Err("observer recipes require the WM inspection publication owner".into());
    }
    Ok(())
}

impl LiveApplicationLaunches {
    pub(super) fn refresh_inspection(&mut self, wm: Option<&LiveWmSession>) {
        let Some(prepared) = self.prepared.as_mut() else {
            return;
        };
        let Some(service) = wm
            .and_then(|wm| wm.public.as_ref())
            .and_then(|public| public.inspection.as_ref())
            .map(|inspection| &inspection.service)
        else {
            return;
        };
        let Some(generation) = service.generation() else {
            return;
        };
        for target in prepared.targets.values_mut() {
            if let Some((endpoint, owner)) = target.inspection.as_mut()
                && owner.generation() != generation
                && let Ok(next) = service.authenticated_acceptor(generation)
            {
                endpoint.service.generation = generation;
                *owner = next;
            }
        }
    }
    /// Startup only: capture already-bound X listeners and real service owners.
    /// The owner loop never loads recipes or resolves a new X socket path.
    pub(super) fn prepare(
        config: &PersistentXtermSessionConfig,
        registry: Arc<Mutex<NamespaceRegistry>>,
        namespaces: &BTreeMap<sophia_x_authority::XServerFrontendListenerId, NamespaceId>,
        xauthority: &Path,
        session_generation: u64,
        wm: Option<&LiveWmSession>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let policy = &config.core_config_state.active().session.namespace_policy;
        if policy.recipes.is_empty() {
            return Ok(Self::default());
        }
        let root = config
            .core_config_source
            .path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new("/"));
        let recipes = ApplicationRecipeSet::load(policy, root)?;
        let parent =
            std::env::var_os("XDG_RUNTIME_DIR").ok_or("recipe runtime directory absent")?;
        let service = NamespaceLaunchService::new(registry.clone(), parent.into());
        let display = parse_display_number(&config.display)?;
        let layout = socket_directories::SocketDirectoryLayout::prepare(display)?;
        let mut targets = BTreeMap::new();
        let mut portals = BTreeMap::new();
        let mut x_targets = BTreeMap::new();
        for (name, recipe) in &policy.recipes {
            let (context, x11, inspection) = match recipe.profile {
                NamespaceRecipeProfile::SharedX | NamespaceRecipeProfile::ConfinedX { .. } => {
                    let group = match recipe.profile {
                        NamespaceRecipeProfile::ConfinedX { group } => Some(group),
                        _ => None,
                    };
                    let listener = group.map_or(
                        sophia_x_authority::XServerFrontendListenerId::PRIMARY,
                        sophia_x_authority::XServerFrontendListenerId,
                    );
                    let namespace = namespaces
                        .get(&listener)
                        .ok_or("recipe X group is not configured")?;
                    let context = registry
                        .lock()
                        .map_err(|_| "namespace registry poisoned")?
                        .namespace(*namespace)
                        .ok_or("recipe X namespace retired")?;
                    let x11 = if let Some(target) = x_targets.get(namespace) {
                        Arc::clone(target)
                    } else {
                        let paths = sophia_runtime::ConfinedXApplicationLaunch {
                            executable: "/usr/bin/true".into(),
                            args: Vec::new(),
                            bubblewrap: config.bubblewrap.clone(),
                            group_directory: layout.group_directory(group.map_or(
                                socket_directories::ClientGroup::Shared,
                                socket_directories::ClientGroup::Confined,
                            )),
                            cookie_file: xauthority.into(),
                            display_number: display,
                        };
                        let target = Arc::new(RecipeXTarget::capture(
                            context,
                            group,
                            reference(
                                context.id,
                                session_generation,
                                "x11",
                                EndpointProtocol::X11,
                            )?,
                            &paths,
                        )?);
                        x_targets.insert(*namespace, target.clone());
                        target
                    };
                    (context, Some(x11), None)
                }
                NamespaceRecipeProfile::Observer => {
                    let capabilities =
                        recipe
                            .request
                            .iter()
                            .fold(NamespaceCapabilities::NONE, |caps, cap| {
                                caps.with_request(crate::namespace_launch::recipe_capability(*cap))
                            });
                    let context = registry
                        .lock()
                        .map_err(|_| "namespace registry poisoned")?
                        .create_namespace(NamespaceProfile::Confined, capabilities);
                    let inspection = wm
                        .and_then(|wm| wm.public.as_ref())
                        .and_then(|public| public.inspection.as_ref())
                        .ok_or("observer inspection owner absent")?;
                    let generation = inspection
                        .service
                        .generation()
                        .ok_or("observer inspection owner fenced")?;
                    let endpoint = ApprovedEndpoint {
                        service: ServiceReference {
                            identity: 1,
                            generation,
                            protocol: EndpointProtocol::NineP2000L,
                            interface: "inspection".into(),
                            version: 1,
                        },
                        audience: 5,
                        availability: ServiceAvailability::Available,
                    };
                    (
                        context,
                        None,
                        Some((
                            endpoint,
                            inspection.service.authenticated_acceptor(generation)?,
                        )),
                    )
                }
            };
            let objects = portals
                .entry(context.id)
                .or_insert_with(|| {
                    Arc::new(sophia_portal::recipient_object::PortalObjectRecipient::new(
                        context.id,
                    ))
                })
                .clone();
            targets.insert(
                name.clone(),
                RecipeLaunchTarget {
                    context,
                    principal: 0,
                    x11,
                    inspection,
                    portal: (
                        ApprovedEndpoint {
                            service: reference(
                                context.id,
                                session_generation,
                                "portal-client",
                                EndpointProtocol::NineP2000L,
                            )?,
                            audience: 4,
                            availability: ServiceAvailability::Available,
                        },
                        objects,
                    ),
                    bubblewrap: config.bubblewrap.clone(),
                    host_control: config.control_socket.clone(),
                    host_inspection: config.inspection_socket.clone(),
                },
            );
        }
        Ok(Self {
            prepared: Some(Prepared {
                policy: policy.clone(),
                recipes,
                service,
                targets,
                next_principal: 1 << 32,
            }),
        })
    }
}

#[path = "../../tests/support/application_recipe_launches.rs"]
mod tests;

fn reference(
    namespace: NamespaceId,
    generation: u64,
    interface: &str,
    protocol: EndpointProtocol,
) -> Result<ServiceReference, String> {
    // One identity per actual namespace owner, shared by every recipe naming it.
    let identity = namespace
        .raw()
        .checked_mul(2)
        .and_then(|id| {
            id.checked_add(if protocol == EndpointProtocol::X11 {
                2
            } else {
                3
            })
        })
        .ok_or("application service identity exhausted")?;
    Ok(ServiceReference {
        identity,
        generation,
        protocol,
        interface: interface.into(),
        version: 1,
    })
}

impl ApplicationLaunchTurn<'_> {
    pub(super) fn spawn_developer(
        &mut self,
        config: &PersistentXtermSessionConfig,
        request: &sophia_protocol::developer_launch::DeveloperLaunchRequest,
    ) -> io::Result<ApplicationProcess> {
        let prepared = self
            .applications
            .prepared
            .as_mut()
            .ok_or_else(|| io::Error::other("namespace launch resources absent"))?;
        if prepared.policy != config.core_config_state.active().session.namespace_policy {
            return Err(io::Error::other(
                "namespace launch policy changed; restart required",
            ));
        }
        let recipe = prepared
            .recipes
            .developer(&request.recipe)
            .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error))?;
        prepared.start(
            recipe,
            &ApplicationLaunchCommand {
                executable: request.executable.clone().into(),
                arguments: request.arguments.clone(),
                working_directory: None,
                namespace_recipe: Some(request.recipe.clone()),
            },
            self.authentication,
            self.wake.clone(),
            LaunchContext {
                source: crate::diagnostics::application::LaunchSource::Developer,
                transaction: Some(u64::from(request.request)),
            },
        )
    }

    pub(super) fn spawn_registered(
        &mut self,
        config: &PersistentXtermSessionConfig,
        xauthority: &Path,
        app: &SessionApplicationSpec,
        context: LaunchContext,
    ) -> io::Result<ApplicationProcess> {
        self.spawn(
            config,
            xauthority,
            &format!("registered:{}", app.id),
            &ApplicationLaunchCommand {
                executable: app.executable.clone(),
                arguments: app.arguments.clone(),
                working_directory: None,
                namespace_recipe: None,
            },
            context,
        )
    }
    pub(super) fn spawn(
        &mut self,
        config: &PersistentXtermSessionConfig,
        xauthority: &Path,
        identity: &str,
        command: &ApplicationLaunchCommand,
        context: LaunchContext,
    ) -> io::Result<ApplicationProcess> {
        let current = &config.core_config_state.active().session.namespace_policy;
        if let Some(prepared) = self.applications.prepared.as_mut() {
            if &prepared.policy != current {
                return Err(io::Error::other(
                    "namespace launch policy changed; restart required",
                ));
            }
            if let Some(recipe) = prepared
                .recipes
                .application(identity, command)
                .map_err(io::Error::other)?
            {
                return prepared.start(
                    recipe,
                    command,
                    self.authentication,
                    self.wake.clone(),
                    context,
                );
            }
        } else if !current.recipes.is_empty() {
            return Err(io::Error::other("namespace launch resources absent"));
        }
        crate::application_catalog::spawn_application_process(
            command,
            CatalogProcessEnvironment {
                display: &config.display,
                xauthority,
                control_socket: config.control_socket.as_deref(),
                inspection_socket: config.inspection_socket.as_deref(),
                confinement: config.application_confinement(xauthority),
            },
            Some(identity),
            context,
        )
        .map(ApplicationProcess::from)
    }
}

impl Prepared {
    fn start(
        &mut self,
        recipe: Arc<sophia_config::namespace_policy::LoadedNamespaceRecipe>,
        command: &ApplicationLaunchCommand,
        authentication: &mut crate::launch_authentication::LaunchCoordinator,
        wake: sophia_wake::Notifier,
        context: LaunchContext,
    ) -> io::Result<ApplicationProcess> {
        let principal = self.next_principal;
        self.next_principal = principal
            .checked_add(1)
            .ok_or_else(|| io::Error::other("application principal exhausted"))?;
        let target = self
            .targets
            .get_mut(&recipe.policy().name)
            .ok_or_else(|| io::Error::other("selected recipe resources absent"))?;
        target.principal = principal;
        target
            .resolve(recipe, command)
            .map_err(io::Error::other)?
            .start(&self.service, authentication, wake, Instant::now(), context)
            .map_err(io::Error::other)
    }
}

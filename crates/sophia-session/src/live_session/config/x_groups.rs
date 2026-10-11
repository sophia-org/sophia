use super::socket_directories::ClientGroup;
use sophia_protocol::{NamespaceCapabilities, NamespacePortalCapability};

/// At most this many confined groups, each with its own listener.
const MAX_CONFINED_GROUPS: usize = 8;
/// At most this many directional clipboard rules.
const MAX_CLIPBOARD_RULES: usize = 16;

/// `shared` or `confined-N` with N nonzero; whether N is configured is
/// checked once every argument has been read.
fn parse_group_selector(text: &str) -> Option<ClientGroup> {
    if text == "shared" {
        return Some(ClientGroup::Shared);
    }
    text.strip_prefix("confined-")
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|id| *id != 0)
        .map(ClientGroup::Confined)
}

/// Startup-only group selection. IDs name listeners, never credentials.
#[derive(Clone, Debug, Default)]
pub(super) struct XGroups {
    pub confined: Vec<u32>,
    pub xtest: Option<ClientGroup>,
    /// Registered application ID to confined group. The key is the Session
    /// application registry ID -- never an executable, and never a scanned
    /// desktop-entry ID, which does not exist until a catalog scan and so
    /// cannot be refused at startup. Scanned desktop entries stay unmapped,
    /// including one that runs the same executable as a mapped application.
    pub applications: std::collections::BTreeMap<String, u32>,
    /// Standing one-way clipboard acceptance, as (source, target) in
    /// argument order: the source may publish and the target may request.
    /// Default none. A rule grants capability only; the portal owner still
    /// matches each transfer's exact pair and decides it.
    pub clipboard_rules: Vec<(ClientGroup, ClientGroup)>,
}

impl XGroups {
    pub fn parse(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut groups = Self::default();
        for arg in args {
            if let Some(value) = arg.strip_prefix("--confined-group=") {
                let id = value
                    .parse::<u32>()
                    .ok()
                    .filter(|id| *id != 0)
                    .ok_or("--confined-group requires a nonzero u32 group index")?;
                if groups.confined.len() >= MAX_CONFINED_GROUPS || groups.confined.contains(&id) {
                    return Err("--confined-group accepts at most 8 distinct groups".into());
                }
                groups.confined.push(id);
            } else if arg == "--confined-group" {
                return Err("use --confined-group=INDEX".into());
            } else if let Some(value) = arg.strip_prefix("--confined-application=") {
                let (app, group) = value
                    .rsplit_once(':')
                    .filter(|(app, _)| !app.is_empty())
                    .ok_or("--confined-application requires APP_ID:GROUP_ID")?;
                let group = group
                    .parse::<u32>()
                    .ok()
                    .filter(|id| *id != 0)
                    .ok_or("--confined-application requires a nonzero u32 group index")?;
                if groups.applications.insert(app.to_owned(), group).is_some() {
                    return Err("--confined-application names an application twice".into());
                }
            } else if arg == "--confined-application" {
                return Err("use --confined-application=APP_ID:GROUP_ID".into());
            } else if arg == "--admit-xtest" || arg.starts_with("--admit-xtest=") {
                if groups.xtest.is_some() {
                    return Err("XTEST may be admitted to exactly one group".into());
                }
                let name = arg.strip_prefix("--admit-xtest=").unwrap_or("shared");
                groups.xtest = Some(
                    parse_group_selector(name)
                        .ok_or("--admit-xtest requires shared or confined-INDEX")?,
                );
            } else if let Some(value) = arg.strip_prefix("--clipboard-rule=") {
                let rule = value
                    .split_once(':')
                    .and_then(|(source, target)| {
                        Some((parse_group_selector(source)?, parse_group_selector(target)?))
                    })
                    .ok_or(
                        "--clipboard-rule requires SOURCE:TARGET, each shared or confined-INDEX",
                    )?;
                if rule.0 == rule.1 {
                    return Err("--clipboard-rule needs two distinct groups".into());
                }
                if groups.clipboard_rules.contains(&rule) {
                    return Err("--clipboard-rule names a pair twice".into());
                }
                if groups.clipboard_rules.len() >= MAX_CLIPBOARD_RULES {
                    return Err("--clipboard-rule accepts at most 16 pairs".into());
                }
                groups.clipboard_rules.push(rule);
            } else if arg == "--clipboard-rule" {
                return Err("use --clipboard-rule=SOURCE:TARGET".into());
            }
        }
        if let Some(ClientGroup::Confined(id)) = groups.xtest
            && !groups.confined.contains(&id)
        {
            return Err("--admit-xtest names an unconfigured confined group".into());
        }
        if groups
            .applications
            .values()
            .any(|id| !groups.confined.contains(id))
        {
            return Err("--confined-application names an unconfigured confined group".into());
        }
        if groups.clipboard_rules.iter().any(|(source, target)| {
            [source, target].into_iter().any(|group| match group {
                ClientGroup::Confined(id) => !groups.confined.contains(id),
                ClientGroup::Shared => false,
            })
        }) {
            return Err("--clipboard-rule names an unconfigured confined group".into());
        }
        Ok(groups)
    }

    /// `base` plus the Clipboard publish and request capabilities the rules
    /// give `group`: publish as a rule's source, request as its target. Only
    /// Clipboard bits are ever added; nothing in `base` is cleared.
    pub fn capabilities(
        &self,
        group: ClientGroup,
        base: NamespaceCapabilities,
    ) -> NamespaceCapabilities {
        let clipboard = NamespacePortalCapability::Clipboard;
        self.clipboard_rules
            .iter()
            .fold(base, |capabilities, (source, target)| {
                let capabilities = if *source == group {
                    capabilities.with_publish(clipboard)
                } else {
                    capabilities
                };
                if *target == group {
                    capabilities.with_request(clipboard)
                } else {
                    capabilities
                }
            })
    }

    /// Refuse XTEST beside an input proof, and, before anything starts, an
    /// application mapping no launch path could honour as confined. Each
    /// refusal is fail-closed: the alternative would be an application the
    /// operator confined starting on the host.
    pub fn validate(
        &self,
        normal_session: bool,
        applications: &super::SessionApplicationConfig,
        catalog: Option<&sophia_config::ApplicationCatalogConfig>,
        input_proof: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.xtest.is_some() && input_proof {
            return Err(
                "--admit-xtest cannot be combined with an input proof: a synthetic source could satisfy it"
                    .into(),
            );
        }
        if self.applications.is_empty() {
            return Ok(());
        }
        if !normal_session {
            return Err("--confined-application requires --session-mode=normal".into());
        }
        let catalog =
            catalog.ok_or("--confined-application requires a selected application catalog")?;
        for app in self.applications.keys() {
            if !applications.applications.contains_key(app) {
                return Err(format!(
                    "--confined-application names unregistered application {app:?}"
                )
                .into());
            }
            if !catalog.applications.contains(app) {
                return Err(format!(
                    "--confined-application {app:?} is not in the selected catalog"
                )
                .into());
            }
            // Launched by inline startup code that this slice does not confine.
            if applications.startup.first() == Some(app) {
                return Err(format!(
                    "--confined-application {app:?} is the primary startup application"
                )
                .into());
            }
            // Terminal=true desktop entries run through it under their own
            // desktop IDs, which would put the confined terminal on the host.
            if catalog.terminal.as_ref() == Some(app) {
                return Err(
                    format!("--confined-application {app:?} is the catalog terminal").into(),
                );
            }
        }
        Ok(())
    }
}

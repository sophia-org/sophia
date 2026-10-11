use super::socket_directories::ClientGroup;

/// At most this many confined groups, each with its own listener.
const MAX_CONFINED_GROUPS: usize = 8;

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
                groups.xtest = Some(if name == "shared" {
                    ClientGroup::Shared
                } else {
                    let id = name
                        .strip_prefix("confined-")
                        .and_then(|n| n.parse::<u32>().ok())
                        .filter(|id| *id != 0)
                        .ok_or("--admit-xtest requires shared or confined-INDEX")?;
                    ClientGroup::Confined(id)
                });
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
        Ok(groups)
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

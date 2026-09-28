// One endpoint-selection owner is shared by initial, automatic and controlled
// starts. Neither negotiation nor attach can change the selected transport.
enum PublicPolicyTransport {
    Files(sophia_runtime::RoleEndpoint),
}
impl LiveWmSession {
    fn policy_wire_name(&self) -> &'static str {
        self.public
            .as_ref()
            .map_or(WmTransportSelection::NineP2000L, |public| {
                public.wm_transport
            })
            .wire_name()
    }
}
impl PublicPolicyTransport {
    fn socket_path(&self) -> &std::path::Path {
        match self {
            Self::Files(endpoint) => endpoint.socket_path(),
        }
    }
    fn authorize(
        &mut self,
        supervisor: &ProcessSupervisor,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Files(endpoint) => endpoint.authorize_protected_peer(
                supervisor
                    .protection_evidence()
                    .ok_or("WM file endpoint requires protected launch evidence")?,
            )?,
        }
        Ok(())
    }
}

fn bind_public_policy_transport(
    directory: &PolicySessionDirectory,
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    selection: WmTransportSelection,
) -> Result<PublicPolicyTransport, Box<dyn std::error::Error>> {
    bind_public_policy_endpoint(directory.endpoint_path(), profile_key, selection)
}

fn bind_public_policy_endpoint(
    endpoint: impl AsRef<std::path::Path>,
    _profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    selection: WmTransportSelection,
) -> Result<PublicPolicyTransport, Box<dyn std::error::Error>> {
    let uid = rustix::process::geteuid().as_raw();
    match selection {
        WmTransportSelection::NineP2000L => Ok(PublicPolicyTransport::Files(
            sophia_runtime::RoleEndpoint::bind_for_supervised_uid(endpoint, uid)?,
        )),
    }
}

fn start_public_policy_worker(
    transport: PublicPolicyTransport,
    connection_epoch: u64,
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    native_scanout: bool,
    supervisor: &ProcessSupervisor,
    qids: policy_transport_worker::PolicyFilesystemQids,
) -> Result<PolicyTransportWorker, Box<dyn std::error::Error>> {
    let ceiling = if native_scanout {
        u64::MAX
    } else {
        !(sophia_protocol::SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
            | sophia_protocol::SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS)
    };
    match transport {
        PublicPolicyTransport::Files(endpoint) => Ok(PolicyTransportWorker::new_files(
            endpoint,
            supervisor,
            connection_epoch,
            sophia_protocol::wm_files::WmFileLimits {
                capability_ceiling: sophia_runtime::select_policy_capabilities(
                    u64::MAX,
                    ceiling,
                    profile_key.is_some(),
                ),
                profile_required: profile_key.is_some(),
            },
            qids,
            profile_key
                .map(|key| policy_profile_identity(connection_epoch, key))
                .transpose()?,
        )?),
    }
}

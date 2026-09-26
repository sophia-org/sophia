// One endpoint-selection owner is shared by initial, automatic and controlled
// starts. Neither negotiation nor attach can change the selected transport.
enum PublicPolicyTransport {
    CurrentIpc(Box<sophia_runtime::PolicyWmSessionTransport>),
    Files(sophia_runtime::PolicyRoleEndpoint),
}
impl LiveWmSession {
    fn policy_wire_name(&self) -> &'static str {
        self.public
            .as_ref()
            .map_or(WmTransportSelection::CurrentIpc, |public| {
                public.wm_transport
            })
            .wire_name()
    }
}
impl PublicPolicyTransport {
    fn socket_path(&self) -> &std::path::Path {
        match self {
            Self::CurrentIpc(transport) => transport.socket_path(),
            Self::Files(endpoint) => endpoint.socket_path(),
        }
    }
    fn authorize(
        &mut self,
        supervisor: &ProcessSupervisor,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::CurrentIpc(transport) => transport.authorize_supervised_pid(
                supervisor
                    .peer_id()
                    .ok_or("public WM has no supervised PID")?,
            )?,
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
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    selection: WmTransportSelection,
) -> Result<PublicPolicyTransport, Box<dyn std::error::Error>> {
    let uid = rustix::process::geteuid().as_raw();
    match selection {
        WmTransportSelection::CurrentIpc => Ok(PublicPolicyTransport::CurrentIpc(Box::new(
            if profile_key.is_some() {
                sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid_profile_activation(endpoint, uid)?
            } else {
                sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid(endpoint, uid)?
            },
        ))),
        WmTransportSelection::NineP2000L => Ok(PublicPolicyTransport::Files(
            sophia_runtime::PolicyRoleEndpoint::bind_for_supervised_uid(endpoint, uid)?,
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
        PublicPolicyTransport::CurrentIpc(mut transport) => {
            if !native_scanout {
                transport.limit_capabilities(ceiling)?;
            }
            match profile_key {
                Some(key) => Ok(PolicyTransportWorker::new_profile_activated(
                    *transport,
                    connection_epoch,
                    policy_profile_identity(connection_epoch, key)?,
                    TransactionId::from_raw(1),
                    TransactionId::from_raw(2),
                )?),
                None => Ok(PolicyTransportWorker::new(*transport, connection_epoch)?),
            }
        }
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

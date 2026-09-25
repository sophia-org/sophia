//! Shared capability and exact registry admission; no socket I/O.
use super::*;

impl ShellComponentTransport {
    pub(super) fn select_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        content_policy: ShellContentAdmissionPolicy,
        hello: ShellV1ClientHello,
    ) -> Result<(ShellV1ServerWelcome, Option<ContentLimits>), ShellTransportError> {
        if epochs.profile(self.store_grant) == Some(crate::ContentStoreProfile::PersistentCatalog) {
            return self.select_catalog_negotiation(connection_epoch, content_policy, hello);
        }
        if epochs.profile(self.store_grant) == Some(crate::ContentStoreProfile::NativeLauncher) {
            return self.select_native_launcher_negotiation(
                connection_epoch,
                content_policy,
                hello,
            );
        }
        if hello.minimum_revision == 0
            || hello.minimum_revision > hello.maximum_revision
            || hello.minimum_revision > sophia_protocol::SOPHIA_SHELL_OVERVIEW_REVISION
        {
            return Err(ShellTransportError::UnsupportedRevision);
        }
        if hello.required_capabilities & SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER == 0 {
            return Err(ShellTransportError::MissingCapability);
        }
        let revision = hello
            .maximum_revision
            .min(sophia_protocol::SOPHIA_SHELL_OVERVIEW_REVISION);
        let capabilities = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | if revision >= 2 {
                hello.required_capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_TAB_GROUPS
            } else {
                0
            };
        let capabilities = capabilities
            | if revision >= 3 {
                hello.required_capabilities
                    & (sophia_protocol::SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG
                        | sophia_protocol::SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET)
            } else {
                0
            };
        let capabilities = capabilities
            | if revision >= sophia_protocol::SOPHIA_SHELL_OVERVIEW_REVISION {
                hello.required_capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_OVERVIEW
            } else {
                0
            };
        let launcher_mask = sophia_protocol::SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER;
        let capabilities = capabilities
            | if revision >= 4 {
                hello.required_capabilities & launcher_mask
            } else {
                0
            };
        let content_request = hello.required_capabilities
            & (sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT);
        if content_request & sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT != 0
            && content_request & sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE == 0
        {
            return Err(ShellTransportError::MissingCapability);
        }
        let content_decision = content_admission::decide(revision, content_request, content_policy);
        let content_capabilities = match content_decision {
            content_admission::ContentAdmissionDecision::NotRequested => 0,
            content_admission::ContentAdmissionDecision::Granted(capabilities) => capabilities,
            content_admission::ContentAdmissionDecision::Refused(refusal) => {
                return Err(ShellTransportError::ContentAdmissionRefused(refusal));
            }
        };
        let indicator_mask = sophia_protocol::SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION;
        let capabilities = capabilities | content_capabilities;
        let capabilities = capabilities
            | if revision >= sophia_protocol::SOPHIA_SHELL_INDICATOR_REVISION {
                hello.required_capabilities & indicator_mask
            } else {
                0
            };
        if capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION != 0
            && capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS == 0
        {
            return Err(ShellTransportError::MissingCapability);
        }
        if capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER != 0
            && capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG == 0
        {
            return Err(ShellTransportError::MissingCapability);
        }
        if capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET != 0
            && capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG == 0
        {
            return Err(ShellTransportError::MissingCapability);
        }
        if hello.required_capabilities & !capabilities != 0 {
            return Err(ShellTransportError::MissingCapability);
        }
        let welcome = ShellV1ServerWelcome {
            selected_revision: revision,
            connection_epoch,
            capabilities,
            max_descriptors: SOPHIA_SHELL_MAX_DESCRIPTORS as u16,
            max_label_bytes: sophia_protocol::MAX_CHROME_LABEL_LEN as u16,
            max_pending_activations: SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u16,
        };
        if content_capabilities == 0 && self.reserved_limits.is_some() {
            return Err(ShellTransportError::MissingCapability);
        }
        let content_limits = if content_capabilities != 0 {
            if let Some(limits) = &self.reserved_limits {
                Some(limits.clone())
            } else {
                let limits = ContentLimits::prototype(epochs.next_grant(connection_epoch)?);
                match epochs.admit(limits.clone()) {
                    Ok(()) => {
                        self.store_grant = limits.grant;
                    }
                    Err(ContentStoreError::Budget) => {
                        return Err(ShellTransportError::ContentAdmissionRefused(
                            ContentAdmissionRefused {
                                reason: 4,
                                denied_capabilities: content_request,
                            },
                        ));
                    }
                    Err(error) => return Err(error.into()),
                }
                Some(limits)
            }
        } else {
            None
        };
        Ok((welcome, content_limits))
    }
}

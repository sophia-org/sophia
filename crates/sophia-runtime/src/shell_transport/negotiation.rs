//! Protected peer negotiation shared by standalone and component owners.
use super::*;

impl ShellComponentTransport {
    /// Reserve the complete footprint before the supervisor launches a peer.
    /// This is operator-side storage preparation, not a negotiated capability.
    /// A refused reservation changes neither this connection nor a neighbor.
    /// The caller must disconnect this owner on launch failure or abandonment.
    pub fn reserve_content(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        limits: ContentLimits,
    ) -> Result<(), ShellTransportError> {
        if self.stream.is_some()
            || self.content_grant.is_some()
            || self.reserved_limits.is_some()
            || epochs.resources(self.store_grant).is_some()
        {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if limits.grant.connection_epoch <= self.connection_epoch {
            return Err(ShellTransportError::InvalidConnectionEpoch);
        }
        epochs.admit(limits.clone())?;
        self.store_grant = limits.grant;
        self.reserved_limits = Some(limits);
        Ok(())
    }

    pub fn accept_and_negotiate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.accept_and_negotiate_with_content_policy(
            epochs,
            connection_epoch,
            timeout,
            ShellContentAdmissionPolicy::Unavailable,
        )
    }

    /// Negotiate one protected shell peer under an explicit content policy.
    ///
    /// Codec support does not grant content. Production callers must name the
    /// operator decision, and the legacy entry point remains unavailable.
    pub fn accept_and_negotiate_with_content_policy(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        content_policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(ShellTransportError::InvalidConnectionEpoch);
        }
        if self.stream.is_some() || self.content_grant.is_some() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if self.reserved_limits.as_ref().is_some_and(|limits| {
            limits.grant.connection_epoch != connection_epoch
                || epochs.resources(limits.grant).is_none()
        }) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let result = self.negotiate_peer(epochs, connection_epoch, timeout, content_policy);
        if result.is_err() {
            // Revoke the exact reservation even when failure precedes Welcome.
            // The original negotiation failure is retained if endpoint release
            // also reports bookkeeping failure; no grant or socket survives.
            let _ = self.disconnect(epochs);
        }
        result
    }

    fn negotiate_peer(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        content_policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        let mut stream = self.endpoint.accept_expected_timeout(timeout)?;
        configure_stream(&stream)?;
        let hello = decode_shell_v1_client_hello_frame(&read_frame(&mut stream)?)?;
        if hello.minimum_revision == 0
            || hello.minimum_revision > hello.maximum_revision
            || hello.minimum_revision > sophia_protocol::SOPHIA_SHELL_INDICATOR_REVISION
        {
            return Err(ShellTransportError::UnsupportedRevision);
        }
        if hello.required_capabilities & SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER == 0 {
            return Err(ShellTransportError::MissingCapability);
        }
        let revision = hello
            .maximum_revision
            .min(sophia_protocol::SOPHIA_SHELL_INDICATOR_REVISION);
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
                return self.refuse_content(stream, refusal);
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
                        return self.refuse_content(
                            stream,
                            ContentAdmissionRefused {
                                reason: 4,
                                denied_capabilities: content_request,
                            },
                        );
                    }
                    Err(error) => return Err(error.into()),
                }
                Some(limits)
            }
        } else {
            None
        };
        let next_content_grant = content_limits.as_ref().map(|limits| limits.grant);
        let write_result = (|| {
            write_frame(&mut stream, &encode_shell_v1_server_welcome_frame(welcome)?)?;
            if let Some(limits) = &content_limits {
                write_frame(
                    &mut stream,
                    &sophia_protocol::encode_shell_content_frame(
                        TransactionId::INVALID,
                        &sophia_protocol::ShellContentRecord::Limits(limits.clone()),
                    )?,
                )?;
            }
            Ok::<(), ShellTransportError>(())
        })();
        if let Err(error) = write_result {
            if next_content_grant.is_some() {
                epochs.disconnect(self.store_grant);
            }
            return Err(error);
        }
        self.pending_activations.clear();
        self.last_candidate_generation = 0;
        self.requested_candidate = None;
        self.pending_candidate = None;
        self.presented_candidate = None;
        self.connection_epoch = connection_epoch;
        self.content_grant = next_content_grant;
        self.content_limits = content_limits;
        self.reserved_limits = None;
        stream
            .set_nonblocking(true)
            .map_err(|e| ShellTransportError::Io(e.to_string()))?;
        self.peer_closed = false;
        self.input.clear();
        self.output.clear();
        self.action_cancellations.clear();
        self.indicator_response = None;
        self.inbox.clear();
        self.capabilities = capabilities;
        self.stream = Some(stream);
        Ok(welcome)
    }

    fn refuse_content(
        &mut self,
        mut stream: UnixStream,
        refusal: ContentAdmissionRefused,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        let frame = sophia_protocol::encode_shell_content_frame(
            TransactionId::INVALID,
            &sophia_protocol::ShellContentRecord::AdmissionRefused(refusal.clone()),
        )?;
        let write_result = write_frame(&mut stream, &frame);
        let _ = stream.shutdown(std::net::Shutdown::Both);
        let release_result = self
            .endpoint
            .active_peer()
            .map(|peer| self.endpoint.release_peer(peer))
            .transpose();
        write_result?;
        release_result?;
        Err(ShellTransportError::ContentAdmissionRefused(refusal))
    }
}

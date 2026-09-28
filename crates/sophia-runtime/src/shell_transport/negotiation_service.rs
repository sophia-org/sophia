//! Retained protected handshake. No sleep or blocking socket operation here.
//! Admission and the reset of the connection's state are shared; each wire
//! only carries the Hello and its answer.
use super::*;
use std::time::Instant;

/// One pending handshake reserves two answer records within this many bytes
/// (the welcome and the limits, or the refusal) on either wire.
pub(super) const REPLY_BYTES: usize = 512;
pub(super) const REPLY_RECORDS: usize = 2;

/// `body "Negotiate" size=16`.
const NEGOTIATE_BODY_BYTES: usize = 16;

pub(super) struct PendingNegotiation {
    pub(super) deadline: Instant,
    pub(super) epoch: u64,
    pub(super) policy: ShellContentAdmissionPolicy,
    pub(super) stage: Stage,
    pub(super) selected: Option<(ShellV1ServerWelcome, Option<ContentLimits>)>,
    pub(super) refusal: Option<ContentAdmissionRefused>,
    /// Session selected the file wire for this component at startup.
    file: bool,
}

pub(super) enum Stage {
    /// No peer accepted yet.
    Waiting,
    /// A file peer accepted but not yet served as an export.
    Accepted(UnixStream),
    Socket(Box<socket::negotiation::Handshake>),
    Files(Box<files::ShellFileWire>),
}

impl ShellComponentTransport {
    /// Start without accepting or contacting a peer, over `sophia_shell_fs_v1`:
    /// the admitted
    /// stream is served as this component's 9P export, and negotiation is
    /// its one submitted Negotiate record. Nothing here changes admission.
    pub fn begin_file_negotiation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<(), ShellTransportError> {
        self.begin_selected_negotiation(epochs, connection_epoch, timeout, policy, true)
    }

    pub(super) fn begin_selected_negotiation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
        file: bool,
    ) -> Result<(), ShellTransportError> {
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(ShellTransportError::InvalidConnectionEpoch);
        }
        if self.negotiation.is_some() || self.wire.is_some() || self.content_grant.is_some() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if self.reserved_limits.as_ref().is_some_and(|limits| {
            limits.grant.connection_epoch != connection_epoch
                || epochs.resources(limits.grant).is_none()
        }) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| ShellTransportError::Io("negotiation deadline out of range".into()))?;
        self.negotiation = Some(PendingNegotiation {
            deadline,
            epoch: connection_epoch,
            policy,
            stage: Stage::Waiting,
            selected: None,
            refusal: None,
            file,
        });
        Ok(())
    }

    /// At most one accept and 32 read/write attempts, sharing at most 64 KiB
    /// with this visit's byte budget. Zero bytes performs no socket operation.
    /// None means still pending, not connected. Welcome is returned exactly
    /// once, after all welcome/limit bytes have been written (not peer receipt).
    /// On returned failure, the exact reservation/socket is revoked; neighbors
    /// remain owned by the registry. Interruption/unwind is not a terminal ACK.
    pub fn poll_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        byte_budget: usize,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        let result = self.visit_negotiation(epochs, byte_budget.min(64 * 1024));
        if result.is_err() && self.negotiation.is_some() {
            let _ = self.disconnect(epochs);
        }
        result
    }

    fn visit_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        budget: usize,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        let pending = self
            .negotiation
            .as_mut()
            .ok_or(ShellTransportError::NotConnected)?;
        if Instant::now() >= pending.deadline {
            return Err(RoleEndpointError::AcceptTimedOut.into());
        }
        if budget == 0 {
            return Ok(None);
        }
        if matches!(pending.stage, Stage::Waiting) {
            let Some(stream) = self.endpoint.poll_expected()? else {
                return Ok(None);
            };
            let pending = self.negotiation.as_mut().expect("visit retains handshake");
            // Store before the fallible mode change. Returned-error cleanup owns it.
            pending.stage = if pending.file {
                Stage::Accepted(stream)
            } else {
                Stage::Socket(Box::new(socket::negotiation::Handshake::new(stream)))
            };
            let stream = match &pending.stage {
                Stage::Accepted(stream) => stream,
                Stage::Socket(handshake) => handshake.stream(),
                Stage::Waiting | Stage::Files(_) => unreachable!("just accepted"),
            };
            stream
                .set_nonblocking(true)
                .map_err(|error| ShellTransportError::Io(error.to_string()))?;
        }
        if self
            .negotiation
            .as_ref()
            .is_some_and(|pending| pending.file)
        {
            return self.visit_file_negotiation(epochs);
        }
        self.visit_socket_negotiation(epochs, budget)
    }

    /// Resets connection state for a freshly selected epoch. Both wires use
    /// it after their last fallible step, then install themselves.
    pub(super) fn install_negotiated(
        &mut self,
        welcome: ShellV1ServerWelcome,
        limits: Option<ContentLimits>,
    ) {
        self.connection_epoch = welcome.connection_epoch;
        self.content_grant = limits.as_ref().map(|limits| limits.grant);
        self.content_limits = limits;
        self.reserved_limits = None;
        self.peer_closed = false;
        self.output.clear();
        self.action_cancellations.clear();
        self.indicator_response = None;
        self.catalog_response = None;
        self.native_control = super::native_launcher::control::NativeControl::default();
        self.capabilities = welcome.capabilities;
    }

    /// One nonblocking turn of the pending file export. The accepted stream
    /// becomes the export's single connection on the first visit. A refusal
    /// is journaled and the component is revoked once the peer acknowledged
    /// it, or at the negotiation deadline, whichever comes first.
    fn visit_file_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        let profile = epochs.profile(self.store_grant);
        let file_qids = self.file_qids;
        let pending = self.negotiation.as_mut().expect("visit retains handshake");
        if let Stage::Accepted(_) = pending.stage {
            let Stage::Accepted(stream) = std::mem::replace(&mut pending.stage, Stage::Waiting)
            else {
                unreachable!("matched above");
            };
            let (role, bounds) = super::files::role_bounds(profile);
            // The launcher/dock profile discloses `catalog`; the bar's Legacy
            // profile never does, no matter which capabilities it negotiates.
            let catalog_allowed = matches!(
                profile,
                Some(crate::ContentStoreProfile::NativeLauncher)
                    | Some(crate::ContentStoreProfile::PersistentCatalog)
            );
            let export = super::files::ShellFiles::awaiting_negotiation(
                pending.epoch,
                role,
                catalog_allowed,
                bounds,
                file_qids,
                Instant::now(),
            );
            pending.stage = Stage::Files(Box::new(super::files::ShellFileWire::adopt(
                stream, export,
            )?));
        }
        let Stage::Files(wire) = &mut pending.stage else {
            unreachable!("a file handshake is served as an export");
        };
        wire.turn()?;
        if let Some(refusal) = &pending.refusal {
            if wire.export().journal().records() == 0 {
                return Err(ShellTransportError::ContentAdmissionRefused(
                    refusal.clone(),
                ));
            }
            return Ok(None);
        }
        let hello = match wire.export_mut().take_inbound() {
            None => return Ok(None),
            Some(super::files::Inbound::Negotiate(hello)) => hello,
            // No family record decodes before negotiation completes.
            Some(_) => return Err(ShellTransportError::WrongContentRecord),
        };
        let epoch = pending.epoch;
        let policy = pending.policy;
        // Admission is retained in the actual registry before encoding.
        let selected = self.select_negotiation(epochs, epoch, policy, hello);
        let pending = self.negotiation.as_mut().expect("visit retains handshake");
        let Stage::Files(wire) = &mut pending.stage else {
            unreachable!("a file handshake is served as an export");
        };
        match selected {
            Ok((welcome, limits)) => {
                let limits_object = limits
                    .as_ref()
                    .map(|limits| {
                        super::files::encode_limits_object(epoch, limits)
                            .map(|bytes| (bytes, limits.clone()))
                    })
                    .transpose()?;
                let body = super::files::encode_negotiated(welcome, limits.is_some())?;
                wire.export_mut()
                    .complete_negotiation(limits_object, welcome.capabilities)
                    .map_err(|error| ShellTransportError::Io(format!("{error:?}")))?;
                if !wire.append(
                    sophia_protocol::shell_files::ShellFileKind::Negotiated,
                    &body,
                    true,
                )? {
                    return Err(ShellTransportError::ContentQueueSaturated);
                }
                wire.turn()?;
                // No fallible operation after removing the exact handshake owner.
                let pending = self.negotiation.take().expect("completed handshake");
                let Stage::Files(wire) = pending.stage else {
                    unreachable!("a file handshake is served as an export");
                };
                self.install_negotiated(welcome, limits);
                self.wire = Some(super::wire::Wire::Files(wire));
                Ok(Some(welcome))
            }
            Err(ShellTransportError::ContentAdmissionRefused(refusal)) => {
                let body = super::files::encode_refused(&refusal)?;
                if !wire.append(
                    sophia_protocol::shell_files::ShellFileKind::Refused,
                    &body,
                    true,
                )? {
                    return Err(ShellTransportError::ContentQueueSaturated);
                }
                wire.turn()?;
                pending.refusal = Some(refusal);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

impl PendingNegotiation {
    /// The next logical qid of an adopted but unfinished file export, so a
    /// following epoch never reuses a qid this peer already observed.
    pub(super) fn file_qids(&self) -> Option<u64> {
        match &self.stage {
            Stage::Files(wire) => Some(wire.export().next_qid()),
            _ => None,
        }
    }

    /// Input this handshake holds for its Hello: the socket's fixed Hello
    /// buffer, or the file peer's one Negotiate record.
    pub(super) fn input_bytes(&self) -> usize {
        if self.file {
            sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES + NEGOTIATE_BODY_BYTES
        } else {
            socket::negotiation::HELLO_BYTES
        }
    }
}

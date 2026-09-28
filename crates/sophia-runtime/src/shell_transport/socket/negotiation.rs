//! The socket half of the handshake: one Hello frame in, the welcome and
//! limits (or a refusal) out, in fixed buffers. Admission is the shared
//! `select_negotiation`; nothing here decides it.
use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;

use sophia_protocol::{
    SOPHIA_IPC_HEADER_LEN, ShellContentRecord, ShellV1ServerWelcome, TransactionId,
    decode_shell_v1_client_hello_frame, encode_shell_content_frame,
    encode_shell_v1_server_welcome_frame,
};

use super::super::negotiation_service::{REPLY_BYTES, Stage};
use super::super::wire::Wire;
use super::super::{ShellComponentTransport, ShellContentAdmissionPolicy, ShellTransportError};
use super::SocketWire;
use std::time::Duration;

/// The one Hello frame: header plus its fixed 12-byte payload.
pub(in crate::shell_transport) const HELLO_BYTES: usize = SOPHIA_IPC_HEADER_LEN + 12;

pub(in crate::shell_transport) struct Handshake {
    stream: UnixStream,
    hello: [u8; HELLO_BYTES],
    received: usize,
    reply: [u8; REPLY_BYTES],
    reply_len: usize,
    sent: usize,
}

impl Handshake {
    pub(in crate::shell_transport) fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            hello: [0; HELLO_BYTES],
            received: 0,
            reply: [0; REPLY_BYTES],
            reply_len: 0,
            sent: 0,
        }
    }

    pub(in crate::shell_transport) fn stream(&self) -> &UnixStream {
        &self.stream
    }
}

fn handshake(transport: &mut ShellComponentTransport) -> &mut Handshake {
    match &mut transport
        .negotiation
        .as_mut()
        .expect("visit retains handshake")
        .stage
    {
        Stage::Socket(handshake) => handshake,
        _ => unreachable!("a socket handshake"),
    }
}

impl ShellComponentTransport {
    /// Start without accepting or contacting a peer. The owner must visit poll
    /// with a deadline and disconnect on abandonment. One pending handshake
    /// reserves two records/512 bytes plus its Hello; no ordinary traffic is
    /// admitted until these initial records have fully left it.
    pub fn begin_negotiation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<(), ShellTransportError> {
        self.begin_selected_negotiation(epochs, connection_epoch, timeout, policy, false)
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

    /// Blocking compatibility driver over the same retained negotiation. The
    /// timeout now bounds the entire handshake, not each separate I/O stage.
    pub fn accept_and_negotiate_with_content_policy(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        content_policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.begin_negotiation(epochs, connection_epoch, timeout, content_policy)?;
        loop {
            if let Some(welcome) = self.poll_negotiation(epochs, 64 * 1024)? {
                return Ok(welcome);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub(in crate::shell_transport) fn visit_socket_negotiation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        mut budget: usize,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        for _ in 0..32 {
            if budget == 0 {
                break;
            }
            let pending = handshake(self);
            if pending.received < HELLO_BYTES {
                let end = if pending.received < SOPHIA_IPC_HEADER_LEN {
                    SOPHIA_IPC_HEADER_LEN
                } else {
                    HELLO_BYTES
                };
                let end = end.min(pending.received + budget);
                match pending
                    .stream
                    .read(&mut pending.hello[pending.received..end])
                {
                    Ok(0) => return Err(ShellTransportError::Io("shell Hello EOF".into())),
                    Ok(count) => {
                        pending.received += count;
                        budget -= count;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(io_error(error)),
                }
                if pending.received == SOPHIA_IPC_HEADER_LEN {
                    let length = u32::from_le_bytes(
                        pending.hello[16..20].try_into().expect("header length"),
                    );
                    if length != 12 {
                        return Err(ShellTransportError::Io(
                            "shell Hello payload length must be 12".into(),
                        ));
                    }
                }
                if pending.received < HELLO_BYTES {
                    continue;
                }
            }
            if handshake(self).reply_len == 0 {
                let hello = decode_shell_v1_client_hello_frame(&handshake(self).hello)?;
                let negotiation = self.negotiation.as_ref().expect("visit retains handshake");
                let (epoch, policy) = (negotiation.epoch, negotiation.policy);
                // Admission is retained in the actual registry before encoding.
                let selected = self.select_negotiation(epochs, epoch, policy, hello);
                let negotiation = self.negotiation.as_mut().expect("visit retains handshake");
                let bytes = match selected {
                    Ok((welcome, limits)) => {
                        let mut bytes = encode_shell_v1_server_welcome_frame(welcome)?;
                        if let Some(limits) = &limits {
                            bytes.extend(encode_shell_content_frame(
                                TransactionId::INVALID,
                                &ShellContentRecord::Limits(limits.clone()),
                            )?);
                        }
                        negotiation.selected = Some((welcome, limits));
                        bytes
                    }
                    Err(ShellTransportError::ContentAdmissionRefused(refusal)) => {
                        negotiation.refusal = Some(refusal.clone());
                        encode_shell_content_frame(
                            TransactionId::INVALID,
                            &ShellContentRecord::AdmissionRefused(refusal),
                        )?
                    }
                    Err(error) => return Err(error),
                };
                if bytes.len() > REPLY_BYTES {
                    return Err(ShellTransportError::ContentQueueSaturated);
                }
                let pending = handshake(self);
                pending.reply[..bytes.len()].copy_from_slice(&bytes);
                pending.reply_len = bytes.len();
            }
            if budget == 0 {
                return Ok(None);
            }
            let pending = handshake(self);
            let end = pending.reply_len.min(pending.sent + budget);
            match pending.stream.write(&pending.reply[pending.sent..end]) {
                Ok(0) => return Err(ShellTransportError::Io("shell welcome write zero".into())),
                Ok(count) => {
                    pending.sent += count;
                    budget -= count;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(io_error(error)),
            }
            if pending.sent == pending.reply_len {
                let negotiation = self.negotiation.as_ref().expect("visit retains handshake");
                if let Some(refusal) = &negotiation.refusal {
                    return Err(ShellTransportError::ContentAdmissionRefused(
                        refusal.clone(),
                    ));
                }
                // No fallible operation after removing the exact handshake owner.
                let negotiation = self.negotiation.take().expect("completed handshake");
                let (welcome, limits) = negotiation.selected.expect("successful selection");
                let Stage::Socket(handshake) = negotiation.stage else {
                    unreachable!("a socket handshake");
                };
                let socket = Box::new(SocketWire::new(handshake.stream, limits.as_ref()));
                self.install_negotiated(welcome, limits);
                self.wire = Some(Wire::Socket(socket));
                return Ok(Some(welcome));
            }
        }
        Ok(None)
    }
}

fn io_error(error: io::Error) -> ShellTransportError {
    ShellTransportError::Io(error.to_string())
}

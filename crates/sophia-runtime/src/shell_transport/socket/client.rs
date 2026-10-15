//! The blocking descriptor-profile client used by conformance hosts and tests.
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    IpcCodecError, SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAX_PAYLOAD_LEN,
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER, SOPHIA_SHELL_INTERFACE_REVISION,
    ShellV1Activation, ShellV1ActivationAck, ShellV1Candidate, ShellV1CandidateOutcome,
    ShellV1ClientHello, ShellV1DescriptorSnapshot, TransactionId, decode_shell_v1_activation_frame,
    decode_shell_v1_candidate_outcome_frame, decode_shell_v1_descriptor_snapshot_frame,
    decode_shell_v1_server_welcome_frame, encode_shell_v1_activation_ack_frame,
    encode_shell_v1_candidate_frame, encode_shell_v1_client_hello_frame,
};

use super::super::ShellTransportError;

const SHELL_IO_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ShellClientTransport {
    stream: UnixStream,
    connection_epoch: u64,
}

impl ShellClientTransport {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self, ShellTransportError> {
        let mut stream = UnixStream::connect(path)
            .map_err(|error| ShellTransportError::Io(error.to_string()))?;
        configure_stream(&stream)?;
        let hello = ShellV1ClientHello {
            minimum_revision: SOPHIA_SHELL_INTERFACE_REVISION,
            maximum_revision: SOPHIA_SHELL_INTERFACE_REVISION,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
        };
        write_frame(&mut stream, &encode_shell_v1_client_hello_frame(hello)?)?;
        let welcome = decode_shell_v1_server_welcome_frame(&read_frame(&mut stream)?)?;
        if welcome.selected_revision != SOPHIA_SHELL_INTERFACE_REVISION
            || welcome.capabilities & SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER == 0
        {
            return Err(ShellTransportError::UnsupportedRevision);
        }
        stream
            .set_read_timeout(None)
            .map_err(|error| ShellTransportError::Io(error.to_string()))?;
        Ok(Self {
            stream,
            connection_epoch: welcome.connection_epoch,
        })
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub fn receive_snapshot(
        &mut self,
    ) -> Result<(TransactionId, ShellV1DescriptorSnapshot), ShellTransportError> {
        let (transaction, snapshot) =
            decode_shell_v1_descriptor_snapshot_frame(&read_frame(&mut self.stream)?)?;
        self.require_epoch(snapshot.connection_epoch)?;
        Ok((transaction, snapshot))
    }

    pub fn send_candidate(
        &mut self,
        transaction: TransactionId,
        candidate: &ShellV1Candidate,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(candidate.connection_epoch)?;
        write_frame(
            &mut self.stream,
            &encode_shell_v1_candidate_frame(transaction, candidate)?,
        )
    }

    pub fn receive_candidate_outcome(
        &mut self,
    ) -> Result<(TransactionId, ShellV1CandidateOutcome), ShellTransportError> {
        let (transaction, outcome) =
            decode_shell_v1_candidate_outcome_frame(&read_frame(&mut self.stream)?)?;
        self.require_epoch(outcome.connection_epoch)?;
        Ok((transaction, outcome))
    }

    pub fn receive_activation(
        &mut self,
    ) -> Result<(TransactionId, ShellV1Activation), ShellTransportError> {
        let (transaction, activation) =
            decode_shell_v1_activation_frame(&read_frame(&mut self.stream)?)?;
        self.require_epoch(activation.connection_epoch)?;
        Ok((transaction, activation))
    }

    pub fn acknowledge_activation(
        &mut self,
        transaction: TransactionId,
        ack: ShellV1ActivationAck,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(ack.connection_epoch)?;
        write_frame(
            &mut self.stream,
            &encode_shell_v1_activation_ack_frame(transaction, ack)?,
        )
    }

    fn require_epoch(&self, epoch: u64) -> Result<(), ShellTransportError> {
        if epoch == self.connection_epoch && epoch != 0 {
            Ok(())
        } else {
            Err(ShellTransportError::InvalidConnectionEpoch)
        }
    }
}

fn configure_stream(stream: &UnixStream) -> Result<(), ShellTransportError> {
    stream
        .set_read_timeout(Some(SHELL_IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(SHELL_IO_TIMEOUT)))
        .map_err(|error| ShellTransportError::Io(error.to_string()))
}

fn write_frame(stream: &mut UnixStream, frame: &[u8]) -> Result<(), ShellTransportError> {
    stream
        .write_all(frame)
        .map_err(|error| ShellTransportError::Io(error.to_string()))
}

fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>, ShellTransportError> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream
        .read_exact(&mut header)
        .map_err(|error| ShellTransportError::Io(error.to_string()))?;
    let payload_len = u32::from_le_bytes(
        header[16..20]
            .try_into()
            .expect("fixed frame payload range is present"),
    ) as usize;
    if payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(ShellTransportError::Codec(IpcCodecError::PayloadTooLarge(
            payload_len,
        )));
    }
    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload_len);
    frame.extend_from_slice(&header);
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .map_err(|error| ShellTransportError::Io(error.to_string()))?;
    Ok(frame)
}

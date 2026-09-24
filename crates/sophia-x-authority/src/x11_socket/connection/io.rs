#[cfg(unix)]
impl From<crate::XAuthorityTransportError> for X11SetupSocketError {
    fn from(error: crate::XAuthorityTransportError) -> Self {
        Self::new(error.to_string())
    }
}

/// The backing size of a DRI3 buffer whose client declared none.
///
/// `fstat` reads metadata only: nothing is mapped and the file offset the client
/// still shares is untouched. A size that is unreadable, negative, zero or wider
/// than the wire field yields `None`, leaving the declared zero to normal
/// rejection.
#[cfg(unix)]
fn dri3_buffer_size_from_descriptor(fd: impl std::os::fd::AsFd) -> Option<u32> {
    u32::try_from(rustix::fs::fstat(fd).ok()?.st_size)
        .ok()
        .filter(|size| *size != 0)
}

/// Whether a DRI3 modifier keeps its plane geometry opaque to this server.
///
/// Linear and `DRM_FORMAT_MOD_INVALID` stay on the descriptor validator's
/// existing packed-row contract. Every explicit modifier keeps opaque plane
/// geometry, which no row arithmetic here interprets.
#[cfg(unix)]
fn dri3_modifier_is_opaque(modifier: u64) -> bool {
    const DRM_FORMAT_MOD_LINEAR: u64 = 0;
    modifier != sophia_protocol::DRM_FORMAT_MOD_INVALID && modifier != DRM_FORMAT_MOD_LINEAR
}

/// The first plane offset no descriptor is known to contain.
///
/// Each offset must fall strictly inside a positive descriptor length within
/// the protocol cap; no height is applied and no extent inferred. `fstat`
/// bounds it without mapping the buffer or moving the shared file position,
/// and an unreadable length refuses normally.
#[cfg(unix)]
fn dri3_plane_offset_outside_descriptor(fds: &[OwnedFd], offsets: &[u32]) -> Option<u32> {
    fds.iter().zip(offsets).find_map(|(fd, offset)| {
        let within = rustix::fs::fstat(fd)
            .ok()
            .and_then(|metadata| u64::try_from(metadata.st_size).ok())
            .is_some_and(|size| {
                size != 0
                    && size <= sophia_protocol::DMA_BUF_MAX_BYTES
                    && u64::from(*offset) < size
            });
        (!within).then_some(*offset)
    })
}

/// Classify a failed read of a client's setup request.
///
/// A peer that closes before finishing the handshake is ordinary: port scans
/// do it, health checks do it, and a client that gives up mid-connect does it.
/// Saying so is what lets `reap_client_worker` retire the worker quietly.
/// Left unclassified, the service reads it as a server fault and propagates it,
/// ending every other client's session along with this one.
///
/// A read that failed for any other reason really is this server's problem and
/// keeps the unclassified form, so a genuine internal fault is not absorbed.
#[cfg(unix)]
fn setup_read_failure(stage: &str, error: &std::io::Error) -> X11SetupSocketError {
    let message = format!("failed to read X11 setup {stage}: {error}");
    if matches!(
        error.kind(),
        ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
    ) {
        X11SetupSocketError::client_disconnect(message)
    } else {
        X11SetupSocketError::new(message)
    }
}

/// The write counterpart of [`setup_read_failure`]: a peer that closed before
/// reading its answer has ended its own connection, not the service.
#[cfg(unix)]
fn setup_write_failure(stage: &str, error: &std::io::Error) -> X11SetupSocketError {
    let message = format!("failed to write X11 {stage}: {error}");
    if matches!(
        error.kind(),
        ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
    ) {
        X11SetupSocketError::client_disconnect(message)
    } else {
        X11SetupSocketError::new(message)
    }
}

#[cfg(unix)]
pub fn read_x11_setup_request(
    stream: &mut UnixStream,
) -> Result<XSetupRequest, X11SetupSocketError> {
    let mut bytes = vec![0; X_SETUP_CLIENT_PREFIX_LEN];
    stream
        .read_exact(&mut bytes)
        .map_err(|error| setup_read_failure("prefix", &error))?;
    // Bytes that arrived and do not parse are the client's error, not a fault
    // of this server: it must answer for them without taking anyone else down.
    let total_len = x11_setup_request_total_len(&bytes).map_err(|error| {
        X11SetupSocketError::client_failure(format!("invalid X11 setup prefix: {error}"))
    })?;
    bytes.resize(total_len, 0);
    stream
        .read_exact(&mut bytes[X_SETUP_CLIENT_PREFIX_LEN..])
        .map_err(|error| setup_read_failure("auth fields", &error))?;
    parse_x11_setup_request(&bytes).map_err(|error| {
        X11SetupSocketError::client_failure(format!("invalid X11 setup request: {error}"))
    })
}

/// Admit one X11 output record to this connection's wire.
///
/// The descriptors travel with the record's first bytes exactly once, and
/// whatever the kernel will not take now is owed in order rather than
/// waited for: see `X11ClientOutput`.
#[cfg(unix)]
pub fn write_x11_socket_output_record(
    output: &mut X11ClientOutput,
    record: X11SocketOutputRecord,
) -> std::io::Result<()> {
    let X11SocketOutputRecord { bytes, fds } = record;
    output.admit(bytes, fds)
}

#[cfg(unix)]
#[derive(Debug)]
pub struct X11ReceivedCoreRequest {
    pub major_opcode: u8,
    pub bytes: Vec<u8>,
    pub fds: Vec<OwnedFd>,
}

pub fn read_x11_core_request(
    stream: &mut UnixStream,
    byte_order: crate::XByteOrder,
) -> Result<Option<X11ReceivedCoreRequest>, X11SetupSocketError> {
    let mut header = [0; 4];
    let mut ancillary_space = [MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
    let mut ancillary = rustix::net::RecvAncillaryBuffer::new(&mut ancillary_space);
    let mut iov = [IoSliceMut::new(&mut header)];
    let received = match rustix::net::recvmsg(
        &*stream,
        &mut iov,
        &mut ancillary,
        rustix::net::RecvFlags::CMSG_CLOEXEC,
    ) {
        Ok(received) => received,
        Err(error) => {
            let error = std::io::Error::from(error);
            if matches!(
                error.kind(),
                ErrorKind::UnexpectedEof
                    | ErrorKind::ConnectionReset
                    | ErrorKind::TimedOut
                    | ErrorKind::WouldBlock
            ) {
                return Ok(None);
            }
            return Err(X11SetupSocketError::new(format!(
                "failed to read X11 request header: {error}"
            )));
        }
    };
    if received.bytes == 0 {
        return Ok(None);
    }
    if received.flags.contains(rustix::net::ReturnFlags::CTRUNC) {
        return Err(X11SetupSocketError::new(
            "X11 request carried too many ancillary file descriptors",
        ));
    }
    let mut fds = Vec::new();
    for message in ancillary.drain() {
        if let rustix::net::RecvAncillaryMessage::ScmRights(rights) = message {
            fds.extend(rights);
        }
    }
    if fds.len() > sophia_protocol::DMA_BUF_MAX_PLANES {
        return Err(X11SetupSocketError::new(
            "X11 request carried too many file descriptors",
        ));
    }
    if received.bytes < header.len() {
        match stream.read_exact(&mut header[received.bytes..]) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof
                        | ErrorKind::ConnectionReset
                        | ErrorKind::TimedOut
                        | ErrorKind::WouldBlock
                ) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(X11SetupSocketError::new(format!(
                    "failed to read X11 request header: {error}"
                )));
            }
        }
    }

    let length = usize::from(byte_order.u16(&header[2..4])) * 4;
    if length < 4 {
        return Ok(Some(X11ReceivedCoreRequest {
            major_opcode: header[0],
            bytes: header.to_vec(),
            fds,
        }));
    }
    // The setup reply advertises the full core u16 request-length range. Keep
    // the socket reader consistent with that wire contract: Firefox emits
    // large, but still ordinary, requests just below the 65,535-unit limit.
    // BIG-REQUESTS extended (zero u16 plus u32 length) frames remain outside
    // this bounded reader until a captured client requires them.
    let max_len = usize::from(crate::X_SETUP_DEFAULT_MAX_REQUEST_UNITS) * 4;
    if length > max_len {
        return Err(X11SetupSocketError::new(format!(
            "X11 request payload too large: {length}"
        )));
    }

    let mut request = Vec::with_capacity(length);
    request.extend_from_slice(&header);
    request.resize(length, 0);
    // The header read above already treats a vanished peer as an ordinary end
    // of stream. A client that announced a length and then left must be read
    // the same way, or abandoning a request part way through ends the service.
    stream
        .read_exact(&mut request[4..])
        .map_err(|error| setup_read_failure("request payload", &error))?;

    Ok(Some(X11ReceivedCoreRequest {
        major_opcode: header[0],
        bytes: request,
        fds,
    }))
}

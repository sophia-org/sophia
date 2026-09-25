impl XWireRequest {
    pub const fn required_fd_count(&self) -> usize {
        match self {
            Self::Dri3(XDri3Request::Dri3PixmapFromBuffer { .. })
            | Self::Dri3(XDri3Request::Dri3FenceFromFd { .. })
            // The segment's memory is the descriptor; without it there is
            // nothing to attach.
            | Self::Shm(XShmRequest::ShmAttachFd { .. }) => 1,
            Self::Dri3(XDri3Request::Dri3PixmapFromBuffers { num_buffers, .. }) => *num_buffers as usize,
            _ => 0,
        }
    }
}

fn decode_extension_query_version(
    context: XWireClientContext,
    bytes: &[u8],
    major_opcode: u8,
    minor_opcode: u8,
    request: impl FnOnce(u32, u32) -> XWireRequest,
) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(major_opcode, 12, bytes.len())?;
    if bytes[1] != minor_opcode {
        return Err(XWireParseError::UnknownOpcode(bytes[1]));
    }
    Ok(request(
        context.byte_order.u32(&bytes[4..8]),
        context.byte_order.u32(&bytes[8..12]),
    ))
}

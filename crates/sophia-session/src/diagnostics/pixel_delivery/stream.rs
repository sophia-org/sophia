use super::*;
use sophia_protocol::output_capture::OUTPUT_CAPTURE_RECORD_LEN;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Instant;

fn remaining(deadline: Instant) -> io::Result<std::time::Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "capture delivery deadline"))
}

fn send(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(invalid("capture delivery write stopped")),
            Ok(n) => bytes = &bytes[n..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Emits exactly one metadata/pixel/checksum record and shuts down its write
/// half. The caller must supply a private connected Unix socket, not a log.
pub fn write_frame(
    stream: &mut UnixStream,
    request: u64,
    info: OutputCaptureFrameInfo,
    pixels: &[u8],
) -> io::Result<()> {
    stream.peer_addr()?;
    if request == 0 || info.byte_len() != Some(pixels.len()) {
        return Err(invalid("invalid diagnostic capture frame"));
    }
    let deadline = Instant::now() + TIMEOUT;
    send(stream, MAGIC, deadline)?;
    send(stream, &request.to_le_bytes(), deadline)?;
    send(
        stream,
        &info.encode().ok_or_else(|| invalid("invalid frame info"))?,
        deadline,
    )?;
    send(stream, pixels, deadline)?;
    send(stream, &checksum(pixels).to_le_bytes(), deadline)?;
    stream.shutdown(Shutdown::Write)
}

#[cfg(feature = "native-session")]
fn exact(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        match stream.read(bytes) {
            Ok(0) => return Err(invalid("truncated capture delivery")),
            Ok(n) => bytes = &mut bytes[n..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(feature = "native-session")]
pub(super) fn read_frame(
    mut stream: UnixStream,
    output: OutputId,
    deadline: Instant,
) -> io::Result<Frame> {
    let mut header = [0; 16 + OUTPUT_CAPTURE_RECORD_LEN];
    exact(&mut stream, &mut header, deadline)?;
    let request = u64::from_le_bytes(header[8..16].try_into().unwrap());
    let info = OutputCaptureFrameInfo::decode(&header[16..])
        .ok_or_else(|| invalid("invalid capture metadata"))?;
    if &header[..8] != MAGIC || request == 0 || info.output != output {
        return Err(invalid("capture delivery identity mismatch"));
    }
    let mut pixels = vec![
        0;
        info.byte_len()
            .ok_or_else(|| invalid("capture size exceeds bound"))?
    ];
    exact(&mut stream, &mut pixels, deadline)?;
    let mut hash = [0; 8];
    exact(&mut stream, &mut hash, deadline)?;
    if u64::from_le_bytes(hash) != checksum(&pixels) {
        return Err(invalid("capture checksum mismatch"));
    }
    if !read_tail(stream, 0, deadline)?.is_empty() {
        return Err(invalid("capture has trailing bytes"));
    }
    Ok(Frame {
        request,
        info,
        pixels,
    })
}

#[cfg(feature = "native-session")]
pub(super) fn read_tail(
    mut stream: UnixStream,
    limit: usize,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let count = buffer.len().min(limit.saturating_sub(result.len()) + 1);
        match stream.read(&mut buffer[..count]) {
            Ok(0) => return Ok(result),
            Ok(n) if n > limit.saturating_sub(result.len()) => {
                return Err(invalid("diagnostic stream exceeds bound"));
            }
            Ok(n) => result.extend_from_slice(&buffer[..n]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

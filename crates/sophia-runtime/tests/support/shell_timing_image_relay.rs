//! A supplied launch relay for the sealed timing peer. It answers the
//! launch client's relay conversation with a success verdict for the one
//! challenge shell_timing_image_authority.rs serves. It is not Factotum and
//! proves no launch domain. Every read and write is bounded, and the thread
//! ends on EOF, error or timeout, so a failed child cannot strand it.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

type Result<T> = std::result::Result<T, String>;

struct Request {
    kind: u8,
    tag: u16,
    body: Vec<u8>,
}

fn next(stream: &mut UnixStream) -> Result<Option<Request>> {
    let mut size = [0; 4];
    match stream.read_exact(&mut size) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("relay read: {error}")),
    }
    let size = u32::from_le_bytes(size) as usize;
    if !(7..=4096).contains(&size) {
        return Err(format!("relay frame size {size}"));
    }
    let mut frame = vec![0; size - 4];
    stream
        .read_exact(&mut frame)
        .map_err(|error| format!("relay read: {error}"))?;
    Ok(Some(Request {
        kind: frame[0],
        tag: u16::from_le_bytes([frame[1], frame[2]]),
        body: frame[3..].to_vec(),
    }))
}

fn request(stream: &mut UnixStream, kind: u8) -> Result<Request> {
    let request = next(stream)?.ok_or("relay closed early")?;
    if request.kind != kind {
        return Err(format!("relay expected {kind}, got {}", request.kind));
    }
    Ok(request)
}

fn reply(stream: &mut UnixStream, request: &Request, kind: u8, body: &[u8]) -> Result<()> {
    let mut frame = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    frame.push(kind);
    frame.extend(request.tag.to_le_bytes());
    frame.extend(body);
    stream
        .write_all(&frame)
        .map_err(|error| format!("relay write: {error}"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "short relay request".into())
}

fn qid(kind: u8) -> Vec<u8> {
    let mut qid = vec![0; 13];
    qid[0] = kind;
    qid[5] = 1;
    qid
}

/// Serve one relay conversation with `step` per exchange, then wait at
/// most `finish` for the peer to close it.
pub fn serve(
    mut stream: UnixStream,
    challenge: [u8; 40],
    step: Duration,
    finish: Duration,
) -> Result<()> {
    for set in [UnixStream::set_read_timeout, UnixStream::set_write_timeout] {
        set(&stream, Some(step)).map_err(|error| error.to_string())?;
    }
    let version = request(&mut stream, 100)?;
    let mut body = version.body.get(..4).ok_or("short version")?.to_vec();
    body.extend(8u16.to_le_bytes());
    body.extend(b"9P2000.L");
    reply(&mut stream, &version, 101, &body)?;
    let attach = request(&mut stream, 104)?;
    let mut claims = Vec::new();
    for value in [b"".as_slice(), b"sophia-launch-relay-v1"] {
        claims.extend((value.len() as u16).to_le_bytes());
        claims.extend(value);
    }
    claims.extend(u32::MAX.to_le_bytes());
    if u32_at(&attach.body, 4)? != u32::MAX || attach.body.get(8..) != Some(&claims[..]) {
        return Err("relay attach claims".into());
    }
    let root = u32_at(&attach.body, 0)?;
    reply(&mut stream, &attach, 105, &qid(128))?;
    let walk = request(&mut stream, 110)?;
    if u32_at(&walk.body, 0)? != root || walk.body.get(8..) != Some(&[1, 0, 3, 0, b'r', b'p', b'c'])
    {
        return Err("relay walk".into());
    }
    let rpc = u32_at(&walk.body, 4)?;
    let mut body = 1u16.to_le_bytes().to_vec();
    body.extend(qid(0));
    reply(&mut stream, &walk, 111, &body)?;
    let open = request(&mut stream, 12)?;
    if open.body != [rpc.to_le_bytes(), 2u32.to_le_bytes()].concat() {
        return Err("relay open".into());
    }
    let mut body = qid(0);
    body.extend(0u32.to_le_bytes());
    reply(&mut stream, &open, 13, &body)?;
    let write = request(&mut stream, 118)?;
    if u32_at(&write.body, 0)? != rpc
        || write.body.get(4..12) != Some(&[0; 8])
        || u32_at(&write.body, 12)? != 40
        || write.body.get(16..) != Some(&challenge[..])
    {
        return Err("relay must carry only the served challenge".into());
    }
    reply(&mut stream, &write, 119, &40u32.to_le_bytes())?;
    let read = request(&mut stream, 116)?;
    if u32_at(&read.body, 0)? != rpc
        || read.body.get(4..12) != Some(&[0; 8])
        || u32_at(&read.body, 12)? != 8
    {
        return Err("relay status read".into());
    }
    let mut body = 8u32.to_le_bytes().to_vec();
    body.extend([1, 0, 0, 0, 0, 0, 0, 0]);
    reply(&mut stream, &read, 117, &body)?;
    for fid in [rpc, root] {
        let clunk = request(&mut stream, 120)?;
        if u32_at(&clunk.body, 0)? != fid {
            return Err("relay clunk order".into());
        }
        reply(&mut stream, &clunk, 121, &[])?;
    }
    // The peer keeps its relay pipeline until exit; nothing more may arrive.
    stream
        .set_read_timeout(Some(finish))
        .map_err(|error| error.to_string())?;
    match next(&mut stream)? {
        None => Ok(()),
        Some(request) => Err(format!("unexpected relay request {}", request.kind)),
    }
}

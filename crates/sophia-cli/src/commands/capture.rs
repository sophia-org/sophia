//! Output-only screenshot client of the admitted namespace portal.
use sophia_9p_client::{
    pipeline::{Pipeline, Reply},
    records::{Fid, Tag},
};
use sophia_protocol::{
    OutputId, namespace_view::NAMESPACE_DIRECTORY_ENV, output_capture::*, output_capture_request::*,
};
use std::{
    io::Write,
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn run(args: &[String]) -> Result<()> {
    if matches!(args, [arg] if arg == "--help" || arg == "-h") {
        println!(
            "sophia capture OUTPUT_ID [--output NEW_FILE.pam]\nRun through an admitted observer recipe with explicit capture-output permission.\nReads one RGBA frame and reports its output/presentation generation and checksum.\nAn optional PAM file requires a writable path in the admitted namespace."
        );
        return Ok(());
    }
    let (relay_ready, args) = if args.first().is_some_and(|arg| arg == "--relay-stdin") {
        (true, &args[1..])
    } else {
        (false, args)
    };
    let (output, destination) = parse(args)?;
    let directory = std::env::var_os(NAMESPACE_DIRECTORY_ENV)
        .ok_or("capture requires an admitted namespace launch")?;
    let relay = if relay_ready {
        UnixStream::from(rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3)?)
    } else {
        sophia_runtime::inherited_launch_channel()?
    };
    let names = sophia_namespace_client::Namespace::open(Path::new(&directory))?;
    let mut authentication = sophia_launch_client::Client::over(
        names.connect("roles/portal")?,
        relay,
        b"namespace-client",
        b"portal-client",
    )?;
    while !authentication.ready() {
        authentication.poll()?;
        authentication.wait_for_io(Duration::from_millis(10))?;
    }
    let mut admitted = authentication.into_authenticated()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let pipe = &mut admitted.role;
    let root = admitted.root;
    let api = read_named(pipe, root, b"api", 512, deadline)?;
    if !std::str::from_utf8(&api)?
        .split_whitespace()
        .any(|field| field == "request=output-capture-v1")
    {
        return Err("capture is not enabled for this recipient".into());
    }
    let previous = status(pipe, root, deadline)?;
    let serial = previous
        .request
        .map_or(0, |request| request.serial)
        .checked_add(1)
        .ok_or("capture serial exhausted")?;
    let request = OutputCaptureRequest { serial, output };
    let capture = opened(pipe, root, b"capture", 1, deadline)?;
    let bytes = request.encode().ok_or("invalid output capture request")?;
    let tag = pipe.write(capture, 0, &bytes)?;
    if answer(pipe, tag, deadline)? != Reply::Write(bytes.len() as u32) {
        return Err("capture request write refused".into());
    }
    close(pipe, capture, deadline)?;
    loop {
        let state = status(pipe, root, deadline)?;
        if state.request != Some(request) {
            return Err("capture request was replaced".into());
        }
        match state.state {
            OutputCaptureState::Ready => break,
            OutputCaptureState::Queued | OutputCaptureState::Capturing => {
                if Instant::now() >= deadline {
                    return Err("capture deadline".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return Err(format!("capture refused: {other:?}").into()),
        }
    }
    let record = opened(pipe, root, b"frame-info", 0, deadline)?;
    let bytes = read_all(pipe, record, OUTPUT_CAPTURE_RECORD_LEN, deadline)?;
    let info = OutputCaptureFrameInfo::decode(&bytes).ok_or("invalid capture frame record")?;
    if info.output != output {
        return Err("capture output mismatch".into());
    }
    let image = opened(pipe, root, b"frame", 0, deadline)?;
    let pixels = read_frame(
        pipe,
        image,
        info.byte_len().ok_or("capture size invalid")?,
        deadline,
    )?;
    if pixels.len() != info.byte_len().unwrap()
        || read_all(pipe, record, OUTPUT_CAPTURE_RECORD_LEN, deadline)? != bytes
    {
        return Err("capture changed or truncated during read".into());
    }
    close(pipe, image, deadline)?;
    close(pipe, record, deadline)?;
    if let Some(path) = destination {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        writeln!(
            file,
            "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR",
            info.width, info.height
        )?;
        file.write_all(&pixels)?;
    }
    let checksum = pixels.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    println!(
        "output={} output_generation={} presentation_generation={} width={} height={} bytes={} rgba_fnv1a64={checksum:016x}",
        output.raw(),
        info.output_generation,
        info.presentation_generation,
        info.width,
        info.height,
        pixels.len()
    );
    Ok(())
}

fn parse(args: &[String]) -> Result<(OutputId, Option<&str>)> {
    let (id, destination) = match args {
        [id] => (id, None),
        [id, flag, path] if flag == "--output" => (id, Some(path.as_str())),
        _ => return Err("usage: sophia capture OUTPUT_ID [--output NEW_FILE.pam]".into()),
    };
    let output = OutputId::from_raw(id.parse()?);
    if !output.is_valid() {
        return Err("output must be nonzero".into());
    }
    Ok((output, destination))
}
fn answer(pipe: &mut Pipeline, tag: Tag, deadline: Instant) -> Result<Reply> {
    let reply = pipe.wait(tag, deadline)?;
    if let Reply::Error(error) = reply {
        return Err(format!("portal refused operation: errno {}", error.0).into());
    }
    Ok(reply)
}
fn opened(
    pipe: &mut Pipeline,
    root: Fid,
    name: &[u8],
    flags: u32,
    deadline: Instant,
) -> Result<Fid> {
    let (tag, fid) = pipe.walk(root, &[name])?;
    if !matches!(answer(pipe, tag, deadline)?, Reply::Walk(qids) if qids.len() == 1) {
        return Err("capture file absent".into());
    }
    let tag = pipe.lopen(fid, flags)?;
    if !matches!(answer(pipe, tag, deadline)?, Reply::Lopen { .. }) {
        return Err("capture open refused".into());
    }
    Ok(fid)
}
fn close(pipe: &mut Pipeline, fid: Fid, deadline: Instant) -> Result<()> {
    let tag = pipe.clunk(fid)?;
    if !matches!(answer(pipe, tag, deadline)?, Reply::Clunk) {
        return Err("capture clunk reply mismatch".into());
    }
    Ok(())
}
fn read_all(pipe: &mut Pipeline, fid: Fid, limit: usize, deadline: Instant) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let count = (limit.saturating_sub(bytes.len()) + 1).min(32 * 1024) as u32;
        let tag = pipe.read(fid, bytes.len() as u64, count)?;
        let Reply::Read(part) = answer(pipe, tag, deadline)? else {
            return Err("capture read reply mismatch".into());
        };
        if part.is_empty() {
            return Ok(bytes);
        }
        if bytes.len() + part.len() > limit {
            return Err("capture file exceeds its bound".into());
        }
        bytes.extend_from_slice(&part);
    }
}
fn read_frame(pipe: &mut Pipeline, fid: Fid, length: usize, deadline: Instant) -> Result<Vec<u8>> {
    // Four replies fit the SDK's 256 KiB receive budget, including headers.
    // Retained delivery still checks the grant for every reply and socket write.
    const CHUNK: usize = 60 * 1024;
    const WINDOW: usize = 4;
    let mut bytes = Vec::with_capacity(length);
    while bytes.len() < length {
        let mut pending = Vec::with_capacity(WINDOW);
        let mut offset = bytes.len();
        for _ in 0..WINDOW {
            let count = (length - offset).min(CHUNK);
            if count == 0 {
                break;
            }
            pending.push((pipe.read(fid, offset as u64, count as u32)?, count));
            offset += count;
        }
        for (tag, count) in pending {
            let Reply::Read(part) = answer(pipe, tag, deadline)? else {
                return Err("capture read reply mismatch".into());
            };
            if part.len() != count {
                return Err("capture frame truncated".into());
            }
            bytes.extend_from_slice(&part);
        }
    }
    let tag = pipe.read(fid, length as u64, 1)?;
    if !matches!(answer(pipe, tag, deadline)?, Reply::Read(part) if part.is_empty()) {
        return Err("capture frame exceeds its bound".into());
    }
    Ok(bytes)
}
fn read_named(
    pipe: &mut Pipeline,
    root: Fid,
    name: &[u8],
    limit: usize,
    deadline: Instant,
) -> Result<Vec<u8>> {
    let fid = opened(pipe, root, name, 0, deadline)?;
    let bytes = read_all(pipe, fid, limit, deadline)?;
    close(pipe, fid, deadline)?;
    Ok(bytes)
}
fn status(pipe: &mut Pipeline, root: Fid, deadline: Instant) -> Result<OutputCaptureStatus> {
    OutputCaptureStatus::decode(&read_named(
        pipe,
        root,
        b"capture-status",
        CAPTURE_STATUS_LEN,
        deadline,
    )?)
    .ok_or_else(|| "invalid capture status".into())
}

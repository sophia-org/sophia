//! Opt-in wire checks against an explicitly admitted capture recipient.
use sophia_9p_client::{
    pipeline::{Pipeline, Reply},
    records::{Fid, Tag},
};
use sophia_protocol::{
    OutputId,
    namespace_view::NAMESPACE_DIRECTORY_ENV,
    output_capture::{OUTPUT_CAPTURE_RECORD_LEN, OutputCaptureFrameInfo},
    output_capture_request::{OutputCaptureRequest, OutputCaptureState, OutputCaptureStatus},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run(args: &[String]) -> Result<String> {
    let [output, mode] = args else {
        return Err(
            "capture-grant-probe OUTPUT_ID replacement|expiry (admitted launch only)".into(),
        );
    };
    if !matches!(mode.as_str(), "replacement" | "expiry") {
        return Err("unknown capture grant probe mode".into());
    }
    let output = OutputId::from_raw(output.parse()?);
    if !output.is_valid() {
        return Err("invalid output".into());
    }
    let directory =
        std::env::var_os(NAMESPACE_DIRECTORY_ENV).ok_or("admitted namespace required")?;
    let names = sophia_namespace_client::Namespace::open(Path::new(&directory))?;
    let mut auth = sophia_launch_client::Client::over(
        names.connect("roles/portal")?,
        sophia_runtime::inherited_launch_channel()?,
        b"namespace-client",
        b"portal-client",
    )?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !auth.ready() {
        if Instant::now() >= deadline {
            return Err("authentication timeout".into());
        }
        auth.poll()?;
        auth.wait_for_io(Duration::from_millis(10))?;
    }
    let mut admitted = auth.into_authenticated()?;
    let pipe = &mut admitted.role;
    let root = admitted.root;
    let previous = status(pipe, root, deadline)?;
    let serial = previous
        .request
        .map_or(0, |request| request.serial)
        .checked_add(1)
        .ok_or("serial exhausted")?;
    let initial_start = Instant::now();
    request(pipe, root, output, serial, deadline)?;
    let record = open(pipe, root, b"frame-info", 0, deadline)?;
    let old = frame_info(pipe, record, deadline)?;
    if old.output != output {
        return Err("capture returned another output".into());
    }
    let image = open(pipe, root, b"frame", 0, deadline)?;
    let tag = pipe.read(image, 0, 4)?;
    if !matches!(reply(pipe, tag, deadline)?, Reply::Read(bytes) if bytes.len() == 4) {
        return Err("initial frame read failed".into());
    }
    if mode == "replacement" {
        request(
            pipe,
            root,
            output,
            serial.checked_add(1).ok_or("serial exhausted")?,
            deadline,
        )?;
    } else {
        std::thread::sleep(Duration::from_millis(2100));
    }
    for fid in [record, image] {
        let tag = pipe.read(fid, 0, 4)?;
        if !matches!(reply(pipe, tag, deadline)?, Reply::Error(error) if error.0 == 116) {
            return Err("old capture fid did not return ESTALE".into());
        }
    }
    // Count through both stale replies, not merely through the replacement.
    if mode == "replacement" && initial_start.elapsed() >= Duration::from_millis(1500) {
        return Err("replacement too slow to exclude lease expiry".into());
    }
    if mode == "expiry" {
        request(
            pipe,
            root,
            output,
            serial.checked_add(1).ok_or("serial exhausted")?,
            deadline,
        )?;
    }
    let fresh = open(pipe, root, b"frame-info", 0, deadline)?;
    let new = frame_info(pipe, fresh, deadline)?;
    if old.output != new.output
        || old.output_generation != new.output_generation
        || new.presentation_generation <= old.presentation_generation
    {
        return Err("replacement did not select a fresh presentation of the same output".into());
    }
    let fresh_image = open(pipe, root, b"frame", 0, deadline)?;
    let tag = pipe.read(fresh_image, 0, 4)?;
    if !matches!(reply(pipe, tag, deadline)?, Reply::Read(bytes) if bytes.len() == 4) {
        return Err("new capture fid unreadable".into());
    }
    Ok(format!(
        "capture_grant_probe mode={mode} output={} generation={} old_presentation={} new_presentation={} old_record=ESTALE old_image=ESTALE new_image=readable result=pass",
        output.raw(),
        old.output_generation,
        old.presentation_generation,
        new.presentation_generation
    ))
}

fn reply(pipe: &mut Pipeline, tag: Tag, deadline: Instant) -> Result<Reply> {
    Ok(pipe.wait(tag, deadline)?)
}
fn open(pipe: &mut Pipeline, root: Fid, name: &[u8], flags: u32, deadline: Instant) -> Result<Fid> {
    let (tag, fid) = pipe.walk(root, &[name])?;
    if !matches!(reply(pipe, tag, deadline)?, Reply::Walk(qids) if qids.len() == 1) {
        return Err("walk refused".into());
    }
    let tag = pipe.lopen(fid, flags)?;
    if !matches!(reply(pipe, tag, deadline)?, Reply::Lopen { .. }) {
        return Err("open refused".into());
    }
    Ok(fid)
}
fn close(pipe: &mut Pipeline, fid: Fid, deadline: Instant) -> Result<()> {
    let tag = pipe.clunk(fid)?;
    if !matches!(reply(pipe, tag, deadline)?, Reply::Clunk) {
        return Err("clunk refused".into());
    }
    Ok(())
}
fn frame_info(pipe: &mut Pipeline, fid: Fid, deadline: Instant) -> Result<OutputCaptureFrameInfo> {
    let tag = pipe.read(fid, 0, OUTPUT_CAPTURE_RECORD_LEN as u32 + 1)?;
    let Reply::Read(bytes) = reply(pipe, tag, deadline)? else {
        return Err("record read refused".into());
    };
    OutputCaptureFrameInfo::decode(&bytes).ok_or_else(|| "invalid frame record".into())
}
fn request(
    pipe: &mut Pipeline,
    root: Fid,
    output: OutputId,
    serial: u64,
    deadline: Instant,
) -> Result<()> {
    let fid = open(pipe, root, b"capture", 1, deadline)?;
    let request = OutputCaptureRequest { output, serial };
    let bytes = request.encode().ok_or("invalid request")?;
    let tag = pipe.write(fid, 0, &bytes)?;
    let response = reply(pipe, tag, deadline)?;
    if response != Reply::Write(bytes.len() as u32) {
        return Err(format!("request refused: {response:?}").into());
    }
    close(pipe, fid, deadline)?;
    loop {
        if Instant::now() >= deadline {
            return Err("capture timeout".into());
        }
        let status = status(pipe, root, deadline)?;
        if status.request != Some(request) {
            return Err("request identity changed".into());
        }
        match status.state {
            OutputCaptureState::Ready => return Ok(()),
            OutputCaptureState::Queued | OutputCaptureState::Capturing => {
                std::thread::sleep(Duration::from_millis(10))
            }
            state => return Err(format!("capture refused: {state:?}").into()),
        }
    }
}

fn status(pipe: &mut Pipeline, root: Fid, deadline: Instant) -> Result<OutputCaptureStatus> {
    let fid = open(pipe, root, b"capture-status", 0, deadline)?;
    let tag = pipe.read(fid, 0, 33)?;
    let Reply::Read(bytes) = reply(pipe, tag, deadline)? else {
        return Err("status read refused".into());
    };
    close(pipe, fid, deadline)?;
    OutputCaptureStatus::decode(&bytes).ok_or_else(|| "invalid status".into())
}

//! Attended lock checks; no authority to lock or unlock the session.
use super::*;

struct Held {
    record: Fid,
    image: Fid,
    info: OutputCaptureFrameInfo,
    requested: Instant,
}

struct Interrupted {
    held: Held,
    next_serial: u64,
    pending_seen: bool,
    live_handle_revoked: bool,
}

pub(super) fn run(
    pipe: &mut Pipeline,
    root: Fid,
    output: OutputId,
    report: &mut impl FnMut(&str),
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut serial = status(pipe, root, deadline)?
        .request
        .map_or(0, |r| r.serial);
    let mut held: Option<Held> = None;
    let mut completed = 0;
    loop {
        check_deadline(deadline)?;
        serial = serial.checked_add(1).ok_or("serial exhausted")?;
        let requested = Instant::now();
        let response = submit(pipe, root, output, serial, deadline)?;
        if matches!(response, Reply::Error(error) if error.0 == 13) {
            return after_lock(
                pipe,
                root,
                output,
                Interrupted {
                    held: held.ok_or("locked before first frame")?,
                    next_serial: serial,
                    pending_seen: false,
                    live_handle_revoked: false,
                },
                deadline,
                report,
            );
        }
        if response != Reply::Write(24) {
            return Err(format!("capture request refused: {response:?}").into());
        }
        let mut pending_seen = false;
        loop {
            check_deadline(deadline)?;
            let state = status(pipe, root, deadline)?;
            if state.request != Some(OutputCaptureRequest { output, serial }) {
                return Err("request changed outside this probe".into());
            }
            match state.state {
                OutputCaptureState::Ready => break,
                OutputCaptureState::Queued | OutputCaptureState::Capturing => pending_seen = true,
                OutputCaptureState::Cancelled => {
                    return after_lock(
                        pipe,
                        root,
                        output,
                        Interrupted {
                            held: held
                                .ok_or("first request cancelled; repeat with an earlier start")?,
                            next_serial: serial.checked_add(1).ok_or("serial exhausted")?,
                            pending_seen,
                            live_handle_revoked: false,
                        },
                        deadline,
                        report,
                    );
                }
                state => return Err(format!("unexpected capture state {state:?}").into()),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let record = open(pipe, root, b"frame-info", 0, deadline)?;
        let info = frame_info(pipe, record, deadline)?;
        if info.output != output {
            return Err("wrong captured output".into());
        }
        let image = open(pipe, root, b"frame", 0, deadline)?;
        let tag = pipe.read(image, 0, 4)?;
        if !matches!(reply(pipe, tag, deadline)?, Reply::Read(bytes) if bytes.len() == 4) {
            return Err("lock raced initial read; repeat to retain a live frame".into());
        }
        if let Some(old) = held.replace(Held {
            record,
            image,
            info,
            requested,
        }) {
            close(pipe, old.record, deadline)?;
            close(pipe, old.image, deadline)?;
        }
        completed += 1;
        if completed == 1 {
            report("capture_grant_probe mode=lock stage=armed timeout_seconds=180");
        }
        // A new request itself revokes the previous binding. Only a stale read
        // with no intervening request can establish lock-triggered revocation.
        let reading_until = Instant::now() + Duration::from_millis(600);
        while Instant::now() < reading_until {
            let tag = pipe.read(held.as_ref().unwrap().image, 0, 4)?;
            match reply(pipe, tag, deadline)? {
                Reply::Read(bytes) if bytes.len() == 4 => {}
                Reply::Error(error) if error.0 == 116 => {
                    return after_lock(
                        pipe,
                        root,
                        output,
                        Interrupted {
                            held: held.take().unwrap(),
                            next_serial: serial.checked_add(1).ok_or("serial exhausted")?,
                            pending_seen: false,
                            live_handle_revoked: true,
                        },
                        deadline,
                        report,
                    );
                }
                other => return Err(format!("unexpected retained read: {other:?}").into()),
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn after_lock(
    pipe: &mut Pipeline,
    root: Fid,
    output: OutputId,
    interrupted: Interrupted,
    deadline: Instant,
    report: &mut impl FnMut(&str),
) -> Result<String> {
    let Interrupted {
        held,
        next_serial: serial,
        pending_seen,
        live_handle_revoked,
    } = interrupted;
    let cancelled = status(pipe, root, deadline)?;
    if cancelled.state != OutputCaptureState::Cancelled {
        return Err("denial without a cancelled request is not lock evidence".into());
    }
    let response = submit(pipe, root, output, serial, deadline)?;
    if !matches!(response, Reply::Error(error) if error.0 == 13) {
        return Err(format!("capture was not denied while locked: {response:?}").into());
    }
    stale(pipe, &held, deadline)?;
    let revocation_msec = held.requested.elapsed().as_millis();
    if revocation_msec >= 1500 {
        return Err("too slow to distinguish lock revocation from expiry".into());
    }
    report(&format!(
        "capture_grant_probe mode=lock stage=locked pending_before_cancel={pending_seen} live_handle_revoked={live_handle_revoked} old_record=ESTALE old_image=ESTALE request=EACCES elapsed_msec={revocation_msec}"
    ));
    let mut denied = 1;
    loop {
        check_deadline(deadline)?;
        std::thread::sleep(Duration::from_millis(100));
        if status(pipe, root, deadline)? != cancelled {
            return Err("cancelled request revived or changed before a new request".into());
        }
        let response = submit(pipe, root, output, serial, deadline)?;
        match response {
            Reply::Error(error) if error.0 == 13 => denied += 1,
            Reply::Write(24) => break,
            other => return Err(format!("unexpected post-lock request result: {other:?}").into()),
        }
    }
    // Admission has reopened. The old handles must still be stale.
    stale(pipe, &held, deadline)?;
    loop {
        check_deadline(deadline)?;
        let state = status(pipe, root, deadline)?;
        if state.request != Some(OutputCaptureRequest { output, serial }) {
            return Err("new request identity mismatch".into());
        }
        match state.state {
            OutputCaptureState::Ready => break,
            OutputCaptureState::Queued | OutputCaptureState::Capturing => {
                std::thread::sleep(Duration::from_millis(10))
            }
            other => return Err(format!("new capture failed: {other:?}").into()),
        }
    }
    let record = open(pipe, root, b"frame-info", 0, deadline)?;
    let info = frame_info(pipe, record, deadline)?;
    if info.output != output
        || info.output_generation != held.info.output_generation
        || info.presentation_generation <= held.info.presentation_generation
    {
        return Err("unlock did not provide an independent fresh capture".into());
    }
    let image = open(pipe, root, b"frame", 0, deadline)?;
    let tag = pipe.read(image, 0, 4)?;
    if !matches!(reply(pipe, tag, deadline)?, Reply::Read(bytes) if bytes.len() == 4) {
        return Err("new frame unreadable after unlock".into());
    }
    stale(pipe, &held, deadline)?;
    Ok(format!(
        "capture_grant_probe mode=lock output={} pending_before_cancel={pending_seen} live_handle_revoked={live_handle_revoked} denied_requests={denied} old_presentation={} new_presentation={} old_handles=ESTALE new_image=readable result=pass",
        output.raw(),
        held.info.presentation_generation,
        info.presentation_generation
    ))
}

fn submit(
    pipe: &mut Pipeline,
    root: Fid,
    output: OutputId,
    serial: u64,
    deadline: Instant,
) -> Result<Reply> {
    let fid = open(pipe, root, b"capture", 1, deadline)?;
    let bytes = OutputCaptureRequest { output, serial }
        .encode()
        .ok_or("invalid request")?;
    let tag = pipe.write(fid, 0, &bytes)?;
    let response = reply(pipe, tag, deadline)?;
    close(pipe, fid, deadline)?;
    Ok(response)
}
fn stale(pipe: &mut Pipeline, held: &Held, deadline: Instant) -> Result<()> {
    for fid in [held.record, held.image] {
        let tag = pipe.read(fid, 0, 4)?;
        if !matches!(reply(pipe, tag, deadline)?, Reply::Error(error) if error.0 == 116) {
            return Err("retained capture handle did not return ESTALE".into());
        }
    }
    Ok(())
}
fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err("attended lock probe timed out".into());
    }
    Ok(())
}

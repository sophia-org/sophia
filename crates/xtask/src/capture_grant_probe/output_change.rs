//! Attended output replacement, excluding lease expiry and request replacement.
use super::*;

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
    let mut armed = false;
    let (record, image, old) = 'observe: loop {
        check(deadline)?;
        serial = serial.checked_add(1).ok_or("serial exhausted")?;
        let started = Instant::now();
        request(pipe, root, output, serial, deadline)?;
        let record = open(pipe, root, b"frame-info", 0, deadline)?;
        let old = frame_info(pipe, record, deadline)?;
        if old.output != output {
            return Err("wrong captured output".into());
        }
        let image = open(pipe, root, b"frame", 0, deadline)?;
        let mut readable = false;
        while started.elapsed() < Duration::from_millis(1000) {
            let tag = pipe.read(image, 0, 4)?;
            match reply(pipe, tag, deadline)? {
                Reply::Read(bytes) if bytes.len() == 4 => {
                    readable = true;
                    if !armed {
                        report(
                            "capture_grant_probe mode=output-change stage=armed timeout_seconds=180",
                        );
                        armed = true;
                    }
                }
                Reply::Error(error) if error.0 == 116 && readable => {
                    stale(pipe, record, image, deadline)?;
                    if started.elapsed() >= Duration::from_millis(1500) {
                        return Err("staleness could be lease expiry".into());
                    }
                    report(&format!(
                        "capture_grant_probe mode=output-change stage=withdrawn generation={} elapsed_msec={} no_intervening_request=true",
                        old.output_generation,
                        started.elapsed().as_millis()
                    ));
                    break 'observe (record, image, old);
                }
                other => {
                    return Err(
                        format!("output change raced initial read or refused: {other:?}").into(),
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        close(pipe, record, deadline)?;
        close(pipe, image, deadline)?;
    };
    let mut unavailable = 0;
    loop {
        check(deadline)?;
        serial = serial.checked_add(1).ok_or("serial exhausted")?;
        let fid = open(pipe, root, b"capture", 1, deadline)?;
        let request = OutputCaptureRequest { output, serial };
        let bytes = request.encode().ok_or("invalid request")?;
        let tag = pipe.write(fid, 0, &bytes)?;
        let response = reply(pipe, tag, deadline)?;
        close(pipe, fid, deadline)?;
        if response != Reply::Write(24) {
            return Err(format!("output recovery request refused: {response:?}").into());
        }
        let terminal = loop {
            check(deadline)?;
            let state = status(pipe, root, deadline)?;
            if state.request != Some(request) {
                return Err("request replaced outside probe".into());
            }
            match state.state {
                OutputCaptureState::Queued | OutputCaptureState::Capturing => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                terminal => break terminal,
            }
        };
        stale(pipe, record, image, deadline)?;
        match terminal {
            OutputCaptureState::Unavailable | OutputCaptureState::Cancelled => {
                unavailable += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            OutputCaptureState::Ready => {
                let fresh_record = open(pipe, root, b"frame-info", 0, deadline)?;
                let fresh = frame_info(pipe, fresh_record, deadline)?;
                if fresh.output != output || fresh.output_generation <= old.output_generation {
                    return Err("capture returned without a new output incarnation".into());
                }
                let fresh_image = open(pipe, root, b"frame", 0, deadline)?;
                let tag = pipe.read(fresh_image, 0, 4)?;
                if !matches!(reply(pipe, tag, deadline)?, Reply::Read(bytes) if bytes.len() == 4) {
                    return Err("replacement image unreadable".into());
                }
                stale(pipe, record, image, deadline)?;
                return Ok(format!(
                    "capture_grant_probe mode=output-change output={} old_generation={} new_generation={} unavailable_requests={unavailable} old_handles=ESTALE new_image=readable result=pass",
                    output.raw(),
                    old.output_generation,
                    fresh.output_generation
                ));
            }
            other => return Err(format!("unexpected recovery state: {other:?}").into()),
        }
    }
}

fn stale(pipe: &mut Pipeline, record: Fid, image: Fid, deadline: Instant) -> Result<()> {
    for fid in [record, image] {
        let tag = pipe.read(fid, 0, 4)?;
        if !matches!(reply(pipe, tag, deadline)?, Reply::Error(error) if error.0 == 116) {
            return Err("old output capture handle is not stale".into());
        }
    }
    Ok(())
}
fn check(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err("attended output change timed out".into());
    }
    Ok(())
}

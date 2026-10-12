//! An explicitly admitted administrative client of the developer-launch tree.
use sophia_9p_client::{
    pipeline::{Pipeline, PipelineLimits, Reply},
    records::{Fid, Tag},
};
use sophia_protocol::developer_launch::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn run(args: &[String]) -> Result<()> {
    if matches!(args, [arg] if arg == "--help" || arg == "-h") {
        println!(
            "sophia launch --socket ABSOLUTE_PATH --recipe NAME -- /absolute/program ARG...\nRequires explicit host-admin policy and developer-launch permission on the recipe.\nSubmission is not readiness; never automatically retry an indeterminate outcome."
        );
        return Ok(());
    }
    let (socket, request) = parse(args)?;
    let bytes = encode_developer_launch_request(&request)?;
    let mut client = Pipeline::connect(&socket, PipelineLimits::default(), Duration::from_secs(3))?;
    let (tag, root) = client.attach(b"", b"")?;
    if !matches!(answer(&mut client, tag)?, Reply::Attach(_)) {
        return Err("launch attach reply mismatch".into());
    }
    let api = opened(&mut client, root, b"api", 0)?;
    if read_all(&mut client, api, 512)? != DEVELOPER_LAUNCH_API.as_bytes() {
        return Err("not a developer-launch endpoint".into());
    }
    let candidate = opened(&mut client, root, b"request", 1)?;
    let submit = opened(&mut client, root, b"submit", 1)?;
    let tag = client.write(candidate, 0, &bytes)?;
    if answer(&mut client, tag)? != Reply::Write(bytes.len() as u32) {
        return Err("incomplete launch candidate write".into());
    }
    // Once submit is queued, a missing wire reply cannot prove that Session
    // did not accept it. Do not reconnect or replay on a fresh connection.
    let outcome = (|| -> Result<DeveloperLaunchResult> {
        let tag = client.write(submit, 0, &request.request.to_le_bytes())?;
        if answer(&mut client, tag)? != Reply::Write(4) {
            return Err("incomplete launch submission".into());
        }
        let (tag, result) = client.walk(root, &[b"result"])?;
        if !matches!(answer(&mut client, tag)?, Reply::Walk(qids) if qids.len()==1) {
            return Err("launch result walk mismatch".into());
        }
        let deadline = Instant::now() + Duration::from_secs(32);
        loop {
            let tag = client.lopen(result, 0)?;
            match client.wait(tag, deadline.min(Instant::now() + Duration::from_secs(3)))? {
                Reply::Lopen { .. } => break,
                Reply::Error(error) if error.0 == 11 && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                reply => return Err(format!("launch result unavailable: {reply:?}").into()),
            }
        }
        let result = decode_developer_launch_result(&read_all(
            &mut client,
            result,
            DEVELOPER_LAUNCH_MAX_RESULT,
        )?)?;
        if result.request != request.request {
            return Err("launch result correlation mismatch".into());
        }
        Ok(result)
    })()
    .map_err(|error| format!("launch outcome unknown ({error}); do not retry automatically"))?;
    println!(
        "{}",
        String::from_utf8(encode_developer_launch_result(&outcome)?)?
    );
    if outcome.status != DeveloperLaunchStatus::Started {
        return Err(format!("launch did not report readiness: {:?}", outcome.status).into());
    }
    Ok(())
}
fn parse(args: &[String]) -> Result<(PathBuf, DeveloperLaunchRequest)> {
    let mut socket = None;
    let mut recipe = None;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            index += 1;
            break;
        }
        let slot = match arg.as_str() {
            "--socket" => &mut socket,
            "--recipe" => &mut recipe,
            _ => return Err(format!("unknown launch option {arg}").into()),
        };
        if slot.is_some() {
            return Err(format!("duplicate launch option {arg}").into());
        }
        index += 1;
        *slot = Some(
            args.get(index)
                .ok_or("missing launch option value")?
                .clone(),
        );
        index += 1;
    }
    let socket = PathBuf::from(socket.ok_or("--socket is required")?);
    if !socket.is_absolute() {
        return Err("launch socket must be absolute".into());
    }
    let executable = args
        .get(index)
        .ok_or("-- /absolute/program is required")?
        .clone();
    let request = DeveloperLaunchRequest {
        version: 1,
        request: 1,
        recipe: recipe.ok_or("--recipe is required")?,
        executable,
        arguments: args[index + 1..].to_vec(),
    };
    encode_developer_launch_request(&request)?;
    Ok((socket, request))
}
fn answer(client: &mut Pipeline, tag: Tag) -> Result<Reply> {
    let reply = client.wait(tag, Instant::now() + Duration::from_secs(3))?;
    if let Reply::Error(error) = reply {
        return Err(format!("launch server errno {}", error.0).into());
    }
    Ok(reply)
}
fn opened(client: &mut Pipeline, root: Fid, name: &[u8], flags: u32) -> Result<Fid> {
    let (tag, fid) = client.walk(root, &[name])?;
    if !matches!(answer(client,tag)?,Reply::Walk(qids) if qids.len()==1) {
        return Err("launch walk mismatch".into());
    }
    let tag = client.lopen(fid, flags)?;
    if !matches!(answer(client, tag)?, Reply::Lopen { .. }) {
        return Err("launch open mismatch".into());
    }
    Ok(fid)
}
fn read_all(client: &mut Pipeline, fid: Fid, bound: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let tag = client.read(fid, bytes.len() as u64, bound as u32)?;
        let Reply::Read(part) = client.wait(tag, deadline)? else {
            return Err("launch read mismatch".into());
        };
        if part.is_empty() {
            return Ok(bytes);
        }
        if bytes.len() + part.len() > bound {
            return Err("launch record exceeds bound".into());
        }
        bytes.extend(part);
    }
}

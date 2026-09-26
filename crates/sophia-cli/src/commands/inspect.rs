//! Read-only client of the separate, explicitly enabled host observation tree.
mod options;
mod render;

use options::{Operation, Options};
use sophia_9p::client::{Client, ClientLimits, File};
use sophia_protocol::inspection::*;
use std::error::Error;
use std::io::{Write, stdout};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const NAMES: [&str; 4] = ["api", "status", "snapshot", "events"];

pub(super) fn run(args: &[String]) -> Result<()> {
    if matches!(args, [arg] if arg == "--help" || arg == "-h")
        || matches!(args, [wm, arg] if wm == "wm" && (arg == "--help" || arg == "-h"))
    {
        println!(
            "sophia inspect wm [--socket ABSOLUTE_PATH] [--json] ls|stat PATH|status|snapshot|watch\nRequires session inspection \"host-admin\"; discovery never enables it.\nwatch starts with a snapshot and fails on a gap or revocation; reopen explicitly."
        );
        return Ok(());
    }
    let options = Options::parse(args)?;
    let mut client = Client::connect(&options.socket, ClientLimits::default())?;
    let root = client.attach(b"", b"")?;
    let api = read_object(&mut client, &root, "api")?;
    if api != inspection_api_text().as_bytes() {
        return Err("endpoint does not implement sophia_wm_inspection_v1".into());
    }
    let mut out = stdout().lock();
    match options.operation {
        Operation::Ls => {
            let mut directory = client.walk(&root, &[])?;
            client.open(&mut directory, true)?;
            let entries = client.list_all(&directory, NAMES.len())?;
            if !entries
                .iter()
                .map(|entry| entry.name.as_slice())
                .eq(NAMES.map(str::as_bytes))
            {
                return Err("inspection root vocabulary changed".into());
            }
            render::listing(&mut out, &entries, options.json)?;
            client.clunk(directory)?;
        }
        Operation::Stat(name) => {
            let file = if name == "/" {
                client.walk(&root, &[])?
            } else {
                client.walk(&root, &[name.as_bytes()])?
            };
            let attr = client.getattr(&file)?;
            render::stat(&mut out, &name, &attr, options.json)?;
            client.clunk(file)?;
        }
        Operation::Status => {
            let status = decode_inspection_status(&read_object(&mut client, &root, "status")?)?;
            render::status(&mut out, &status, options.json)?;
        }
        Operation::Snapshot => {
            let snapshot =
                decode_inspection_snapshot(&read_object(&mut client, &root, "snapshot")?)?;
            render::snapshot(&mut out, &snapshot, options.json)?;
        }
        Operation::Watch => {
            if let Err(error) = watch(&mut client, &root, &mut out, options.json) {
                return Err(format!(
                    "inspection watch stopped ({error}); reopen explicitly after a gap or revocation"
                )
                .into());
            }
        }
    }
    out.flush()?;
    client.clunk(root)?;
    Ok(())
}

fn read_object(client: &mut Client, root: &File, name: &str) -> Result<Vec<u8>> {
    let mut file = client.walk(root, &[name.as_bytes()])?;
    client.open(&mut file, false)?;
    let before = client.getattr(&file)?;
    if before.size > INSPECTION_MAX_SNAPSHOT_BYTES as u64 {
        return Err("inspection object exceeds its bound".into());
    }
    // One object budget, not a fresh timeout for every short fragment.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    loop {
        let remaining = (before.size as usize).saturating_sub(bytes.len());
        let count = remaining.saturating_add(1).min(client.msize() as usize) as u32;
        let chunk = client
            .read_until(&file, bytes.len() as u64, count, deadline)?
            .ok_or("inspection object deadline expired")?;
        if chunk.is_empty() {
            break;
        }
        if bytes.len().saturating_add(chunk.len()) > before.size as usize {
            return Err("inspection object exceeds its pinned size".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let after = client.getattr(&file)?;
    if before.qid != after.qid || before.size != after.size || bytes.len() as u64 != before.size {
        return Err("inspection object changed during read".into());
    }
    client.clunk(file)?;
    Ok(bytes)
}

fn watch(client: &mut Client, root: &File, out: &mut impl Write, json: bool) -> Result<()> {
    let snapshot = decode_inspection_snapshot(&read_object(client, root, "snapshot")?)?;
    let mut events = client.walk(root, &[b"events"])?;
    client.open(&mut events, false)?;
    let mut cursor = snapshot.event_offset;
    let mut sequence = snapshot.sequence;
    let mut assembly = Vec::new();
    let mut assembly_deadline = None;
    render::snapshot(out, &snapshot, json)?;
    out.flush()?;
    loop {
        // Clean flush permits bounded idle waits without advancing the cursor;
        // the server still owns authorization and loss detection on each retry.
        let poll_deadline = Instant::now() + Duration::from_secs(1);
        let deadline =
            assembly_deadline.map_or(poll_deadline, |cap: Instant| cap.min(poll_deadline));
        let Some(bytes) = client.read_until(&events, cursor, 64 * 1024, deadline)? else {
            if assembly_deadline.is_some_and(|cap| Instant::now() >= cap) {
                return Err("inspection event assembly deadline expired".into());
            }
            continue;
        };
        if bytes.is_empty() {
            return Err("inspection watch ended; reopen explicitly".into());
        }
        cursor = cursor
            .checked_add(bytes.len() as u64)
            .ok_or("inspection cursor exhausted")?;
        if assembly.len().saturating_add(bytes.len()) > INSPECTION_MAX_RING_BYTES {
            return Err("inspection event exceeds its bound".into());
        }
        assembly.extend_from_slice(&bytes);
        assembly_deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
        while let Some(end) = assembly.iter().position(|byte| *byte == b'\n') {
            let event = decode_inspection_event(&assembly[..=end])?;
            if event.generation != snapshot.generation
                || event.loss_generation != snapshot.loss_generation
                || sequence.checked_add(1) != Some(event.sequence)
            {
                return Err(
                    "inspection watch has a gap or changed generation; reopen explicitly".into(),
                );
            }
            render::event(out, &event, json)?;
            sequence = event.sequence;
            assembly.drain(..=end);
            assembly_deadline = if assembly.is_empty() {
                None
            } else {
                Some(Instant::now() + Duration::from_secs(5))
            };
        }
        out.flush()?;
    }
}

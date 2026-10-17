//! Counts returned byte lengths from a dedicated strace data-transfer pass.
//! This is socket-boundary accounting, not an estimate of internal buffer copies.
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Count {
    read: u64,
    written: u64,
    calls: u64,
}

pub fn run(args: &[String]) -> Result<Vec<String>, String> {
    let [path] = args else {
        return Err("expected broker-socket-bytes /absolute/strace.log".into());
    };
    if !std::path::Path::new(path).is_absolute() {
        return Err("trace path must be absolute".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(vec![parse(&text)?.to_string()])
}

fn parse(text: &str) -> Result<serde_json::Value, String> {
    let mut pending = BTreeMap::<u32, String>::new();
    let mut counts = BTreeMap::<u32, Count>::new();
    let mut seen = BTreeSet::new();
    let mut ended = BTreeSet::new();
    for (line_number, line) in text.lines().enumerate() {
        let (pid, body) = line
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("line {} lacks pid", line_number + 1))?;
        let pid: u32 = pid
            .parse()
            .map_err(|_| format!("line {} has invalid pid", line_number + 1))?;
        if pid == 0 || ended.contains(&pid) {
            return Err("invalid or reused trace pid".into());
        }
        seen.insert(pid);
        let body = body.trim();
        if body.starts_with("+++ exited with ") || body.starts_with("+++ killed by ") {
            if pending.contains_key(&pid) {
                return Err(format!("pid {pid} ended with an unreturned syscall"));
            }
            ended.insert(pid);
            continue;
        }
        if body.starts_with("--- ") {
            continue;
        }
        let complete = if let Some(tail) = body.strip_prefix("<... ") {
            let (name, rest) = tail
                .split_once(" resumed>")
                .ok_or("malformed resumed call")?;
            let prefix = pending.remove(&pid).ok_or("resumed call without a start")?;
            if !prefix.starts_with(&format!("{name}(")) {
                return Err("resumed syscall name differs".into());
            }
            format!("{prefix}{rest}")
        } else if let Some(prefix) = body.strip_suffix("<unfinished ...>") {
            if pending.insert(pid, prefix.to_owned()).is_some() {
                return Err("overlapping syscalls for one task".into());
            }
            continue;
        } else {
            if pending.contains_key(&pid) {
                return Err("missing resumed syscall".into());
            }
            body.to_owned()
        };
        let (name, arguments) = complete.split_once('(').ok_or("malformed syscall")?;
        let read = match name {
            "read" | "readv" | "recvfrom" | "recvmsg" => true,
            "write" | "writev" | "sendto" | "sendmsg" => false,
            _ => {
                return Err(format!(
                    "unexpected syscall {name}; use the documented transfer-only trace filter"
                ));
            }
        };
        let (_, returned) = arguments
            .rsplit_once(" = ")
            .ok_or("syscall has no returned length")?;
        let value: i64 = returned
            .split_whitespace()
            .next()
            .ok_or("missing result")?
            .parse()
            .map_err(|_| "indeterminate syscall result")?;
        if value < -1 {
            return Err("invalid negative transfer result".into());
        }
        // Descriptor annotation must be the first argument, not a string in data.
        let fd = arguments.split_once('>').map(|(fd, _)| fd).unwrap_or("");
        if !fd.contains("<UNIX-STREAM:[") {
            continue;
        }
        let count = counts.entry(pid).or_default();
        count.calls = count.calls.checked_add(1).ok_or("call count overflow")?;
        let bytes = u64::try_from(value).unwrap_or(0);
        let total = if read {
            &mut count.read
        } else {
            &mut count.written
        };
        *total = total.checked_add(bytes).ok_or("byte count overflow")?;
    }
    if !pending.is_empty() || seen.is_empty() || seen != ended {
        return Err("incomplete trace".into());
    }
    if counts.is_empty() {
        return Err("trace contains no annotated Unix-stream transfers".into());
    }
    let tasks: Vec<_> = counts.into_iter().map(|(pid, c)| json!({"pid":pid,"read_bytes":c.read,"written_bytes":c.written,"calls":c.calls})).collect();
    Ok(
        json!({"schema":1,"scope":"returned Unix-stream read/write bytes, full traced process lifetime including startup and teardown",
        "internal_buffer_copies":"unmeasured","qualification":false,"tasks":tasks}),
    )
}

#[path = "../tests/support/broker_socket_bytes.rs"]
mod tests;

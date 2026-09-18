//! Versioned binary records; arbitrary stderr bytes never enter structured history.
use super::{PACKET_BYTES, Shared};
use crate::diagnostics::storage::{Directory, invalid};
use rustix::fs::OFlags;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver, RecvTimeoutError},
};
use std::time::{Duration, Instant};

const SEGMENT_BYTES: u64 = 4 * 1024 * 1024;
const SEGMENTS: usize = 4;
static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(super) struct Packet {
    bytes: [u8; PACKET_BYTES],
    len: usize,
}

impl Packet {
    pub const HEADER: usize = 36;

    pub fn new(kind: u32, launch: u64, offset: u64, body: &[u8]) -> Self {
        assert!(body.len() <= PACKET_BYTES - Self::HEADER);
        let mut bytes = [0; PACKET_BYTES];
        bytes[..4].copy_from_slice(b"ASD1");
        bytes[4..8].copy_from_slice(&kind.to_le_bytes());
        bytes[8..16].copy_from_slice(&launch.to_le_bytes());
        bytes[16..24].copy_from_slice(&offset.to_le_bytes());
        bytes[24..32].copy_from_slice(
            &RECORD_SEQUENCE
                .fetch_add(1, Ordering::Relaxed)
                .to_le_bytes(),
        );
        bytes[32..36].copy_from_slice(&(body.len() as u32).to_le_bytes());
        bytes[Self::HEADER..Self::HEADER + body.len()].copy_from_slice(body);
        Self {
            bytes,
            len: Self::HEADER + body.len(),
        }
    }
}

pub(super) fn store(directory: Directory, receiver: Receiver<Packet>, shared: &Shared) {
    let mut index = 0;
    let mut size = 0u64;
    let mut rotation = 0u64;
    let mut last_sync = Instant::now();
    loop {
        let packet = match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(packet) => Some(packet),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if last_sync.elapsed() >= Duration::from_secs(1) {
            if health(&directory, shared, index, rotation, false).is_err() {
                shared.storage_errors.fetch_add(1, Ordering::Relaxed);
            }
            last_sync = Instant::now();
        }
        let Some(packet) = packet else { continue };
        let result = (|| -> io::Result<()> {
            let _lock = directory.lock()?;
            if size + packet.len as u64 > SEGMENT_BYTES {
                index = (index + 1) % SEGMENTS;
                size = 0;
                rotation += 1;
            }
            let mut file = directory.file(
                &format!("application-stderr.{index}.bin"),
                OFlags::CREATE | OFlags::WRONLY | OFlags::APPEND,
            )?;
            if size == 0 {
                file.set_len(0)?;
            }
            // On a partial failed write, discard that suffix before the next record.
            file.set_len(size)?;
            file.write_all(&packet.bytes[..packet.len])?;
            size += packet.len as u64;
            Ok(())
        })();
        if result.is_err() {
            if shared.storage_errors.fetch_add(1, Ordering::Relaxed) == 0 {
                crate::diagnostics::capture_line(
                    "sophia_application_capture schema=1 status=incomplete reason=storage_failure",
                );
            }
            shared
                .storage_dropped
                .fetch_add(packet.len as u64, Ordering::Relaxed);
        }
    }
    if health(&directory, shared, index, rotation, true).is_err() {
        crate::diagnostics::capture_line(
            "sophia_application_capture schema=1 status=incomplete reason=storage_failure",
        );
    }
}

fn health(
    directory: &Directory,
    shared: &Shared,
    index: usize,
    rotation: u64,
    stopped: bool,
) -> io::Result<()> {
    let _lock = directory.lock()?;
    let mut synchronized = true;
    for segment in 0..SEGMENTS {
        match directory.sync(&format!("application-stderr.{segment}.bin")) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => synchronized = false,
        }
    }
    directory.replace("application-health", &format!(
            "schema=1\nrotations={rotation}\nnewest_segment={index}\nrefused={}\nstorage_errors={}\nstorage_dropped_bytes={}\nmetadata_lost={}\ncollector_stopped={stopped}\nsynchronized={synchronized}\nobserved_boot_msec={}\n",
            shared.refused.load(Ordering::Relaxed), shared.storage_errors.load(Ordering::Relaxed),
            shared.storage_dropped.load(Ordering::Relaxed), shared.metadata_lost.load(Ordering::Relaxed),
            crate::diagnostics::Stamp::now().boot_msec,
        ))
}

#[derive(Default)]
pub struct ApplicationRecords {
    pub launches: BTreeMap<u64, String>,
    /// Kept chunks include original stream offsets; missing ranges are never synthesized.
    pub chunks: Vec<(u64, u64, Vec<u8>)>,
    pub incomplete_tail: bool,
    pub health: String,
}

pub(in crate::diagnostics) fn read_records(
    directory: &Directory,
) -> io::Result<ApplicationRecords> {
    let mut records = Vec::new();
    let mut incomplete_tail = false;
    for index in 0..SEGMENTS {
        let bytes =
            match directory.read_bytes(&format!("application-stderr.{index}.bin"), SEGMENT_BYTES) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            if rest.len() < Packet::HEADER {
                incomplete_tail = true;
                break;
            }
            if &rest[..4] != b"ASD1" {
                return Err(invalid("invalid application stderr framing"));
            }
            let kind = u32::from_le_bytes(rest[4..8].try_into().unwrap());
            let id = u64::from_le_bytes(rest[8..16].try_into().unwrap());
            let offset = u64::from_le_bytes(rest[16..24].try_into().unwrap());
            let time = u64::from_le_bytes(rest[24..32].try_into().unwrap());
            let len = u32::from_le_bytes(rest[32..36].try_into().unwrap()) as usize;
            if id == 0 || !matches!(kind, 1 | 2) || len > PACKET_BYTES - Packet::HEADER {
                return Err(invalid("invalid application stderr record"));
            }
            if rest.len() < Packet::HEADER + len {
                incomplete_tail = true;
                break;
            }
            records.push((
                time,
                kind,
                id,
                offset,
                rest[Packet::HEADER..Packet::HEADER + len].to_vec(),
            ));
            rest = &rest[Packet::HEADER + len..];
        }
    }
    records.sort_by_key(|record| record.0);
    let mut result = ApplicationRecords {
        incomplete_tail,
        health: match directory.read("application-health", 4096) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                "status=health_not_yet_observed\n".into()
            }
            Err(error) => return Err(error),
        },
        ..Default::default()
    };
    for (_, kind, id, offset, bytes) in records {
        if kind == 2 {
            let text = String::from_utf8(bytes).map_err(|_| invalid("invalid launch metadata"))?;
            if text
                .bytes()
                .any(|byte| byte.is_ascii_control() && byte != b'\n')
            {
                return Err(invalid("unsafe launch metadata"));
            }
            result.launches.insert(id, text);
        } else {
            result.chunks.push((id, offset, bytes));
        }
    }
    result.chunks.sort_by_key(|(id, offset, _)| (*id, *offset));
    Ok(result)
}

pub fn escape_bytes(bytes: &[u8]) -> String {
    let mut result = String::new();
    for &byte in bytes {
        match byte {
            b' '..=b'~' if byte != b'\\' => result.push(char::from(byte)),
            b'\n' => result.push_str("\\n"),
            b'\r' => result.push_str("\\r"),
            b'\t' => result.push_str("\\t"),
            b'\\' => result.push_str("\\\\"),
            _ => {
                use std::fmt::Write;
                let _ = write!(result, "\\x{byte:02x}");
            }
        }
    }
    result
}

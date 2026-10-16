//! Typed evidence avoids constructing an allocating JSON value for each event.
use serde::Serialize;
use std::io::{self, Write};

#[derive(Serialize)]
pub struct Upload {
    pub connection_epoch: u64,
    pub content_grant_epoch: u64,
    pub output: u64,
    pub candidate_generation: u64,
    pub resource_id: u64,
    pub resource_generation: u64,
    pub width: u32,
    pub height: u32,
    pub total_bytes: u64,
    pub begin_usec: u64,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event<'a> {
    Start {
        frames: u64,
        wire: &'a str,
        probe_bytes: u64,
    },
    UploadOffer {
        upload: Upload,
    },
    UploadAccepted {
        resource_id: u64,
        resource_generation: u64,
        accepted_usec: u64,
    },
    ProbeReleased {
        resource_id: u64,
        resource_generation: u64,
        released_usec: u64,
    },
    Permit {
        generation: u64,
        demand_usec: u64,
        received_usec: u64,
    },
    Offer {
        connection_epoch: u64,
        content_grant_epoch: u64,
        output: u64,
        candidate_generation: u64,
        submitted_monotonic_usec: u64,
    },
    Terminal {
        connection_epoch: u64,
        content_grant_epoch: u64,
        output: u64,
        candidate_generation: u64,
        outcome: &'a str,
        kind: u16,
        reason: u16,
        observed_monotonic_usec: u64,
    },
    End {
        offered: u64,
        completed: u64,
    },
    Failed {
        error: &'a str,
    },
}

pub fn write(output: &mut impl Write, event: Event<'_>) -> io::Result<()> {
    #[derive(Serialize)]
    struct Record<'a> {
        schema: u8,
        #[serde(flatten)]
        event: Event<'a>,
    }
    // A complete record is below PIPE_BUF. Overflow emits no partial evidence.
    let mut bytes = [0u8; 1024];
    let mut buffer = io::Cursor::new(bytes.as_mut_slice());
    serde_json::to_writer(&mut buffer, &Record { schema: 2, event })?;
    writeln!(buffer)?;
    let len = buffer.position() as usize;
    output.write_all(&bytes[..len])?;
    output.flush()
}

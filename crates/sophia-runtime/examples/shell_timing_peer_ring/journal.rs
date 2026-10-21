//! Journal records for the raw panel arms. Schema 5 is the owned-ring
//! arm (separate ring registration/offer/slot-release records, never a
//! sealed `UploadOffer`); schema 2 is the sealed arm with the legacy
//! shapes of the existing peer journal.
use serde::Serialize;
use std::io::{self, Write};

#[derive(Clone, Copy, Serialize)]
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
    /// The offer write time, after the slot fill has completed.
    pub begin_usec: u64,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event<'a> {
    Start {
        frames: u64,
        wire: &'a str,
        probe_bytes: u64,
        source: Option<&'a str>,
        workload: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ring_slots: Option<u64>,
    },
    UploadOffer {
        upload: Upload,
    },
    UploadAccepted {
        resource_id: u64,
        resource_generation: u64,
        accepted_usec: u64,
    },
    RingReady {
        connection_epoch: u64,
        content_grant_epoch: u64,
        channel: u64,
        ring: u64,
        ring_generation: u64,
        slots: u64,
        pixel_bytes: u64,
        admitted_usec: u64,
        ready_usec: u64,
    },
    RingOffer {
        upload: Upload,
        offer: u64,
        slot: u64,
        slot_generation: u64,
        fill_start_usec: u64,
    },
    RingAccepted {
        offer: u64,
        resource_id: u64,
        resource_generation: u64,
        accepted_usec: u64,
    },
    SlotReleased {
        offer: u64,
        slot: u64,
        slot_generation: u64,
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

pub fn write(output: &mut impl Write, schema: u8, event: Event<'_>) -> io::Result<()> {
    #[derive(Serialize)]
    struct Record<'a> {
        schema: u8,
        #[serde(flatten)]
        event: Event<'a>,
    }
    // A complete record is below PIPE_BUF. Overflow emits no partial evidence.
    let mut bytes = [0u8; 1024];
    let mut buffer = io::Cursor::new(bytes.as_mut_slice());
    serde_json::to_writer(&mut buffer, &Record { schema, event })?;
    writeln!(buffer)?;
    let len = buffer.position() as usize;
    output.write_all(&bytes[..len])?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of each record, rendered for the runner parser cross-check.
    #[test]
    fn sample_journal() {
        let mut out = Vec::new();
        let upload = |begin_usec| Upload {
            connection_epoch: 1,
            content_grant_epoch: 1,
            output: 1,
            candidate_generation: 1,
            resource_id: 1,
            resource_generation: 1,
            width: 200,
            height: 20,
            total_bytes: 16000,
            begin_usec,
        };
        for event in [
            Event::Start {
                frames: 1,
                wire: "files",
                probe_bytes: 0,
                source: Some("owned_ring"),
                workload: Some("fresh"),
                ring_slots: Some(2),
            },
            Event::RingReady {
                connection_epoch: 1,
                content_grant_epoch: 1,
                channel: 1,
                ring: 1,
                ring_generation: 1,
                slots: 2,
                pixel_bytes: 16000,
                admitted_usec: 100,
                ready_usec: 200,
            },
            Event::RingOffer {
                upload: upload(400),
                offer: 1,
                slot: 0,
                slot_generation: 2,
                fill_start_usec: 300,
            },
            Event::RingAccepted {
                offer: 1,
                resource_id: 1,
                resource_generation: 1,
                accepted_usec: 500,
            },
            Event::SlotReleased {
                offer: 1,
                slot: 0,
                slot_generation: 2,
                resource_id: 1,
                resource_generation: 1,
                released_usec: 600,
            },
            Event::Permit {
                generation: 1,
                demand_usec: 700,
                received_usec: 800,
            },
            Event::Offer {
                connection_epoch: 1,
                content_grant_epoch: 1,
                output: 1,
                candidate_generation: 1,
                submitted_monotonic_usec: 900,
            },
            Event::Terminal {
                connection_epoch: 1,
                content_grant_epoch: 1,
                output: 1,
                candidate_generation: 1,
                outcome: "presented",
                kind: 2,
                reason: 0,
                observed_monotonic_usec: 1000,
            },
            Event::End {
                offered: 1,
                completed: 1,
            },
        ] {
            write(&mut out, 5, event).unwrap();
        }
        print!("{}", String::from_utf8(out).unwrap());
    }

    /// One complete sealed frame, rendered for the runner parser check.
    #[test]
    fn sealed_sample_journal() {
        let mut out = Vec::new();
        let upload = Upload {
            connection_epoch: 1,
            content_grant_epoch: 1,
            output: 1,
            candidate_generation: 1,
            resource_id: 1,
            resource_generation: 1,
            width: 200,
            height: 20,
            total_bytes: 16000,
            begin_usec: 400,
        };
        for event in [
            Event::Start {
                frames: 1,
                wire: "files",
                probe_bytes: 0,
                source: Some("sealed"),
                workload: None,
                ring_slots: None,
            },
            Event::UploadOffer { upload },
            Event::UploadAccepted {
                resource_id: 1,
                resource_generation: 1,
                accepted_usec: 600,
            },
            Event::Permit {
                generation: 1,
                demand_usec: 700,
                received_usec: 800,
            },
            Event::Offer {
                connection_epoch: 1,
                content_grant_epoch: 1,
                output: 1,
                candidate_generation: 1,
                submitted_monotonic_usec: 900,
            },
            Event::Terminal {
                connection_epoch: 1,
                content_grant_epoch: 1,
                output: 1,
                candidate_generation: 1,
                outcome: "presented",
                kind: 2,
                reason: 0,
                observed_monotonic_usec: 1000,
            },
            Event::End {
                offered: 1,
                completed: 1,
            },
        ] {
            write(&mut out, 2, event).unwrap();
        }
        print!("{}", String::from_utf8(out).unwrap());
    }
}

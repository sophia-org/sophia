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

/// One placement of a retained tile in a candidate.
#[derive(Serialize)]
pub struct Placement {
    pub resource_id: u64,
    pub resource_generation: u64,
    pub destination_x_px: i32,
    pub destination_y_px: i32,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event<'a> {
    Start {
        frames: u64,
        wire: &'a str,
        probe_bytes: u64,
        /// Absent on byte runs, so their records keep the earlier bytes.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<&'a str>,
        /// Absent on fresh runs, so their records keep the earlier bytes.
        #[serde(skip_serializing_if = "Option::is_none")]
        workload: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        retained_resources: Option<u64>,
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
    /// Retained runs only: which retained tiles the candidate places and
    /// where, so resources join presentations without being per-frame uploads.
    CandidatePlacements {
        candidate_generation: u64,
        placements: Vec<Placement>,
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

    fn record(schema: u8, event: Event<'_>) -> String {
        let mut out = Vec::new();
        write(&mut out, schema, event).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// A fresh run's start record is byte-identical to the schema-2 record
    /// the analyzers already read: the new fields are absent, not null.
    #[test]
    fn fresh_start_record_is_unchanged() {
        let fresh = Event::Start {
            frames: 1000,
            wire: "files",
            probe_bytes: 0,
            source: Some("sealed"),
            workload: None,
            retained_resources: None,
        };
        assert_eq!(
            record(2, fresh),
            "{\"schema\":2,\"event\":\"start\",\"frames\":1000,\"wire\":\"files\",\"probe_bytes\":0,\"source\":\"sealed\"}\n"
        );
    }

    #[test]
    fn retained_records_carry_schema_3_and_the_agreed_fields() {
        let start = Event::Start {
            frames: 1000,
            wire: "files",
            probe_bytes: 0,
            source: Some("sealed"),
            workload: Some("retained"),
            retained_resources: Some(4),
        };
        assert_eq!(
            record(3, start),
            "{\"schema\":3,\"event\":\"start\",\"frames\":1000,\"wire\":\"files\",\"probe_bytes\":0,\"source\":\"sealed\",\"workload\":\"retained\",\"retained_resources\":4}\n"
        );
        let placements = Event::CandidatePlacements {
            candidate_generation: 7,
            placements: vec![
                Placement {
                    resource_id: 2,
                    resource_generation: 1,
                    destination_x_px: 480,
                    destination_y_px: 0,
                },
                Placement {
                    resource_id: 3,
                    resource_generation: 1,
                    destination_x_px: 0,
                    destination_y_px: 0,
                },
            ],
        };
        assert_eq!(
            record(3, placements),
            "{\"schema\":3,\"event\":\"candidate_placements\",\"candidate_generation\":7,\"placements\":[{\"resource_id\":2,\"resource_generation\":1,\"destination_x_px\":480,\"destination_y_px\":0},{\"resource_id\":3,\"resource_generation\":1,\"destination_x_px\":0,\"destination_y_px\":0}]}\n"
        );
    }

    /// Eight placements, the maximum, stay inside the one-record bound.
    #[test]
    fn eight_placements_fit_one_record() {
        let placements = (0..8)
            .map(|i| Placement {
                resource_id: 2 + i,
                resource_generation: 1,
                destination_x_px: 1_000_000 + i as i32,
                destination_y_px: 1_000_000,
            })
            .collect();
        let event = Event::CandidatePlacements {
            candidate_generation: u64::MAX,
            placements,
        };
        assert!(record(3, event).len() < 1024);
    }
}

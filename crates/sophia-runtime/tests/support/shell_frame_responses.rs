use crate::shell_transport::{
    ShellTransportError, control_budget::CONTROL_RECORD_BYTES, outbound::OutboundRecord,
};
use crate::{ContentCandidateError, ShellSessionTransport};
use sophia_protocol::ContentLimits;
use sophia_protocol::shell_files::*;

#[path = "frame_stream.rs"]
#[expect(
    clippy::duplicate_mod,
    reason = "reuse the integration lifecycle fixture inside private FIFO tests"
)]
#[expect(
    dead_code,
    reason = "response tests use the shared stream lifecycle fixture"
)]
mod fixture;

#[cfg(target_os = "linux")]
#[path = "shell_frame_controls_wire.rs"]
mod controls;

#[cfg(target_os = "linux")]
#[path = "shell_frame_sdk.rs"]
mod sdk;

struct Transport {
    transport: ShellSessionTransport,
    path: std::path::PathBuf,
}

impl Transport {
    fn take(f: &mut fixture::Fixture) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-frame-responses-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellSessionTransport::bind_for_supervised_uid(
            &path,
            rustix::process::getuid().as_raw(),
        )
        .unwrap();
        let limits = ContentLimits::prototype(fixture::grant());
        transport.content_epochs.admit(limits.clone()).unwrap();
        let (resources, candidates) = transport
            .content_epochs
            .active_pair_mut(fixture::grant())
            .unwrap();
        let (old_resources, old_candidates) = f.pool.active_pair_mut().unwrap();
        std::mem::swap(resources, old_resources);
        std::mem::swap(candidates, old_candidates);
        transport.state.store_grant = fixture::grant();
        transport.state.content_grant = Some(fixture::grant());
        transport.state.connection_epoch = fixture::grant().connection_epoch;
        transport.state.capabilities = SHELL_FRAME_CAPABILITY
            | crate::owned_ring_protocol::CAPABILITY
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        transport.state.content_limits = Some(limits);
        Self { transport, path }
    }

    fn flush(&mut self, now: u64) -> Result<(), ShellTransportError> {
        self.transport
            .state
            .flush_content_frame_events(&mut self.transport.content_epochs, now)
    }

    fn take_record(&mut self) -> ShellFrameRecord {
        let OutboundRecord::Frame(record) = self.transport.state.output.pop_front().record else {
            panic!("expected SFT response");
        };
        record
    }

    fn candidates(&mut self) -> &mut crate::ContentCandidateStore {
        self.transport
            .content_epochs
            .active_candidates_mut(fixture::grant())
            .unwrap()
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn frame_response_charges_and_bytes_match_every_event_golden() {
    let mut events = 0;
    for line in include_str!("../../../../protocol/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        let bytes: Vec<u8> = fields[2]
            .as_bytes()
            .chunks_exact(2)
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect();
        if let Ok(record) = decode_shell_frame_record(&bytes, ShellFileClass::Event, 13, 8192) {
            events += 1;
            let outbound = OutboundRecord::Frame(record);
            assert_eq!(
                outbound.body_len().unwrap(),
                bytes.len() - SHELL_FILE_HEADER_BYTES,
                "{}",
                fields[0]
            );
            let (_, body) = outbound.native().unwrap();
            assert_eq!(body, bytes[SHELL_FILE_HEADER_BYTES..], "{}", fields[0]);
            assert!(bytes.len() <= CONTROL_RECORD_BYTES);
        } else {
            let record =
                decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap();
            assert!(
                OutboundRecord::Frame(record).body_len().is_err(),
                "requests cannot consume server response credit"
            );
        }
    }
    assert_eq!(events, 10);
}

#[test]
fn response_backpressure_keeps_the_exact_head_and_one_lifetime_close_credit() {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    let mut t = Transport::take(&mut f);
    let head = *t.candidates().pending_frame_stream_event().unwrap();
    let before = t.candidates().control_occupancy();
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    assert_eq!(t.flush(2), Err(ShellTransportError::ContentQueueSaturated));
    assert_eq!(t.candidates().pending_frame_stream_event(), Some(&head));
    assert_eq!(t.candidates().control_occupancy(), before);
    assert!(t.transport.state.output.front().is_none());
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 64;
    t.flush(2).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Opened { .. }));
    assert_eq!(t.candidates().control_occupancy(), before - 1);
    t.candidates()
        .close_frame_stream(fixture::tx(21), fixture::key(1), 3)
        .unwrap();
    t.flush(3).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Closed { .. }));
    t.flush(4).unwrap();
    assert!(t.transport.state.output.front().is_none());
    assert_eq!(t.candidates().control_occupancy(), 0);
}

#[test]
fn prepared_and_terminal_use_the_same_fifo_but_only_terminal_returns_credit() {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.frame(1, 1, None).unwrap();
    let now = f.tick();
    let _display = f
        .candidates()
        .begin_submission(fixture::output(), 1, now)
        .unwrap();
    let now = f.tick();
    let mut t = Transport::take(&mut f);
    t.transport
        .state
        .content_prepared(
            &mut t.transport.content_epochs,
            fixture::grant(),
            fixture::output(),
            1,
            20,
            30,
            now,
        )
        .unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Opened { .. }));
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 1)
    );
    assert_eq!(
        t.candidates()
            .begin(fixture::tx(22), fixture::begin(2, 1), now),
        Err(ContentCandidateError::Budget)
    );
    t.candidates()
        .presented_at(fixture::output(), 1, 40, 20, 30, now + 1)
        .unwrap();
    let before = t.candidates().pending_event().cloned().unwrap();
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    assert_eq!(
        t.flush(now + 2),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(t.candidates().pending_event(), Some(&before));
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 64;
    t.flush(now + 3).unwrap();
    let ttl = ContentLimits::prototype(fixture::grant()).permit_timeout_ms;
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec, .. } if outcome.kind == 2 && credit_expires_msec == now + 3 + u64::from(ttl))
    );
    assert_eq!(
        t.candidates()
            .begin(fixture::tx(23), fixture::begin(3, 1), now + 3),
        Ok(())
    );
}

#[test]
fn a_foreign_grant_or_missing_capability_cannot_transfer_store_output() {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    let mut t = Transport::take(&mut f);
    t.transport.state.capabilities = 0;
    assert_eq!(t.flush(2), Err(ShellTransportError::MissingCapability));
    t.transport.state.capabilities = SHELL_FRAME_CAPABILITY;
    t.transport
        .state
        .content_grant
        .as_mut()
        .unwrap()
        .content_grant_epoch += 1;
    assert_eq!(t.flush(2), Err(ShellTransportError::WrongContentGrant));
    assert!(t.candidates().pending_frame_stream_event().is_some());
    assert!(t.transport.state.output.front().is_none());
}

#[test]
fn legacy_two_credit_streams_are_not_serialized_as_the_one_credit_profile() {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 2).unwrap();
    let mut t = Transport::take(&mut f);
    assert_eq!(t.flush(2), Err(ShellTransportError::WrongContentRecord));
    assert!(t.candidates().pending_frame_stream_event().is_some());
}

#[test]
fn close_and_terminal_keep_fifo_order_and_do_not_replenish_a_closed_stream() {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.frame(1, 1, None).unwrap();
    let _display = f.submit(1);
    let now = f.tick();
    f.candidates()
        .close_frame_stream(fixture::tx(25), fixture::key(1), now)
        .unwrap();
    f.present(1);
    let now = f.now;
    let mut t = Transport::take(&mut f);
    let reserved = t
        .transport
        .content_epochs
        .control_occupancy(fixture::grant());
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = reserved as u32;
    t.flush(now).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Opened { .. }));
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 1)
    );
    assert!(matches!(t.take_record(), ShellFrameRecord::Closed { .. }));
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 2)
    );
    assert!(t.transport.state.output.front().is_none());
    assert_eq!(t.candidates().control_occupancy(), 0);
}

#[path = "shell_frame_owner_completion.rs"]
mod completion;

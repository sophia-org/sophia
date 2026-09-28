use super::*;

const BOUNDS: JournalBounds = JournalBounds {
    bytes: 65_536,
    reserve_bytes: 12_288,
};

fn body(size: usize) -> Vec<u8> {
    // A Submitted body: nonzero submission, a candidate kind, reserved zeros.
    let mut body = vec![0; size];
    body[0] = 1;
    body[8..10].copy_from_slice(&(ShellFileKind::Negotiate as u16).to_le_bytes());
    body
}

fn ack(sequence: u64) -> ShellFileAck {
    ShellFileAck {
        connection_epoch: 1,
        sequence,
    }
}

#[test]
fn unsolicited_records_leave_the_terminal_reserve_to_credited_responses() {
    let now = Instant::now();
    let mut journal = Journal::new(1, BOUNDS, now);
    let general = usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS - SHELL_FILE_TERMINAL_RESERVE_RECORDS);
    for _ in 0..general {
        journal
            .append(ShellFileKind::Submitted, &body(16), false)
            .unwrap();
    }
    // A refused append spends no sequence, offset or byte.
    let size = journal.size();
    assert_eq!(
        journal.append(ShellFileKind::Submitted, &body(16), false),
        Err(Errno::EAGAIN)
    );
    assert_eq!(journal.size(), size);
    for _ in 0..SHELL_FILE_TERMINAL_RESERVE_RECORDS {
        journal
            .append(ShellFileKind::Submitted, &body(16), true)
            .unwrap();
    }
    assert_eq!(
        journal.records(),
        usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS)
    );
    assert_eq!(
        journal.append(ShellFileKind::Submitted, &body(16), true),
        Err(Errno::EAGAIN)
    );
    // The next sequence is the one after the last committed record.
    journal.ack(ack(1), now).unwrap();
    assert_eq!(
        journal.append(ShellFileKind::Submitted, &body(16), true),
        Ok(u64::from(SHELL_FILE_MAX_JOURNAL_RECORDS) + 1)
    );
}

#[test]
fn byte_reserve_binds_before_the_record_count() {
    let mut journal = Journal::new(1, BOUNDS, Instant::now());
    // 32-byte header plus a 2000-byte body: bytes bind before 192 records.
    let record = 2032;
    let fitting = (BOUNDS.bytes - BOUNDS.reserve_bytes) / record;
    assert!(fitting < 192);
    let large = body(2000);
    let mut appended = 0;
    while journal
        .append(ShellFileKind::Submitted, &large, false)
        .is_ok()
    {
        appended += 1;
    }
    assert_eq!(appended, fitting);
    // The byte reserve still admits a credited response of the same size.
    assert!(journal.fits(record, true));
    assert!(!journal.fits(record, false));
}

#[test]
fn acknowledgement_releases_retention_and_records_progress() {
    let start = Instant::now();
    let mut journal = Journal::new(1, BOUNDS, start);
    for _ in 0..3 {
        journal
            .append(ShellFileKind::Submitted, &body(16), true)
            .unwrap();
    }
    assert_eq!(journal.last_progress(), start);
    let later = start + std::time::Duration::from_millis(5);
    journal.ack(ack(2), later).unwrap();
    assert_eq!(journal.records(), 1);
    assert_eq!(journal.last_progress(), later);
    // Below the floor is stale; past the tail is invalid; a repeat is idempotent.
    assert_eq!(journal.read(0, 64), Err(Errno::ESTALE));
    assert_eq!(journal.read(journal.size() + 1, 64), Err(Errno::EINVAL));
    assert!(matches!(
        journal.read(journal.size(), 64),
        Ok(ReadOutcome::Pending)
    ));
    journal.ack(ack(2), later).unwrap();
    assert_eq!(journal.ack(ack(1), later), Err(Errno::EINVAL));
    assert_eq!(
        journal.ack(
            ShellFileAck {
                connection_epoch: 2,
                sequence: 3
            },
            later
        ),
        Err(Errno::ESTALE)
    );
}

mod terminal_reserve {
    use super::super::super::{
        ALLOCATION_RESULT_RECORD_BYTES, NATIVE_INPUT_RECORD_BYTES, role_bounds,
    };
    use super::*;
    use crate::ContentStoreProfile;
    use crate::shell_transport::outbound::OutboundRecord;
    use sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES;
    use sophia_protocol::*;

    const GRANT: ContentGrant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };

    /// The bar's and dock's largest credited response, a rejected
    /// `AllocationResult`, in its surviving native encoding.
    fn allocation_result() -> (ShellFileKind, Vec<u8>) {
        OutboundRecord::Content(
            TransactionId::from_raw(1),
            ShellContentRecord::AllocationResult(ContentAllocationResult {
                grant: GRANT,
                allocation_request_id: 1,
                status: 2,
                reason: ContentReason::Budget as u16,
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
                allocation: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                scale_generation: 0,
                logical: ContentLogicalRect::default(),
                pixel: ContentPixelRect::default(),
                scale_numerator: 0,
                scale_denominator: 0,
                allowed_reservation_extent: 0,
                margins: ContentMargins::default(),
                acknowledged_anchor: ContentPixelRect::default(),
            }),
        )
        .native()
        .unwrap()
    }

    /// The launcher's largest credited record, a `NativeInput` with the
    /// longest admitted text, in its surviving native encoding.
    fn native_input() -> (ShellFileKind, Vec<u8>) {
        let binding = NativeLauncherBinding {
            grant: GRANT,
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            opening: 1,
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            catalog_generation: 1,
            candidate_generation: 1,
            presentation_epoch: 1,
            interaction_generation: 1,
            state_revision: 1,
            focus_lease: 1,
        };
        OutboundRecord::NativeLauncher(
            TransactionId::from_raw(1),
            ShellNativeLauncherRecord::Input(NativeLauncherInput {
                event: NativeLauncherEvent {
                    binding,
                    event_id: 1,
                    state_revision: 2,
                },
                issued_mono_usec: 1,
                kind: NativeLauncherInputKind::Text,
                text: "a".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES),
            }),
        )
        .native()
        .unwrap()
    }

    /// Fills the unsolicited share of the journal as tightly as whole records
    /// allow, largest first, so the credited share is only the reserve.
    fn fill_unsolicited(journal: &mut Journal) {
        for size in [2000, 400, 80, 16, 10] {
            while journal
                .append(ShellFileKind::Submitted, &body(size), false)
                .is_ok()
            {}
        }
    }

    fn assert_reserve_holds(
        profile: ContentStoreProfile,
        (kind, record): (ShellFileKind, Vec<u8>),
    ) {
        let (_, bounds) = role_bounds(Some(profile));
        let whole = SHELL_FILE_HEADER_BYTES + record.len();
        assert_eq!(
            bounds.reserve_bytes,
            whole * usize::from(SHELL_FILE_TERMINAL_RESERVE_RECORDS)
        );
        assert!(whole * usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS) <= bounds.bytes);
        let mut journal = Journal::new(1, bounds, Instant::now());
        fill_unsolicited(&mut journal);
        // Every promised response has space: all 64 credited maximal records.
        for _ in 0..SHELL_FILE_TERMINAL_RESERVE_RECORDS {
            journal.append(kind, &record, true).unwrap();
        }
    }

    #[test]
    fn the_bar_and_dock_reserve_holds_64_encoded_allocation_results() {
        let record = allocation_result();
        assert_eq!(
            SHELL_FILE_HEADER_BYTES + record.1.len(),
            ALLOCATION_RESULT_RECORD_BYTES
        );
        assert_eq!(ALLOCATION_RESULT_RECORD_BYTES, 200);
        for profile in [
            ContentStoreProfile::Legacy,
            ContentStoreProfile::PersistentCatalog,
        ] {
            assert_reserve_holds(profile, record.clone());
        }
        assert_eq!(
            role_bounds(Some(ContentStoreProfile::Legacy)).1,
            JournalBounds {
                bytes: 65_536,
                reserve_bytes: 12_800,
            }
        );
    }

    #[test]
    fn the_launcher_reserve_holds_64_encoded_native_inputs() {
        let record = native_input();
        assert_eq!(
            SHELL_FILE_HEADER_BYTES + record.1.len(),
            NATIVE_INPUT_RECORD_BYTES
        );
        assert_eq!(NATIVE_INPUT_RECORD_BYTES, 430);
        assert_reserve_holds(ContentStoreProfile::NativeLauncher, record);
        assert_eq!(
            role_bounds(Some(ContentStoreProfile::NativeLauncher)).1,
            JournalBounds {
                bytes: 131_072,
                reserve_bytes: 27_520,
            }
        );
    }
}

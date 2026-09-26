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

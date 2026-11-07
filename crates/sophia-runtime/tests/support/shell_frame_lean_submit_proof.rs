//! Lean (bit 20) custody on the combined node: the header word is an ACK
//! applied before admission, no Submitted is published, nothing is retained.
use super::*;
use sophia_protocol::shell_files::{
    SHELL_FRAME_LEAN_CAPABILITY, SHELL_FRAME_TERMINAL_ONLY_CAPABILITY,
};

fn lean_export() -> ShellFiles {
    let mut files = export();
    files.capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY | SHELL_FRAME_LEAN_CAPABILITY;
    files
}

fn lean(files: &mut ShellFiles, bytes: &[u8]) -> Result<u32, Errno> {
    files.write(&Node::FrameSubmit, &mut Handle::Plain, 0, bytes)
}

fn with(bytes: &[u8], submission_id: u64, word: u64) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    bytes[16..24].copy_from_slice(&submission_id.to_le_bytes());
    bytes[24..32].copy_from_slice(&word.to_le_bytes());
    bytes
}

/// A paired Open whose Submitted is retained until a covering ACK.
fn retained_open(files: &mut ShellFiles) -> u64 {
    let (_writer, submit) = stage(files, &golden("open"));
    files.submit_transaction(&submit, false).unwrap();
    let sequence = files.accepted.as_ref().unwrap().sequence;
    assert_ne!(sequence, 0);
    sequence
}

#[test]
fn lean_commit_is_custody_on_its_rwrite_with_no_submitted_and_nothing_retained() {
    let mut files = lean_export();
    let _writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = golden("commit-copy-retire");
    let journal = journal_bytes(&files);
    assert_eq!(lean(&mut files, &bytes), Ok(bytes.len() as u32));
    assert_eq!(journal_bytes(&files), journal, "no Submitted record");
    assert!(files.accepted.is_none() && files.staging.is_none());
    assert_eq!(files.inbound.len(), 1);
    // The writer serves the next commit at once; no covering ACK is needed.
    let next = with(&bytes, 99, 0);
    assert_eq!(lean(&mut files, &next), Ok(next.len() as u32));
    assert_eq!(files.inbound.len(), 2);
    assert_eq!(journal_bytes(&files), journal);
    assert!(files.accepted.is_none());
    // A repeat is not a retry, and a refusal sticks to the writer as before.
    assert_eq!(lean(&mut files, &next), Err(EALREADY));
    assert_eq!(lean(&mut files, &with(&bytes, 100, 0)), Err(EBUSY));
}

#[test]
fn lean_header_word_is_an_ack_applied_before_admission() {
    let mut files = lean_export();
    let sequence = retained_open(&mut files);
    let bytes = golden("commit-copy-retire");
    // Without a covering word the retained Open still blocks the writer.
    assert_eq!(lean(&mut files, &with(&bytes, 20, 0)), Err(EBUSY));
    assert!(files.accepted.is_some());
    // An unretained word refuses the whole write before any effect.
    assert_eq!(lean(&mut files, &with(&bytes, 20, 99)), Err(Errno::EINVAL));
    assert!(files.accepted.is_some() && files.staging.is_none() && files.inbound.len() == 1);
    assert_eq!(files.submission_watermark, 12);
    // The covering word releases the Open, then the commit is admitted.
    let commit = with(&bytes, 20, sequence);
    assert_eq!(lean(&mut files, &commit), Ok(commit.len() as u32));
    assert!(files.accepted.is_none());
    assert_eq!(files.inbound.len(), 2);
    // An exact repeat of the word is idempotent.
    assert_eq!(
        lean(&mut files, &with(&bytes, 21, sequence)),
        Ok(commit.len() as u32)
    );
    assert_eq!(files.inbound.len(), 3);
    assert_eq!(files.submission_watermark, 21);
}

#[test]
fn nonzero_word_is_invalid_without_lean() {
    let mut files = export();
    files.capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
    let _writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = with(&golden("commit-copy-retire"), 20, 1);
    assert_eq!(lean(&mut files, &bytes), Err(Errno::EINVAL));
    assert!(files.staging.is_none() && files.inbound.is_empty());
    assert_eq!(files.submission_watermark, 0);
}

#[test]
fn lean_backpressure_retry_keeps_identity_without_the_word() {
    let mut files = lean_export();
    let sequence = retained_open(&mut files);
    let bytes = golden("commit-copy-retire");
    let value = decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap();
    files
        .inbound
        .extend((0..INBOUND_RECORDS).map(|_| Inbound::Frame(Box::new(value.clone()))));
    // The ACK applies even though admission is refused; staging drops the word.
    assert_eq!(
        lean(&mut files, &with(&bytes, 20, sequence)),
        Err(Errno::EAGAIN)
    );
    assert!(files.accepted.is_none());
    assert_eq!(files.staging.as_ref().unwrap().bytes, with(&bytes, 20, 0));
    // A retry may carry a fresh word (here, an idempotent repeat) and no change of bytes.
    assert_eq!(lean(&mut files, &with(&bytes, 20, 0)), Err(Errno::EAGAIN));
    assert_eq!(
        lean(&mut files, &with(&bytes, 20, sequence)),
        Err(Errno::EAGAIN)
    );
    let mut changed = with(&bytes, 20, 0);
    changed[32..40].copy_from_slice(&99u64.to_le_bytes());
    assert!(decode_shell_frame_record(&changed, ShellFileClass::Candidate, 13, 8192).is_ok());
    assert_eq!(lean(&mut files, &changed), Err(Errno::EINVAL));
    assert_eq!(files.staging.as_ref().unwrap().bytes, with(&bytes, 20, 0));
    files.inbound.clear();
    assert_eq!(
        lean(&mut files, &with(&bytes, 20, sequence)),
        Ok(bytes.len() as u32)
    );
    assert!(files.accepted.is_none() && files.staging.is_none());
    assert_eq!(files.inbound.len(), 1);
}

#[test]
fn lean_frame_in_flight_holds_source_records_until_its_outcome_is_journaled() {
    let mut files = lean_export();
    let _writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = golden("commit-copy-retire");
    assert_eq!(lean(&mut files, &bytes), Ok(bytes.len() as u32));
    assert!(
        files.publish_image_records(),
        "nothing in flight before admission"
    );
    let accepted = files.with_frame_commit(|input| {
        input.take();
        Ok(crate::ContentFrameAdmission::Accepted { copy_job: None })
    });
    assert!(matches!(
        accepted,
        Ok(Some(crate::ContentFrameAdmission::Accepted { .. }))
    ));
    assert_eq!(files.lean_frames_in_flight, 1);
    assert!(
        !files.publish_image_records(),
        "source records wait for the terminal"
    );
    // A legacy candidate outcome is not a frame terminal.
    files
        .append_event(ShellFileKind::CandidateOutcome, &[0; 40], false)
        .unwrap();
    assert_eq!(files.lean_frames_in_flight, 1);
    files
        .append_event(ShellFileKind::FrameOutcome, &[0; 116], false)
        .unwrap();
    assert_eq!(files.lean_frames_in_flight, 0);
    assert!(files.publish_image_records());
    // Without bit 20 the same admission holds nothing.
    let mut files = export();
    files.capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
    let _writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    assert_eq!(
        files.write(&Node::FrameSubmit, &mut Handle::Plain, 0, &bytes),
        Ok(bytes.len() as u32)
    );
    files
        .with_frame_commit(|input| {
            input.take();
            Ok(crate::ContentFrameAdmission::Accepted { copy_job: None })
        })
        .unwrap();
    assert_eq!(files.lean_frames_in_flight, 0);
    assert!(files.publish_image_records());
}

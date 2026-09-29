use sophia_protocol::TransactionId;
use sophia_protocol::shell_files::*;

pub fn header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 5,
        submission_id: u64::from(shell_file_class(kind) == ShellFileClass::Candidate),
        sequence: u64::from(shell_file_class(kind) == ShellFileClass::Event),
    }
}

pub fn encode(record: ShellDescriptorRecord) -> Result<Vec<u8>, ShellFilePayloadError> {
    encode_shell_file_descriptor(
        header(shell_file_descriptor_kind(&record)),
        &ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(11),
            record,
        },
    )
}

/// The whole value and its domain transaction survive. Neither the file
/// submission ID nor journal sequence substitutes for the transaction.
pub fn round_trip(record: ShellDescriptorRecord) {
    let kind = shell_file_descriptor_kind(&record);
    let bytes = encode(record.clone()).unwrap();
    assert_eq!(
        decode_shell_file_descriptor(&bytes, kind).unwrap(),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(11),
            record,
        }
    );
    for end in 0..bytes.len() {
        assert!(decode_shell_file_descriptor(&bytes[..end], kind).is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(decode_shell_file_descriptor(&trailing, kind).is_err());
}

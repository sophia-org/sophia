use sophia_protocol::{IpcCodecError, IpcMessageKind, TransactionId, decode_frame, encode_frame};

#[test]
fn retired_output_ipc_kinds_are_unknown_even_with_a_valid_frame_header() {
    for kind in 64_u16..=68 {
        let mut bytes =
            encode_frame(IpcMessageKind::BrokerHealth, TransactionId::INVALID, &[]).unwrap();
        bytes[6..8].copy_from_slice(&kind.to_le_bytes());
        assert_eq!(
            decode_frame(&bytes),
            Err(IpcCodecError::UnknownMessageKind(kind))
        );
    }
}

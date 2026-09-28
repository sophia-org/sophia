//! The socket codec keeps its prototype layout check: a `ResourceBegin` frame
//! carries the prototype grant's chunk count. The file wire lays out the
//! negotiated grant instead, so the neutral record validator no longer makes
//! this check; the socket codec makes it explicitly.
use sophia_shell_ipc::*;
use sophia_shell_protocol::*;

/// 4096 x 8: three rows per prototype chunk (3 chunks), two per 32 KiB
/// negotiated chunk (4 chunks).
fn begin(chunks: u32) -> ShellContentRecord {
    ShellContentRecord::ResourceBegin(ContentResourceBegin {
        grant: ContentGrant {
            connection_epoch: 11,
            content_grant_epoch: 3,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 4096,
        height_px: 8,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: chunks,
        total_bytes: 4096 * 8 * 4,
    })
}

#[test]
fn a_socket_resource_begin_must_carry_the_prototype_chunk_count() {
    let tx = TransactionId::from_raw(7);
    let frame = encode_shell_content_frame(tx, &begin(3)).unwrap();
    assert_eq!(decode_shell_content_frame(&frame).unwrap(), (tx, begin(3)));
    for chunks in [4, 2, 9] {
        assert_eq!(
            encode_shell_content_frame(tx, &begin(chunks)),
            Err(IpcCodecError::InvalidRecord("content resource chunk count")),
            "{chunks} chunks"
        );
    }
    // An incoming frame whose count is the negotiated grant's, not the
    // prototype's, is refused with the same error. The count is the payload's
    // u32 after grant, resource, width, height, scale and format.
    let at = SOPHIA_IPC_HEADER_LEN + 16 + 16 + 16 + 4;
    assert_eq!(u32::from_le_bytes(frame[at..at + 4].try_into().unwrap()), 3);
    let mut patched = frame.clone();
    patched[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(
        decode_shell_content_frame(&patched),
        Err(IpcCodecError::InvalidRecord("content resource chunk count"))
    );
}

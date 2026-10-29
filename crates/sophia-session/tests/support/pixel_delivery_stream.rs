use super::*;
use std::{
    io::Write,
    net::Shutdown,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn info() -> OutputCaptureFrameInfo {
    OutputCaptureFrameInfo {
        output: OutputId::from_raw(7),
        output_generation: 11,
        presentation_generation: 13,
        width: 2,
        height: 1,
    }
}
fn bytes() -> Vec<u8> {
    let pixels = [255, 0, 0, 255, 0, 0, 255, 255];
    [
        MAGIC.as_slice(),
        &17u64.to_le_bytes(),
        &info().encode().unwrap(),
        &pixels,
        &checksum(&pixels).to_le_bytes(),
    ]
    .concat()
}
fn parse(bytes: &[u8]) -> io::Result<Frame> {
    let (reader, mut writer) = UnixStream::pair().unwrap();
    writer.write_all(bytes).unwrap();
    writer.shutdown(Shutdown::Write).unwrap();
    stream::read_frame(
        reader,
        info().output,
        Instant::now() + Duration::from_secs(1),
    )
}

#[test]
fn private_capture_stream_matches_independent_bytes_and_requires_eof() {
    let expected = bytes();
    let frame = parse(&expected).unwrap();
    assert_eq!(frame.request, 17);
    assert_eq!(frame.info, info());
    let (mut writer, reader) = UnixStream::pair().unwrap();
    write_frame(&mut writer, 17, info(), &frame.pixels).unwrap();
    assert_eq!(
        stream::read_tail(
            reader,
            expected.len(),
            Instant::now() + Duration::from_secs(1)
        )
        .unwrap(),
        expected
    );
    for end in 0..expected.len() {
        assert!(parse(&expected[..end]).is_err(), "truncation {end}");
    }
    let mut extra = expected;
    extra.push(0);
    assert!(parse(&extra).is_err());
}

#[test]
fn private_capture_stream_rejects_identity_shape_checksum_and_metadata_text() {
    for offset in [0, 16, 24, 48, 56, 64, 72, 80, 88] {
        let mut bad = bytes();
        bad[offset] ^= 0xff;
        assert!(parse(&bad).is_err(), "offset {offset}");
    }
    let mut text = b"output=7\n".to_vec();
    text.extend(bytes());
    assert!(parse(&text).is_err());
    let mut zero = bytes();
    zero[8..16].fill(0);
    assert!(parse(&zero).is_err());
    let mut zero = bytes();
    zero[32..40].fill(0);
    assert!(parse(&zero).is_err());
}

#[test]
fn private_capture_deadline_and_stderr_bound_do_not_wait_for_a_writer() {
    let (reader, _writer) = UnixStream::pair().unwrap();
    assert!(stream::read_frame(reader, info().output, Instant::now()).is_err());
    let (reader, mut writer) = UnixStream::pair().unwrap();
    writer.write_all(b"error").unwrap();
    writer.shutdown(Shutdown::Write).unwrap();
    assert!(stream::read_tail(reader, 4, Instant::now() + Duration::from_secs(1)).is_err());
    let (mut writer, _reader) = UnixStream::pair().unwrap();
    assert!(write_frame(&mut writer, 0, info(), &[0; 8]).is_err());
    assert!(write_frame(&mut writer, 1, info(), &[0; 7]).is_err());
}

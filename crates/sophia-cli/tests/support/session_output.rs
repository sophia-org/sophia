use std::io::{self, LineWriter, Write};

const PEER: &[u8] = b"gpu_bar_prepare schema=1 arm=owned_ring generation=266\n";

#[derive(Default)]
struct SharedLog {
    bytes: Vec<u8>,
    writes: usize,
}

impl Write for SharedLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        self.bytes.extend_from_slice(bytes);
        self.bytes.extend_from_slice(PEER);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn peer_cannot_split_long_stdout_record_from_its_newline() {
    for size in [0, 15, 1023, 1024, 2048, 8192] {
        let line = "x".repeat(size);
        let mut log = SharedLog::default();
        {
            let mut stdout = LineWriter::with_capacity(1024, &mut log);
            super::write_line(&mut stdout, &line).unwrap();
            stdout.flush().unwrap();
        }
        assert_eq!(log.writes, 1, "record length {size}");
        assert_eq!(log.bytes, [line.as_bytes(), b"\n", PEER].concat());
    }
}

#[test]
fn unbuffered_stderr_also_receives_the_whole_record() {
    let mut log = SharedLog::default();
    super::write_line(&mut log, "session error").unwrap();
    assert_eq!(log.writes, 1);
    assert_eq!(log.bytes, [b"session error\n".as_slice(), PEER].concat());
}

#[test]
fn output_errors_are_not_swallowed() {
    let mut empty = &mut [][..];
    assert_eq!(
        super::write_line(&mut empty, "error").unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
}

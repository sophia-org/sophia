// A client that writes before it reads, over a real socket.
//
// WHAT THESE PROVE. The thread that reads a client's requests never waits on
// that client taking its replies. A client that sends a burst and only then
// reads -- XTS5's TOO_LONG purpose: a zero-length header and 262 KB of body,
// which this frontend parses as one BadLength and sixty-five thousand
// opcode-0 requests -- has its whole burst read while it writes, and then
// receives every answer in order, followed by a reply to the request it
// sends afterwards. Before t165 the server's replies filled the kernel's
// buffer, its writer blocked, its reader stopped, and the client's write
// never returned: both tests here ended on a write timeout.
//
// And a client that never reads is not buffered without limit: what it is
// owed is bounded, the connection is ended at the bound, and a peer on the
// same service is answered throughout.

#[cfg(unix)]
mod flooding_client {
    use super::*;
    use std::io::{BufReader, Read, Write};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// AllocColor with a zero length field: four bytes the frontend answers
    /// with BadLength, since the request cannot be that short.
    const ZERO_LENGTH_ALLOC_COLOR: [u8; 4] = [84, 0, 0, 0];
    /// GetInputFocus, the round trip that proves a connection is served.
    const GET_INPUT_FOCUS: [u8; 4] = [43, 0, 1, 0];
    const BAD_LENGTH: u8 = 16;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-flooding-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1165),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy)
            .with_max_concurrent_clients(std::num::NonZeroUsize::new(4).unwrap());
        let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(8).unwrap());
        let server = thread::spawn(move || -> Result<(), X11SetupSocketError> {
            let mut frontend = XServerFrontend::bind(config).unwrap();
            for _ in 0..clients {
                frontend.serve_next_concurrently_routed(&broker)?;
            }
            frontend.wait_for_clients()
        });
        wait_for_socket(&socket_path);
        (socket_path, server)
    }

    fn connected(socket_path: &std::path::Path) -> std::os::unix::net::UnixStream {
        let mut client = connect_x_socket(socket_path);
        client
            .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut client, XByteOrder::LittleEndian);
        client
    }

    fn sequence(record: &[u8; 32]) -> u16 {
        u16::from_le_bytes([record[2], record[3]])
    }

    #[test]
    fn a_client_that_writes_its_burst_before_it_reads_is_still_served() {
        let (socket_path, server) = routed_service("burst", 1);
        let mut client = connected(&socket_path);
        // THE RED. A write that does not return is the deadlock: the server's
        // reader is blocked writing replies this client has not started to
        // read. Ten seconds is far past what reading 262 KB takes.
        client
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut burst = ZERO_LENGTH_ALLOC_COLOR.to_vec();
        burst.resize(4 + 262_140, 0);
        let requests_in_burst = burst.len() / 4;
        let written = Instant::now();
        client
            .write_all(&burst)
            .expect("the burst is read by a server that does not wait on this client reading");
        eprintln!(
            "flooding_client: {} bytes written in {:?}",
            burst.len(),
            written.elapsed()
        );
        client.write_all(&GET_INPUT_FOCUS).unwrap();

        // Everything comes back in order: one error per request in the
        // burst, sequence numbers consecutive, then the reply.
        client
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut reader = BufReader::new(client);
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut record = [0u8; 32];
        let mut errors = 0usize;
        let mut expected_sequence = 1u16;
        loop {
            assert!(Instant::now() < deadline, "answers stopped after {errors} errors");
            reader.read_exact(&mut record).expect("a record while answers are owed");
            assert_eq!(
                sequence(&record),
                expected_sequence,
                "records leave in order: after {errors} errors"
            );
            expected_sequence = expected_sequence.wrapping_add(1);
            match record[0] {
                0 => {
                    if errors == 0 {
                        assert_eq!(record[1], BAD_LENGTH, "a zero-length AllocColor is BadLength");
                    }
                    errors += 1;
                }
                1 => break,
                other => panic!("unexpected record type {other} after {errors} errors"),
            }
        }
        assert_eq!(
            errors, requests_in_burst,
            "one answer per request in the burst, none dropped"
        );
        drop(reader);
        server.join().unwrap().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn a_client_that_never_reads_is_ended_at_the_bound_while_a_peer_is_served() {
        let (socket_path, server) = routed_service("bound", 2);
        let mut healthy = connected(&socket_path);
        let round_trip = |healthy: &mut std::os::unix::net::UnixStream| {
            let asked = Instant::now();
            healthy.write_all(&GET_INPUT_FOCUS).unwrap();
            let record = read_x_record(healthy);
            assert_eq!(record[0], 1, "the peer's round trip is answered");
            asked.elapsed()
        };
        round_trip(&mut healthy);

        let mut stalled = connected(&socket_path);
        // Enough four-byte requests that their 32-byte errors pass the
        // 16 MiB bound many times over: the server must end this client
        // rather than keep buffering, and must not stop reading it first.
        let flood = vec![0u8; 3 << 20];
        stalled
            .set_write_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let written = Instant::now();
        match stalled.write_all(&flood) {
            // Read to the end, or ended part way: both mean the server kept
            // reading until it decided.
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ) => {}
            Err(error) => panic!("the server stopped reading a client it had not ended: {error}"),
        }
        eprintln!("flooding_client: flood took {:?}", written.elapsed());
        // The peer is served during and after, without waiting on the flood.
        let waited = round_trip(&mut healthy);
        assert!(waited < Duration::from_secs(3), "the peer waited {waited:?} behind the flood");

        // The stalled client's socket was ended: it reads what the kernel
        // held and then end of stream, never the spill that was dropped.
        stalled
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut drained = 0usize;
        let mut buffer = [0u8; 4096];
        loop {
            assert!(Instant::now() < deadline, "the stalled client was never ended; {drained} bytes read");
            match stalled.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => drained += count,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => panic!("reading the ended client: {error}"),
            }
        }
        assert!(
            drained < 32 << 20,
            "the server kept more than the bound for a client that never read: {drained} bytes"
        );
        round_trip(&mut healthy);
        drop(healthy);
        drop(stalled);
        server.join().unwrap().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }
}

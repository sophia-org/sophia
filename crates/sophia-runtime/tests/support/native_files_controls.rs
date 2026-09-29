//! Negotiation refusals and paced upload controls used by the owner tests.
use super::*;

impl Peer {
    pub fn refuse_offer(
        &mut self,
        r: &mut ContentEpochRegistry,
        hello: ShellV1ClientHello,
        policy: ShellContentAdmissionPolicy,
        expected: ShellTransportError,
    ) {
        self.transport
            .begin_file_negotiation(
                r,
                self.limits.grant.connection_epoch,
                Duration::from_secs(3),
                policy,
            )
            .unwrap();
        std::thread::scope(|scope| {
            let client = &mut self.client;
            let worker = scope.spawn(move || {
                client.wire.setup();
                let offer =
                    encode_shell_file_negotiate(client.header(ShellFileKind::Negotiate), hello)
                        .unwrap();
                assert_eq!(client.wire.submit(&offer).0, 119);
                let submitted = negotiation_io(client.wire.try_next_event())?;
                assert_eq!(
                    decode_shell_file_submitted(&submitted)
                        .unwrap()
                        .submission_id,
                    1
                );
                negotiation_io(client.wire.try_ack(&submitted))?;
                let event = negotiation_io(client.wire.try_next_event())?;
                let refused = decode_shell_file_refused(&event).unwrap();
                // The server revokes after this ACK; its Rwrite may race EOF.
                negotiation_io(client.wire.try_ack(&event));
                Some(refused)
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match self.transport.poll_negotiation(r, 65536) {
                    Ok(None) => {}
                    Ok(Some(_)) => panic!("refused offer negotiated"),
                    Err(error) => {
                        assert_eq!(error, expected);
                        break;
                    }
                }
                assert!(Instant::now() < deadline, "refusal hung");
                std::thread::yield_now();
            }
            let observed = worker.join().unwrap();
            if let ShellTransportError::ContentAdmissionRefused(refusal) = expected {
                assert_eq!(observed, Some(refusal));
            } else {
                assert_eq!(observed, None);
            }
        });
    }

    pub fn permit(&mut self, r: &mut ContentEpochRegistry) {
        self.send_content(
            r,
            ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: GRANT,
                output: OUTPUT,
                allocation: ALLOCATION,
                demand_id: 1,
                reason: 1,
            }),
        );
        let c = catalog();
        assert_eq!(
            self.transport
                .service_native_launcher_content(r, context(&[allocation()]), native(&c), 0)
                .unwrap(),
            1
        );
        assert_eq!(
            self.transport.next_content_demand(r).unwrap().1.demand_id,
            1
        );
        self.transport
            .grant_content_demand(r, tx(3), OUTPUT, 1, 0)
            .unwrap();
        assert!(matches!(
            self.read_content(r).1,
            ShellContentRecord::FramePermit(_)
        ));
    }

    pub fn begin_upload(&mut self, r: &mut ContentEpochRegistry, record: ContentResourceBegin) {
        self.drive(r, |client| {
            let bytes = encode_shell_file_resource_begin(
                client.header(ShellFileKind::ResourceBegin),
                &ShellFileResourceBegin {
                    transaction: tx(20),
                    slot: 0,
                    record: ShellContentRecord::ResourceBegin(record),
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
    }
    pub fn upload_chunks(&mut self, r: &mut ContentEpochRegistry, chunks: &[Vec<u8>]) {
        self.drive(r, |client| {
            assert_eq!(client.wire.open_path(10, &[b"upload", b"0"], 1).0, 13);
            let mut offset = 0;
            for chunk in chunks {
                let reply = client.wire.write_at(10, offset, chunk);
                assert_eq!(reply.0, 119);
                assert_eq!(
                    u32::from_le_bytes(reply.1.try_into().unwrap()) as usize,
                    chunk.len()
                );
                offset += chunk.len() as u64;
            }
        });
    }
    pub fn end_upload(&mut self, r: &mut ContentEpochRegistry, record: ContentResourceEnd) {
        self.drive(r, |client| {
            let bytes = encode_shell_file_resource_end(
                client.header(ShellFileKind::ResourceEnd),
                &ShellFileTransactionRecord {
                    transaction: tx(20),
                    record: ShellContentRecord::ResourceEnd(record),
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
    }
}

fn negotiation_io<T>(result: std::io::Result<T>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                ),
                "unexpected negotiation error: {error}"
            );
            None
        }
    }
}

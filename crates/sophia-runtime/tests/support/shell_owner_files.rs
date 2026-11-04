//! File-server custody for private owner controls with supplied negotiation.
//! Request I/O and outbox transfer are separate so tests choose the handoff.
use crate::ContentStoreProfile;
use crate::shell_transport::ShellComponentTransport;
use crate::shell_transport::files::{ShellFileWire, ShellFiles, role_bounds};
use crate::shell_transport::outbound::OutboundRecord;
use sophia_protocol::shell_files::*;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub(in crate::shell_transport) use crate::raw_file_test_peer as raw;

pub struct Peer {
    raw: raw::Peer,
    epoch: u64,
    submission: u64,
    revision: u16,
}

impl Peer {
    pub fn attach(t: &mut ShellComponentTransport, profile: Option<ContentStoreProfile>) -> Self {
        Self::attach_revision(t, profile, 8)
    }

    #[cfg(all(feature = "experimental-native-frame-transaction", target_os = "linux"))]
    pub fn attach_frame(t: &mut ShellComponentTransport) -> Self {
        Self::attach_revision(t, Some(ContentStoreProfile::Legacy), 13)
    }

    fn attach_revision(
        t: &mut ShellComponentTransport,
        profile: Option<ContentStoreProfile>,
        revision: u16,
    ) -> Self {
        let (local, remote) = UnixStream::pair().unwrap();
        let (role, bounds) = role_bounds(profile);
        let mut export = ShellFiles::awaiting_negotiation(
            t.connection_epoch,
            role,
            matches!(
                profile,
                Some(ContentStoreProfile::NativeLauncher | ContentStoreProfile::PersistentCatalog)
            ),
            bounds,
            100,
            Instant::now(),
        );
        let limits = t.content_limits.clone().map(|limits| {
            let bytes = encode_shell_file_limits(
                ShellFileHeader {
                    kind: ShellFileKind::Limits,
                    connection_epoch: t.connection_epoch,
                    submission_id: 0,
                    sequence: 0,
                },
                limits.clone(),
            )
            .unwrap();
            (bytes, limits)
        });
        #[cfg(all(feature = "experimental-native-frame-transaction", target_os = "linux"))]
        if revision == 13 {
            let (mut discovery, limits, formats, role_bounds) =
                crate::shell_transport::owned_copy::wire_tests::config();
            discovery.connection_epoch = t.connection_epoch;
            export
                .configure_image_channel(Some(
                    crate::shell_transport::files::ShellImageChannelConfig {
                        discovery,
                        limits,
                        formats,
                        role_bounds,
                    },
                ))
                .unwrap();
        }
        if revision == 13 {
            export
                .complete_negotiation_with_image_revision(limits, t.capabilities, revision)
                .unwrap();
        } else {
            export.complete_negotiation(limits, t.capabilities).unwrap();
        }
        t.wire = Some(Box::new(
            ShellFileWire::adopt(local, export, None, false).unwrap(),
        ));
        let mut peer = Self {
            raw: raw::Peer::from_stream(remote),
            epoch: t.connection_epoch,
            submission: 0,
            revision,
        };
        peer.raw.set_revision(revision);
        peer.drive(t, raw::Peer::setup);
        peer
    }

    fn drive<R: Send>(
        &mut self,
        t: &mut ShellComponentTransport,
        operation: impl FnOnce(&mut raw::Peer) -> R + Send,
    ) -> R {
        std::thread::scope(|scope| {
            let raw = &mut self.raw;
            let worker = scope.spawn(move || operation(raw));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !worker.is_finished() {
                t.files_mut().unwrap().turn().unwrap();
                assert!(Instant::now() < deadline, "owner file peer hung");
                std::thread::yield_now();
            }
            worker.join().unwrap()
        })
    }

    pub fn submit(
        &mut self,
        t: &mut ShellComponentTransport,
        kind: ShellFileKind,
        encode: impl FnOnce(ShellFileHeader) -> Vec<u8>,
    ) {
        assert!(
            self.try_submit(t, kind, encode),
            "unexpected custody pressure"
        );
    }

    /// EAGAIN takes no custody and keeps the submission ID for an exact retry.
    pub fn try_submit(
        &mut self,
        t: &mut ShellComponentTransport,
        kind: ShellFileKind,
        encode: impl FnOnce(ShellFileHeader) -> Vec<u8>,
    ) -> bool {
        let id = self.submission + 1;
        let bytes = encode(ShellFileHeader {
            kind,
            connection_epoch: self.epoch,
            submission_id: id,
            sequence: 0,
        });
        let revision = self.revision;
        let accepted = self.drive(t, |raw| {
            let reply = raw.submit(&bytes);
            if reply.0 == 7 {
                assert_eq!(u32::from_le_bytes(reply.1.try_into().unwrap()), 11);
                raw.clear();
                return false;
            }
            assert_eq!(reply.0, 119);
            let receipt = raw.next_event();
            assert_eq!(
                decode_shell_file_submitted_at_revision(&receipt, revision)
                    .unwrap()
                    .submission_id,
                id
            );
            raw.ack(&receipt);
            raw.clear();
            true
        });
        if accepted {
            self.submission = id;
        }
        accepted
    }

    /// Every queued record stays charged until the production drain succeeds.
    pub fn drain(t: &mut ShellComponentTransport) -> bool {
        let Some(files) = t.wire.as_mut() else {
            panic!("file wire required")
        };
        ShellComponentTransport::drain_file_output(files, &mut t.output, None).unwrap()
    }

    pub fn fill_journal(t: &mut ShellComponentTransport, record: &OutboundRecord) -> usize {
        let (kind, body) = record.native().unwrap();
        assert_eq!(shell_file_class(kind), ShellFileClass::Event);
        for count in 0..=256 {
            if !t.files_mut().unwrap().append(kind, &body, true).unwrap() {
                assert!(count > 0);
                return count;
            }
        }
        panic!("journal did not saturate");
    }

    /// Check the exact native body, including transaction, before releasing
    /// retention. Objects are read through EOF and clunked before their ACK.
    pub fn expect(&mut self, t: &mut ShellComponentTransport, record: &OutboundRecord) {
        let (kind, body) = record.native().unwrap();
        let revision = self.revision;
        self.drive(t, |raw| {
            let event = raw.next_event();
            if shell_file_class(kind) == ShellFileClass::Object {
                let announcement = decode_shell_file_object_published(&event).unwrap();
                assert_eq!(announcement.object, kind);
                let name: &[u8] = match kind {
                    ShellFileKind::Outputs => b"outputs",
                    _ => panic!("unsupported fixture object: {kind:?}"),
                };
                raw.open(7, name, 0);
                let mut bytes = Vec::new();
                loop {
                    let part = raw.read(7, bytes.len() as u64);
                    if part.is_empty() {
                        break;
                    }
                    bytes.extend(part);
                    assert!(bytes.len() <= 1024 * 1024);
                }
                raw.clunk(7);
                let object =
                    decode_shell_file_record_at_revision(&bytes, ShellFileClass::Object, revision)
                        .unwrap();
                assert_eq!(object.header.kind, kind);
                assert_eq!(object.body, body);
            } else {
                let value =
                    decode_shell_file_record_at_revision(&event, ShellFileClass::Event, revision)
                        .unwrap();
                assert_eq!(value.header.kind, kind);
                assert_eq!(value.body, body);
            }
            raw.ack(&event);
        });
    }

    pub fn quiet(&mut self, t: &mut ShellComponentTransport) {
        self.drive(t, raw::Peer::no_event);
    }
}

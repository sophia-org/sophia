//! Raw 9P client for separately ordered native input ACKs and activations.
//! Record codecs are shared Rust contract code; this is not codec independence.
use super::*;
use std::collections::VecDeque;

pub(super) enum Observation {
    Content(TransactionId, ShellContentRecord),
    Native(TransactionId, ShellNativeLauncherRecord),
    Catalog(ShellApplicationCatalog),
}

pub(super) struct Client {
    pub wire: shell_file_peer::Peer,
    pending: VecDeque<Observation>,
    epoch: u64,
    submission: u64,
}

impl Client {
    pub fn new(path: &std::path::Path, epoch: u64) -> Self {
        Self {
            wire: shell_file_peer::Peer::connect(path),
            pending: VecDeque::new(),
            epoch,
            submission: 0,
        }
    }

    pub fn header(&mut self, kind: ShellFileKind) -> ShellFileHeader {
        self.submission += 1;
        ShellFileHeader {
            kind,
            connection_epoch: self.epoch,
            submission_id: self.submission,
            sequence: 0,
        }
    }

    pub fn submit(&mut self, bytes: &[u8]) {
        assert_eq!(self.wire.submit(bytes).0, 119);
        loop {
            let event = self.wire.next_event();
            let kind = decode_shell_file_record(&event, ShellFileClass::Event)
                .unwrap()
                .header
                .kind;
            if kind == ShellFileKind::Submitted {
                assert_eq!(
                    decode_shell_file_submitted(&event).unwrap().submission_id,
                    self.submission
                );
                self.wire.ack(&event);
                self.wire.clear();
                return;
            }
            let observation = self.observe(&event);
            self.pending.push_back(observation);
        }
    }

    pub fn next(&mut self) -> Observation {
        if let Some(observation) = self.pending.pop_front() {
            return observation;
        }
        let event = self.wire.next_event();
        self.observe(&event)
    }

    pub fn no_event(&mut self) {
        assert!(self.pending.is_empty(), "unexpected buffered observation");
        self.wire.no_event();
    }

    fn observe(&mut self, bytes: &[u8]) -> Observation {
        let kind = decode_shell_file_record(bytes, ShellFileClass::Event)
            .unwrap()
            .header
            .kind;
        let observation = match kind {
            ShellFileKind::ObjectPublished => {
                let object = decode_shell_file_object_published(bytes).unwrap();
                let name: &[u8] = match object.object {
                    ShellFileKind::Outputs => b"outputs",
                    ShellFileKind::Catalog => b"catalog",
                    other => panic!("unexpected object: {other:?}"),
                };
                self.wire.open(7, name, 0);
                let mut value = Vec::new();
                loop {
                    let part = self.wire.read(7, value.len() as u64);
                    if part.is_empty() {
                        break;
                    }
                    value.extend(part);
                    assert!(value.len() <= 1024 * 1024, "object exceeded fixture bound");
                }
                self.wire.clunk(7);
                if object.object == ShellFileKind::Outputs {
                    let value = decode_shell_file_outputs(&value).unwrap();
                    Observation::Content(value.transaction, value.record)
                } else {
                    Observation::Catalog(decode_shell_file_catalog(&value).unwrap().catalog.catalog)
                }
            }
            ShellFileKind::NativeInput => {
                let value = decode_shell_file_native_input(bytes).unwrap();
                Observation::Native(value.transaction, value.record)
            }
            ShellFileKind::NativeOpening
            | ShellFileKind::NativeFocus
            | ShellFileKind::NativeFocusRevoked
            | ShellFileKind::NativeClosed
            | ShellFileKind::NativeActivationOutcome => {
                let value = decode_shell_file_native_launcher_transaction(bytes, kind).unwrap();
                Observation::Native(value.transaction, value.record)
            }
            _ => {
                let value = match kind {
                    ShellFileKind::AllocationResult => decode_shell_file_allocation_result(bytes),
                    ShellFileKind::ResourceStatus => decode_shell_file_resource_status(bytes),
                    ShellFileKind::ResourceReleased => decode_shell_file_resource_released(bytes),
                    _ => decode_shell_file_transaction(bytes, kind),
                }
                .unwrap();
                Observation::Content(value.transaction, value.record)
            }
        };
        self.wire.ack(bytes);
        observation
    }
}

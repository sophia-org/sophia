//! A raw 9P2000.L peer for `sophia_shell_fs_v1` tests: exactly the bytes a
//! client writes, with no Sophia transport code. Shared by runtime and Session.
#![allow(dead_code)]
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use sophia_protocol::shell_files::*;

/// A raw 9P2000.L peer: exactly the bytes a client writes, no Sophia codec
/// for the transport itself.
pub struct Peer {
    stream: UnixStream,
    tag: u16,
    offset: u64,
    queued: VecDeque<Vec<u8>>,
}

impl Peer {
    pub fn connect(path: &std::path::Path) -> Self {
        let stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        Self {
            stream,
            tag: 0,
            offset: 0,
            queued: VecDeque::new(),
        }
    }

    pub fn rpc(&mut self, kind: u8, body: &[u8]) -> io::Result<(u8, Vec<u8>)> {
        let tag = if kind == 100 {
            u16::MAX
        } else {
            self.tag += 1;
            self.tag
        };
        let mut bytes = ((7 + body.len()) as u32).to_le_bytes().to_vec();
        bytes.push(kind);
        bytes.extend(tag.to_le_bytes());
        bytes.extend(body);
        self.stream.write_all(&bytes)?;
        let mut header = [0; 7];
        self.stream.read_exact(&mut header)?;
        assert_eq!(u16::from_le_bytes(header[5..].try_into().unwrap()), tag);
        let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        assert!((7..=65536).contains(&size));
        let mut body = vec![0; size - 7];
        self.stream.read_exact(&mut body)?;
        Ok((header[4], body))
    }

    pub fn setup(&mut self) {
        let version = [
            65536u32.to_le_bytes().as_slice(),
            &8u16.to_le_bytes(),
            b"9P2000.L",
        ]
        .concat();
        assert_eq!(self.rpc(100, &version).unwrap().0, 101);
        let attach = [
            1u32.to_le_bytes().as_slice(),
            &u32::MAX.to_le_bytes(),
            &[0; 4],
            &u32::MAX.to_le_bytes(),
        ]
        .concat();
        assert_eq!(self.rpc(104, &attach).unwrap().0, 105);
        self.open(2, b"events", 0);
        self.open(3, b"submit", 1);
        self.open(4, b"ack", 1);
    }

    pub fn walk(&mut self, fid: u32, name: &[u8]) {
        let walk = [
            1u32.to_le_bytes().as_slice(),
            &fid.to_le_bytes(),
            &1u16.to_le_bytes(),
            &(name.len() as u16).to_le_bytes(),
            name,
        ]
        .concat();
        assert_eq!(self.rpc(110, &walk).unwrap().0, 111);
    }

    pub fn open(&mut self, fid: u32, name: &[u8], mode: u32) {
        self.walk(fid, name);
        assert_eq!(
            self.rpc(12, &[fid.to_le_bytes(), mode.to_le_bytes()].concat())
                .unwrap()
                .0,
            13
        );
    }

    pub fn write(&mut self, fid: u32, bytes: &[u8]) -> (u8, Vec<u8>) {
        self.rpc(
            118,
            &[
                fid.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &(bytes.len() as u32).to_le_bytes(),
                bytes,
            ]
            .concat(),
        )
        .unwrap()
    }

    pub fn read(&mut self, fid: u32, offset: u64) -> Vec<u8> {
        let (kind, body) = self
            .rpc(
                116,
                &[
                    fid.to_le_bytes().as_slice(),
                    &offset.to_le_bytes(),
                    &65500u32.to_le_bytes(),
                ]
                .concat(),
            )
            .unwrap();
        assert_eq!(kind, 117);
        body[4..].to_vec()
    }

    /// Stages `bytes` in a fresh transaction fid and submits them. Returns
    /// the submit reply; the transaction stays open until `clear`.
    pub fn submit(&mut self, bytes: &[u8]) -> (u8, Vec<u8>) {
        let record = decode_shell_file_record(bytes, ShellFileClass::Candidate).unwrap();
        self.open(5, b"transaction", 2);
        assert_eq!(self.write(5, bytes).0, 119);
        let submit = encode_shell_file_submit(ShellFileSubmit {
            connection_epoch: record.header.connection_epoch,
            submission_id: record.header.submission_id,
            candidate_bytes: bytes.len() as u32,
        })
        .unwrap();
        self.write(3, &submit)
    }

    pub fn clear(&mut self) {
        assert_eq!(self.rpc(120, &5u32.to_le_bytes()).unwrap().0, 121);
    }

    pub fn next_event(&mut self) -> Vec<u8> {
        if let Some(bytes) = self.queued.pop_front() {
            return bytes;
        }
        let bytes = self.read(2, self.offset);
        self.offset += bytes.len() as u64;
        let mut rest = &bytes[..];
        while !rest.is_empty() {
            let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
            self.queued.push_back(rest[..size].to_vec());
            rest = &rest[size..];
        }
        self.queued.pop_front().expect("nonempty read")
    }

    pub fn ack(&mut self, bytes: &[u8]) {
        let record = decode_shell_file_record(bytes, ShellFileClass::Event).unwrap();
        let ack = encode_shell_file_ack(ShellFileAck {
            connection_epoch: record.header.connection_epoch,
            sequence: record.header.sequence,
        })
        .unwrap();
        assert_eq!(self.write(4, &ack).0, 119);
    }

    /// Submits, reads and acknowledges the custody record, clears the fid.
    pub fn submit_acknowledged(&mut self, bytes: &[u8], id: u64) {
        assert_eq!(self.submit(bytes).0, 119);
        let submitted = self.next_event();
        assert_eq!(
            decode_shell_file_submitted(&submitted)
                .unwrap()
                .submission_id,
            id
        );
        self.ack(&submitted);
        self.clear();
    }
}

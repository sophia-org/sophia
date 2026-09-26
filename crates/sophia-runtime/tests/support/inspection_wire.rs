#![allow(dead_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub fn frame(kind: u8, tag: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u32::try_from(7 + body.len()).unwrap().to_le_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}
pub fn string(bytes: &mut Vec<u8>, text: &[u8]) {
    bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
    bytes.extend_from_slice(text);
}
pub fn version() -> Vec<u8> {
    let mut body = (64_u32 * 1024).to_le_bytes().to_vec();
    string(&mut body, b"9P2000.L");
    frame(100, u16::MAX, &body)
}
pub fn attach() -> Vec<u8> {
    let mut body = 0_u32.to_le_bytes().to_vec();
    body.extend_from_slice(&u32::MAX.to_le_bytes());
    string(&mut body, b"irrelevant");
    string(&mut body, b"wm-admin-attach-does-not-grant-role");
    body.extend_from_slice(&u32::MAX.to_le_bytes());
    frame(104, 1, &body)
}
pub fn walk(tag: u16, fid: u32, name: &[u8]) -> Vec<u8> {
    let mut body = 0_u32.to_le_bytes().to_vec();
    body.extend_from_slice(&fid.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    string(&mut body, name);
    frame(110, tag, &body)
}
pub fn open(tag: u16, fid: u32, flags: u32) -> Vec<u8> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&flags.to_le_bytes());
    frame(12, tag, &body)
}
pub fn read(tag: u16, fid: u32, offset: u64, count: u32) -> Vec<u8> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend_from_slice(&offset.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    frame(116, tag, &body)
}
pub fn clunk(tag: u16, fid: u32) -> Vec<u8> {
    frame(120, tag, &fid.to_le_bytes())
}
pub fn receive(stream: &mut UnixStream) -> std::io::Result<Vec<u8>> {
    let mut header = [0; 7];
    stream.read_exact(&mut header)?;
    let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    assert!((7..=64 * 1024).contains(&size));
    let mut bytes = header.to_vec();
    bytes.resize(size, 0);
    stream.read_exact(&mut bytes[7..])?;
    Ok(bytes)
}
pub fn call(stream: &mut UnixStream, request: &[u8]) -> Vec<u8> {
    stream.write_all(request).unwrap();
    let bytes = receive(stream).unwrap();
    assert_eq!(&bytes[5..7], &request[5..7]);
    bytes
}
pub fn body(reply: &[u8]) -> &[u8] {
    assert_eq!(reply[4], 117, "expected Rread: {reply:?}");
    let count = u32::from_le_bytes(reply[7..11].try_into().unwrap()) as usize;
    assert_eq!(count, reply.len() - 11);
    &reply[11..]
}
pub fn errno(reply: &[u8]) -> u32 {
    assert_eq!(reply[4], 7);
    u32::from_le_bytes(reply[7..11].try_into().unwrap())
}
pub fn connect(path: &std::path::Path) -> UnixStream {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert_eq!(call(&mut stream, &version())[4], 101);
    assert_eq!(call(&mut stream, &attach())[4], 105);
    stream
}
pub fn opened(stream: &mut UnixStream, fid: u32, name: &[u8]) -> u64 {
    assert_eq!(call(stream, &walk(2, fid, name))[4], 111);
    let reply = call(stream, &open(3, fid, 0));
    assert_eq!(reply[4], 13);
    u64::from_le_bytes(reply[12..20].try_into().unwrap())
}

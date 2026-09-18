use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub enum Order {
    Little,
    Big,
}

impl Order {
    fn u16(self, value: u16) -> [u8; 2] {
        match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        }
    }
    fn read16(self, bytes: &[u8]) -> u16 {
        let bytes = bytes[..2].try_into().unwrap();
        match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        }
    }
}

pub struct Peer {
    _stream: UnixStream,
    size: (u16, u16),
}

impl Peer {
    pub fn connect(path: &Path, order: Order, cookie: Option<[u8; 32]>) -> Result<Self, String> {
        let mut stream = UnixStream::connect(path).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + super::WAIT;
        stream
            .set_read_timeout(Some(super::WAIT))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(super::WAIT))
            .map_err(|e| e.to_string())?;
        let name = if cookie.is_some() {
            b"SOPHIA-PRIVATE-INPUT-1".as_slice()
        } else {
            b""
        };
        let mut hello = vec![
            match order {
                Order::Little => b'l',
                Order::Big => b'B',
            },
            0,
        ];
        hello.extend(order.u16(11));
        hello.extend(order.u16(0));
        hello.extend(order.u16(name.len() as u16));
        hello.extend(order.u16(if cookie.is_some() { 32 } else { 0 }));
        hello.extend([0, 0]);
        hello.extend(name);
        while hello.len() % 4 != 0 {
            hello.push(0);
        }
        if let Some(cookie) = cookie {
            hello.extend(cookie);
        }
        stream.write_all(&hello).map_err(|e| e.to_string())?;
        let mut prefix = [0; 8];
        read_exact_until(&mut stream, &mut prefix, deadline)?;
        let mut body = vec![0; usize::from(order.read16(&prefix[6..])) * 4];
        read_exact_until(&mut stream, &mut body, deadline)?;
        if prefix[0] != 1 {
            return Err("setup refused".into());
        }
        if body.len() < 32 || body[20] != 1 {
            return Err("unexpected setup topology".into());
        }
        let vendor = usize::from(order.read16(&body[16..]));
        let formats = usize::from(body[21]);
        let screen = 32 + vendor.next_multiple_of(4) + formats * 8;
        if body.len() < screen + 40 {
            return Err("short setup screen".into());
        }
        let size = (
            order.read16(&body[screen + 20..]),
            order.read16(&body[screen + 22..]),
        );
        Ok(Self {
            _stream: stream,
            size,
        })
    }
    pub fn root_size(&self) -> (u16, u16) {
        self.size
    }
}

fn read_exact_until(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), String> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or("wire read deadline expired")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        match stream.read(bytes) {
            Ok(0) => return Err("peer closed before the expected bytes".into()),
            Ok(read) => bytes = &mut bytes[read..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

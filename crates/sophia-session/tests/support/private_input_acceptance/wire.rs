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
    fn u32(self, value: u32) -> [u8; 4] {
        match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        }
    }
    fn read32(self, bytes: &[u8]) -> u32 {
        let bytes = bytes[..4].try_into().unwrap();
        match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        }
    }
}

pub struct Peer {
    stream: UnixStream,
    size: (u16, u16),
    order: Order,
    root: u32,
    base: u32,
    sequence: u16,
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
            stream,
            size,
            order,
            root: order.read32(&body[screen..]),
            base: order.read32(&body[4..]),
            sequence: 0,
        })
    }
    pub fn root_size(&self) -> (u16, u16) {
        self.size
    }

    /// A reply orders the preceding create/map/draw requests. An X error must
    /// fail here rather than become an unexplained Engine-commit timeout.
    pub fn confirm_geometry(&mut self, window: u32) {
        self.request(14, 0, &self.order.u32(window));
        let mut reply = [0; 32];
        read_exact_until(&mut self.stream, &mut reply, Instant::now() + super::WAIT).unwrap();
        assert_eq!(reply[0], 1, "expected GetGeometry reply, got {reply:?}");
        assert_eq!(self.order.read16(&reply[2..]), self.sequence);
        assert_eq!(self.order.read32(&reply[4..]), 0);
        assert_eq!(self.order.read32(&reply[8..]), self.root);
        assert_eq!(self.order.read16(&reply[16..]), 8);
        assert_eq!(self.order.read16(&reply[18..]), 8);
    }

    pub fn create_map_and_draw(&mut self) -> u32 {
        let window = self.base | 1;
        let gc = self.base | 2;
        let mut create = Vec::new();
        create.extend(self.order.u32(window));
        create.extend(self.order.u32(self.root));
        for value in [0, 0, 8, 8, 0, 1] {
            create.extend(self.order.u16(value));
        }
        create.extend(self.order.u32(0)); // CopyFromParent visual.
        create.extend(self.order.u32(1 << 11)); // CWEventMask.
        create.extend(
            self.order
                .u32((1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 21)),
        );
        self.request(1, 0, &create);
        let mut create_gc = Vec::new();
        for value in [gc, window, 0] {
            create_gc.extend(self.order.u32(value));
        }
        self.request(55, 0, &create_gc);
        self.request(8, 0, &self.order.u32(window));
        let mut rectangle = Vec::new();
        rectangle.extend(self.order.u32(window));
        rectangle.extend(self.order.u32(gc));
        for value in [0, 0, 8, 8] {
            rectangle.extend(self.order.u16(value));
        }
        self.request(70, 0, &rectangle);
        window
    }

    fn request(&mut self, opcode: u8, detail: u8, payload: &[u8]) {
        assert_eq!(payload.len() % 4, 0);
        let mut bytes = vec![opcode, detail];
        bytes.extend(
            self.order
                .u16(u16::try_from(1 + payload.len() / 4).unwrap()),
        );
        bytes.extend(payload);
        self.stream.write_all(&bytes).unwrap();
        self.sequence = self.sequence.checked_add(1).unwrap();
    }

    pub fn focus_event(&mut self, window: u32) {
        let mut expected = [0; 32];
        expected[0] = 9;
        expected[1] = 3; // FocusIn, NotifyNonlinear.
        expected[2..4].copy_from_slice(&self.order.u16(self.sequence));
        expected[4..8].copy_from_slice(&self.order.u32(window));
        self.exact_event(expected);
    }

    pub fn input_event(&mut self, window: u32, kind: u8, detail: u8, time: u32, state: u16) {
        let mut expected = [0; 32];
        expected[0] = kind;
        expected[1] = detail;
        expected[2..4].copy_from_slice(&self.order.u16(self.sequence));
        expected[4..8].copy_from_slice(&self.order.u32(time));
        expected[8..12].copy_from_slice(&self.order.u32(self.root));
        expected[12..16].copy_from_slice(&self.order.u32(window));
        expected[28..30].copy_from_slice(&self.order.u16(state));
        expected[30] = 1;
        self.exact_event(expected);
    }

    fn exact_event(&mut self, expected: [u8; 32]) {
        let mut bytes = [0; 32];
        read_exact_until(&mut self.stream, &mut bytes, Instant::now() + super::WAIT).unwrap();
        assert_eq!(
            bytes, expected,
            "independent {:?} wire expectation",
            self.order
        );
    }

    pub fn empty_tail(&mut self) {
        self.stream
            .set_read_timeout(Some(std::time::Duration::from_millis(30)))
            .unwrap();
        match self.stream.read(&mut [0; 32]) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            other => panic!("unexpected wire tail: {other:?}"),
        }
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

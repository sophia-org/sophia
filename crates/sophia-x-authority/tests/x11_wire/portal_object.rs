//! Supplied application namespace, served by the production 9P core. It is
//! deliberately not the future 9P application frontend or its admission UI.
use sophia_9p::{Connection, ConnectionId, Errno, Limits, OpenFlags, export::*};
use sophia_portal::recipient_object::PortalObjectRecipient;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Node {
    Root,
    Object(u64),
}
pub enum Data {
    Source(Vec<u8>),
    Bound(Arc<PortalObjectRecipient>),
}
pub struct ObjectExport {
    data: Data,
    pub reads: usize,
}
impl Export for ObjectExport {
    type Node = Node;
    type Handle = ();
    fn attach(&mut self, c: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if c.connection != ConnectionId(1) || !c.aname.is_empty() {
            return Err(Errno::EACCES);
        }
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(1),
        })
    }
    fn check(&mut self, a: &Access<'_, Node>) -> Result<(), Errno> {
        if a.connection != ConnectionId(1) || a.epoch != Epoch(1) {
            return Err(Errno::ESTALE);
        }
        if matches!(a.operation, Operation::Write) {
            return Err(Errno::EACCES);
        }
        if let (Node::Object(qid), Data::Bound(owner)) = (a.node, &self.data) {
            owner.read(*qid, 0, 0)?;
        }
        Ok(())
    }
    fn lookup(&mut self, dir: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *dir != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        if name != WalkName::Child(b"transfer") {
            return Err(Errno::ENOENT);
        }
        Ok(Node::Object(match &self.data {
            Data::Source(_) => 1,
            Data::Bound(owner) => owner.current()?,
        }))
    }
    fn describe(&self, node: &Node, _: Option<&()>) -> Entry {
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: match node {
                Node::Root => 0,
                Node::Object(qid) => *qid,
            },
            qid_version: 0,
            permissions: 0o444,
            size: 0,
        }
    }
    fn open(&mut self, _: &Node, flags: OpenFlags) -> Result<(), Errno> {
        if flags.0 == 0 {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    }
    fn read(
        &mut self,
        node: &Node,
        _: &mut (),
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let Node::Object(qid) = node else {
            return Err(Errno::EISDIR);
        };
        self.reads += 1;
        Ok(ReadOutcome::Ready(match &self.data {
            Data::Source(bytes) => bytes
                .iter()
                .skip(offset as usize)
                .take(count as usize)
                .copied()
                .collect(),
            Data::Bound(owner) => owner.read(*qid, offset, count)?,
        }))
    }
    fn write(&mut self, _: &Node, _: &mut (), _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn release(&mut self, _: Node, _: Option<()>) {}
}
pub struct Wire {
    connection: Connection<ObjectExport>,
    pub owner: ObjectExport,
}
impl Wire {
    pub fn new(data: Data) -> Self {
        let mut wire = Self {
            connection: Connection::new(ConnectionId(1), None, Limits::default()),
            owner: ObjectExport { data, reads: 0 },
        };
        let mut body = 4096u32.to_le_bytes().to_vec();
        string(&mut body, b"9P2000.L");
        assert_eq!(wire.one(100, u16::MAX, body)[4], 101);
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend(u32::MAX.to_le_bytes());
        string(&mut body, b"supplied");
        string(&mut body, b"");
        body.extend(u32::MAX.to_le_bytes());
        assert_eq!(wire.one(104, 1, body)[4], 105);
        wire
    }
    fn one(&mut self, kind: u8, tag: u16, body: Vec<u8>) -> Vec<u8> {
        let mut request = ((body.len() + 7) as u32).to_le_bytes().to_vec();
        request.push(kind);
        request.extend(tag.to_le_bytes());
        request.extend(body);
        assert_eq!(
            self.connection.receive(&mut self.owner, &request).unwrap(),
            request.len()
        );
        let reply = self.connection.output().to_vec();
        self.connection.sent(reply.len());
        assert!(reply.len() >= 7);
        assert_eq!(
            u32::from_le_bytes(reply[..4].try_into().unwrap()) as usize,
            reply.len()
        );
        assert_eq!(&reply[5..7], &tag.to_le_bytes());
        reply
    }
    pub fn open(&mut self) {
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend(1u32.to_le_bytes());
        body.extend(1u16.to_le_bytes());
        string(&mut body, b"transfer");
        assert_eq!(self.one(110, 2, body)[4], 111);
        let mut body = 1u32.to_le_bytes().to_vec();
        body.extend(0u32.to_le_bytes());
        assert_eq!(self.one(12, 3, body)[4], 13);
    }
    pub fn read(&mut self) -> Result<Vec<u8>, u32> {
        let mut body = 1u32.to_le_bytes().to_vec();
        body.extend(0u64.to_le_bytes());
        body.extend(4096u32.to_le_bytes());
        let reply = self.one(116, 4, body);
        if reply[4] == 7 {
            return Err(u32::from_le_bytes(reply[7..11].try_into().unwrap()));
        }
        assert_eq!(reply[4], 117);
        assert_eq!(
            u32::from_le_bytes(reply[7..11].try_into().unwrap()) as usize,
            reply.len() - 11
        );
        Ok(reply[11..].to_vec())
    }
}
fn string(body: &mut Vec<u8>, bytes: &[u8]) {
    body.extend((bytes.len() as u16).to_le_bytes());
    body.extend(bytes);
}

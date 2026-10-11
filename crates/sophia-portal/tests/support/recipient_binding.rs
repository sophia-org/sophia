//! Device-free recipient owner, not an application frontend. The supplied
//! object is bound under one name; the production lifecycle supplies grants
//! and the production 9P core calls the recipient's checks on retained fids.
use sophia_9p::export::*;
use sophia_9p::{Connection, ConnectionId, Errno, Limits, OpenFlags};
use sophia_portal::execution_fence::PortalExecutionFence;
use sophia_portal::{PortalPolicyDecision, PortalRequestGrantLifecycle};
use sophia_protocol::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Node {
    Root,
    Status,
    Object(u64),
}
struct Bound {
    grant: PortalGrant,
    revision: u64,
    bytes: Vec<u8>,
}
pub struct Recipient {
    pub lifecycle: PortalRequestGrantLifecycle,
    pub fence: PortalExecutionFence,
    pub generation: u64,
    pub now: u64,
    pub ready: bool,
    pub reads: usize,
    pub released: Vec<Node>,
    current: Option<u64>,
    next: u64,
    objects: BTreeMap<u64, Bound>,
}
impl Recipient {
    fn new() -> Self {
        Self {
            lifecycle: PortalRequestGrantLifecycle::new(1).unwrap(),
            fence: PortalExecutionFence::default(),
            generation: 9,
            now: 100,
            ready: true,
            reads: 0,
            released: Vec::new(),
            current: None,
            next: 2,
            objects: BTreeMap::new(),
        }
    }
    pub fn grant(&mut self, id: u64, allow: bool) -> Option<PortalGrant> {
        self.lifecycle
            .submit_decided(
                PortalRequest {
                    transfer: PortalTransfer {
                        transfer: PortalTransferId::from_raw(id),
                        source_namespace: NamespaceId::from_raw(1),
                        target_namespace: NamespaceId::from_raw(2),
                        kind: PortalTransferKind::Clipboard,
                        mime_type: Some("text/plain".into()),
                        byte_size: 6,
                        decision: PortalDecision::Pending,
                        generation: self.generation,
                    },
                    deadline_msec: 1000,
                },
                if allow {
                    PortalPolicyDecision::Allow
                } else {
                    PortalPolicyDecision::Deny
                },
                self.generation,
                self.now,
            )
            .unwrap()
    }
    fn valid(&self, grant: &PortalGrant, revision: u64) -> bool {
        grant.state == PortalGrantState::Active
            && grant.target_namespace == NamespaceId::from_raw(2)
            && self.lifecycle.grant(grant.transfer) == Some(grant)
            && grant.source_generation == self.generation
            && grant.deadline_msec > self.now
            && self.fence.live()
            && self.fence.revision() == revision
    }
    pub fn bind(&mut self, grant: PortalGrant, bytes: &[u8]) -> Result<u64, Errno> {
        if !self.valid(&grant, self.fence.revision()) {
            return Err(Errno::EACCES);
        }
        // Two incarnations suffice for the replacement proof; no unbounded
        // object allocator or namespace compiler is hidden in this fixture.
        if self.objects.len() == 2 || bytes.len() > 64 {
            return Err(Errno::ENOSPC);
        }
        let qid = self.next;
        self.next += 1;
        self.objects.insert(
            qid,
            Bound {
                grant,
                revision: self.fence.revision(),
                bytes: bytes.to_vec(),
            },
        );
        self.current = Some(qid);
        Ok(qid)
    }
}
impl Export for Recipient {
    type Node = Node;
    type Handle = ();
    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        // Supplied recipient admission; attach names cannot select authority.
        if context.connection != ConnectionId(1) || !context.aname.is_empty() {
            return Err(Errno::EACCES);
        }
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(1),
        })
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if access.connection != ConnectionId(1) || access.epoch != Epoch(1) {
            return Err(Errno::ESTALE);
        }
        if let Node::Object(qid) = access.node {
            let b = &self.objects[qid];
            if self.current != Some(*qid) || !self.valid(&b.grant, b.revision) {
                return Err(Errno::ESTALE);
            }
        }
        if matches!(access.operation, Operation::Write) {
            return Err(Errno::EACCES);
        }
        Ok(())
    }
    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *directory != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        match name {
            WalkName::Child(b"status") => Ok(Node::Status),
            WalkName::Child(b"transfer") => self.current.map(Node::Object).ok_or(Errno::ENOENT),
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &Node, _: Option<&()>) -> Entry {
        let (qid_path, size) = match node {
            Node::Root => (0, 0),
            Node::Status => (1, 5),
            Node::Object(qid) => (*qid, self.objects[qid].bytes.len() as u64),
        };
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path,
            qid_version: 0,
            permissions: 0o444,
            size,
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
        let bytes = match node {
            Node::Status => b"ready".as_slice(),
            Node::Object(qid) => {
                self.reads += 1;
                if !self.ready {
                    return Ok(ReadOutcome::Pending);
                }
                &self.objects[qid].bytes
            }
            Node::Root => return Err(Errno::EISDIR),
        };
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        Ok(ReadOutcome::Ready(
            bytes[start..]
                .iter()
                .take(count as usize)
                .copied()
                .collect(),
        ))
    }
    fn write(&mut self, _: &Node, _: &mut (), _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn release(&mut self, node: Node, _: Option<()>) {
        self.released.push(node);
    }
}

// Small raw 9P client: no export methods are invoked by client requests.
pub struct Wire<E: Export = Recipient> {
    pub owner: E,
    pub connection: Connection<E>,
}
#[derive(Debug)]
pub struct Reply {
    pub kind: u8,
    pub tag: u16,
    pub body: Vec<u8>,
}
impl Reply {
    pub fn errno(&self) -> Option<u32> {
        (self.kind == 7).then(|| u32::from_le_bytes(self.body[..4].try_into().unwrap()))
    }
    pub fn data(&self) -> &[u8] {
        assert_eq!(self.kind, 117, "{self:?}");
        assert_eq!(
            u32::from_le_bytes(self.body[..4].try_into().unwrap()) as usize,
            self.body.len() - 4
        );
        &self.body[4..]
    }
}
impl Wire {
    pub fn new() -> Self {
        Self::with_owner(Recipient::new())
    }
}
impl<E: Export> Wire<E> {
    pub fn with_owner(owner: E) -> Self {
        let mut s = Self {
            owner,
            connection: Connection::new(ConnectionId(1), None, Limits::default()),
        };
        let mut b = 4096u32.to_le_bytes().to_vec();
        string(&mut b, b"9P2000.L");
        assert_eq!(s.one(100, u16::MAX, b).kind, 101);
        let mut b = 0u32.to_le_bytes().to_vec();
        b.extend(u32::MAX.to_le_bytes());
        string(&mut b, b"supplied-recipient");
        string(&mut b, b"");
        b.extend(u32::MAX.to_le_bytes());
        assert_eq!(s.one(104, 1, b).kind, 105);
        s
    }
    pub fn send(&mut self, kind: u8, tag: u16, body: Vec<u8>) -> Vec<Reply> {
        let mut b = ((body.len() + 7) as u32).to_le_bytes().to_vec();
        b.push(kind);
        b.extend(tag.to_le_bytes());
        b.extend(body);
        assert_eq!(
            self.connection.receive(&mut self.owner, &b).unwrap(),
            b.len()
        );
        self.drain()
    }
    pub fn drain(&mut self) -> Vec<Reply> {
        let bytes = self.connection.output().to_vec();
        self.connection.sent(bytes.len());
        let mut b = bytes.as_slice();
        let mut replies = Vec::new();
        while !b.is_empty() {
            let n = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
            replies.push(Reply {
                kind: b[4],
                tag: u16::from_le_bytes(b[5..7].try_into().unwrap()),
                body: b[7..n].to_vec(),
            });
            b = &b[n..];
        }
        replies
    }
    pub fn one(&mut self, kind: u8, tag: u16, body: Vec<u8>) -> Reply {
        let mut r = self.send(kind, tag, body);
        assert_eq!(r.len(), 1);
        let r = r.remove(0);
        assert_eq!(r.tag, tag);
        r
    }
    pub fn walk(&mut self, from: u32, to: u32, names: &[&[u8]]) -> Reply {
        let mut b = from.to_le_bytes().to_vec();
        b.extend(to.to_le_bytes());
        b.extend((names.len() as u16).to_le_bytes());
        for name in names {
            string(&mut b, name);
        }
        self.one(110, 2, b)
    }
    pub fn open(&mut self, fid: u32, name: &[u8]) -> u64 {
        let r = self.walk(0, fid, &[name]);
        assert_eq!(r.kind, 111, "{r:?}");
        assert_eq!(&r.body[..2], &1u16.to_le_bytes());
        let qid = u64::from_le_bytes(r.body[7..15].try_into().unwrap());
        let mut b = fid.to_le_bytes().to_vec();
        b.extend(0u32.to_le_bytes());
        assert_eq!(self.one(12, 3, b).kind, 13);
        qid
    }
    pub fn read_body(fid: u32) -> Vec<u8> {
        let mut b = fid.to_le_bytes().to_vec();
        b.extend(0u64.to_le_bytes());
        b.extend(64u32.to_le_bytes());
        b
    }
    pub fn read(&mut self, fid: u32) -> Reply {
        self.one(116, 4, Self::read_body(fid))
    }
}
fn string(b: &mut Vec<u8>, text: &[u8]) {
    b.extend((text.len() as u16).to_le_bytes());
    b.extend(text);
}

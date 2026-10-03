//! factotum's files over `sophia-9p`.
//!
//! The tree, qid paths and modes are 9front's (`sys/src/cmd/auth/factotum/
//! fs.c`): a root holding `factotum`, which holds `confirm`, `needkey`,
//! `ctl`, `rpc`, `proto` and `log`. Bytes inside files are 9front's. The
//! file-level errors 9front gives as strings become errno values here, and
//! their 9front text goes to `log`.
//!
//! Every attached connection is the agent's owner, admitted before it was
//! adopted; the agent records each one's [`ChannelClass`] first, and an
//! unrecorded connection cannot attach.

use crate::conversation::{Conversation, RpcFileError, RpcRead};
use crate::ctl::{self, CtlEffect, CtlError, ListCursor, ListTooSmall};
use crate::jobs::{ConversationId, JobSink};
use crate::keyring::Keyring;
use crate::logbuf::{LogBuf, LogReadTooShort};
use crate::proto::{ChannelClass, Env, JobOutcome, PamVerdict, Settings};
use sophia_9p::records::{Errno, OpenFlags};
use sophia_9p::{
    Access, AttachContext, Attachment, ConnectionId, DirEntry, Entry, Epoch, Export, NodeKind,
    Operation, ReadOutcome, WalkName,
};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;

const EBUSY: Errno = Errno(16);
const ERANGE: Errno = Errno(34);
const ENOMSG: Errno = Errno(42);
const EMSGSIZE: Errno = Errno(90);
const EPROTONOSUPPORT: Errno = Errno(93);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Node {
    Root,
    Dir,
    Confirm,
    Needkey,
    Ctl,
    Rpc,
    Proto,
    Log,
}

/// 9front's `dirtab`, in its order.
const FILES: [(&str, Node); 6] = [
    ("confirm", Node::Confirm),
    ("needkey", Node::Needkey),
    ("ctl", Node::Ctl),
    ("rpc", Node::Rpc),
    ("proto", Node::Proto),
    ("log", Node::Log),
];

impl Node {
    /// 9front's qid paths; 3 is unused there too.
    const fn qid_path(self) -> u64 {
        match self {
            Node::Root => 0,
            Node::Dir => 1,
            Node::Rpc => 2,
            Node::Proto => 4,
            Node::Confirm => 5,
            Node::Log => 6,
            Node::Ctl => 7,
            Node::Needkey => 8,
        }
    }

    const fn permissions(self) -> u32 {
        match self {
            Node::Root | Node::Dir => 0o555,
            Node::Confirm | Node::Needkey => 0o600,
            Node::Ctl => 0o644,
            Node::Rpc => 0o666,
            Node::Proto => 0o444,
            Node::Log => 0o400,
        }
    }

    const fn kind(self) -> NodeKind {
        match self {
            Node::Root | Node::Dir => NodeKind::Directory,
            _ => NodeKind::File,
        }
    }
}

#[derive(Debug)]
pub enum Handle {
    Directory,
    Rpc(ConversationId),
    Ctl(ListCursor),
    Proto(ListCursor),
    Log,
}

/// The agent's state behind its files.
pub struct FactotumExport {
    keyring: Keyring,
    log: LogBuf,
    settings: Settings,
    sequence: u32,
    debug: bool,
    classes: HashMap<ConnectionId, ChannelClass>,
    conversations: HashMap<ConversationId, (Conversation, ChannelClass)>,
    next_conversation: ConversationId,
    log_open: bool,
    /// The class of the connection whose open of `rpc` the core is about to
    /// perform, recorded by the check that precedes it.
    opening: Option<ChannelClass>,
    jobs: Option<Box<dyn JobSink>>,
    outcomes: Option<Receiver<(ConversationId, JobOutcome)>>,
}

impl FactotumExport {
    pub fn new(settings: Settings) -> Self {
        Self {
            keyring: Keyring::new(),
            log: LogBuf::new(),
            settings,
            sequence: 0,
            debug: false,
            classes: HashMap::new(),
            conversations: HashMap::new(),
            next_conversation: 1,
            log_open: false,
            opening: None,
            jobs: None,
            outcomes: None,
        }
    }

    /// Connects the worker pool. Until then a step that needs a job is
    /// answered as unavailable.
    pub fn attach_jobs(
        &mut self,
        jobs: Box<dyn JobSink>,
        outcomes: Receiver<(ConversationId, JobOutcome)>,
    ) {
        self.jobs = Some(jobs);
        self.outcomes = Some(outcomes);
    }

    /// Records an adopted connection's class; only recorded connections may
    /// attach.
    pub fn admit(&mut self, connection: ConnectionId, class: ChannelClass) {
        self.classes.insert(connection, class);
    }

    pub fn forget(&mut self, connection: ConnectionId) {
        self.classes.remove(&connection);
    }

    pub fn keyring(&self) -> &Keyring {
        &self.keyring
    }

    /// Delivers finished jobs to their conversations.
    fn collect_outcomes(&mut self) {
        let Some(outcomes) = self.outcomes.as_ref() else {
            return;
        };
        while let Ok((conversation, outcome)) = outcomes.try_recv() {
            if let Some((cx, _)) = self.conversations.get_mut(&conversation) {
                cx.finish_job(outcome);
            }
        }
    }

    fn read_rpc(&mut self, id: ConversationId, count: usize) -> Result<ReadOutcome, Errno> {
        self.collect_outcomes();
        let Self {
            conversations,
            keyring,
            log,
            settings,
            sequence,
            jobs,
            ..
        } = self;
        let Some((cx, class)) = conversations.get_mut(&id) else {
            return Err(Errno::EBADF);
        };
        let mut env = Env {
            keyring,
            log,
            settings,
            channel: *class,
            sequence,
        };
        match cx.read(&mut env, count) {
            RpcRead::Reply(reply) => Ok(ReadOutcome::Ready(reply)),
            RpcRead::Waiting => Ok(ReadOutcome::Pending),
            RpcRead::Started(job) => {
                let queued = jobs.as_mut().is_some_and(|jobs| jobs.submit(id, job));
                if !queued {
                    cx.finish_job(JobOutcome::Pam(PamVerdict::Unavailable));
                }
                Ok(ReadOutcome::Pending)
            }
            RpcRead::Refused(error) => Err(self.rpc_errno(error)),
        }
    }

    fn rpc_errno(&mut self, error: RpcFileError) -> Errno {
        let (text, errno) = match error {
            RpcFileError::TooLarge => ("rpc too large", EMSGSIZE),
            RpcFileError::AlreadyPending => ("rpc already pending; read to clear", EBUSY),
            RpcFileError::ReadTooSmall => ("rpc read too small", Errno::EINVAL),
            RpcFileError::NoRpcPending => ("no rpc pending", ENOMSG),
            RpcFileError::ConfirmClosed => ("confirm is closed", Errno::EACCES),
        };
        self.log.append(text.to_owned());
        errno
    }

    fn ctl_errno(&mut self, error: &CtlError) -> Errno {
        self.log.append(error.to_string());
        match error {
            CtlError::UnknownProto(_) => EPROTONOSUPPORT,
            CtlError::ProtoTakesNoKeys(_) => Errno::EOPNOTSUPP,
            CtlError::NoKeysToDelete => Errno::ENOENT,
            CtlError::PrivatePattern => Errno::EPERM,
            CtlError::UnknownVerb
            | CtlError::MultilineWrite
            | CtlError::KeyWithoutProtos
            | CtlError::Proto(_) => Errno::EINVAL,
        }
    }
}

impl Export for FactotumExport {
    type Node = Node;
    type Handle = Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if !self.classes.contains_key(&context.connection) {
            return Err(Errno::EACCES);
        }
        let root = match context.aname {
            b"" => Node::Root,
            b"factotum" => Node::Dir,
            _ => {
                self.log.append("unknown mount spec".into());
                return Err(Errno::ENOENT);
            }
        };
        Ok(Attachment {
            root,
            epoch: Epoch(context.connection.0),
        })
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        let Some(class) = self.classes.get(&access.connection).copied() else {
            return Err(Errno::EACCES);
        };
        // `open` is not told its connection, so the conversation's class is
        // taken from the check the core makes immediately before it.
        self.opening = matches!(access.operation, Operation::Open(_)).then_some(class);
        Ok(())
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (directory, name) {
            (Node::Root, WalkName::Child(b"factotum")) => Ok(Node::Dir),
            (Node::Dir, WalkName::Parent) => Ok(Node::Root),
            (Node::Dir, WalkName::Child(name)) => FILES
                .iter()
                .find(|(file, _)| file.as_bytes() == name)
                .map(|(_, node)| *node)
                .ok_or(Errno::ENOENT),
            (Node::Root, _) => Err(Errno::ENOENT),
            _ => Err(Errno::ENOTDIR),
        }
    }

    fn describe(&self, node: &Node, _handle: Option<&Handle>) -> Entry {
        Entry {
            kind: node.kind(),
            qid_path: node.qid_path(),
            qid_version: 0,
            permissions: node.permissions(),
            size: 0,
        }
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        // 9front refuses any mode bit but the access and truncate bits.
        if flags.0
            & !(OpenFlags::ACCESS_MASK
                | OpenFlags::TRUNCATE
                | OpenFlags::IGNORED
                | OpenFlags::DIRECTORY)
            != 0
        {
            return Err(Errno::EACCES);
        }
        let Some(access) = flags.access() else {
            return Err(Errno::EINVAL);
        };
        let owner = node.permissions() >> 6;
        if (access.reads() && owner & 4 == 0) || (access.writes() && owner & 2 == 0) {
            self.log.append("permission denied".into());
            return Err(Errno::EACCES);
        }
        match node {
            Node::Root | Node::Dir => Ok(Handle::Directory),
            // Closed until a trusted prompt exists: a confirm or needkey
            // answered from an ordinary client could approve a key use the
            // user never saw.
            Node::Confirm | Node::Needkey => Err(Errno::EACCES),
            Node::Ctl => Ok(Handle::Ctl(ListCursor::default())),
            Node::Proto => Ok(Handle::Proto(ListCursor::default())),
            Node::Log if self.log_open => {
                self.log.append("file in use".into());
                Err(EBUSY)
            }
            Node::Log => {
                self.log_open = true;
                Ok(Handle::Log)
            }
            Node::Rpc => {
                let id = self.next_conversation;
                self.next_conversation += 1;
                // Without a preceding check, the least-privileged class.
                let class = self.opening.take().unwrap_or(ChannelClass::User);
                self.conversations.insert(id, (Conversation::new(), class));
                Ok(Handle::Rpc(id))
            }
        }
    }

    fn read(
        &mut self,
        _node: &Node,
        handle: &mut Handle,
        _offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        match handle {
            Handle::Directory => Err(Errno::EISDIR),
            Handle::Rpc(id) => self.read_rpc(*id, count),
            Handle::Ctl(cursor) => cursor
                .read_key(&self.keyring, count)
                .map(ReadOutcome::Ready)
                .map_err(|ListTooSmall| ERANGE),
            Handle::Proto(cursor) => cursor
                .read_proto(count)
                .map(ReadOutcome::Ready)
                .map_err(|ListTooSmall| ERANGE),
            Handle::Log => match self.log.read(count) {
                Ok(Some(message)) => Ok(ReadOutcome::Ready(message)),
                Ok(None) => Ok(ReadOutcome::Pending),
                Err(LogReadTooShort) => Err(Errno::EINVAL),
            },
        }
    }

    fn write(
        &mut self,
        _node: &Node,
        handle: &mut Handle,
        _offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        let accepted = u32::try_from(data.len()).map_err(|_| EMSGSIZE)?;
        match handle {
            Handle::Rpc(id) => {
                let Some((cx, _)) = self.conversations.get_mut(id) else {
                    return Err(Errno::EBADF);
                };
                match cx.write(data) {
                    Ok(_) => Ok(accepted),
                    Err(error) => Err(self.rpc_errno(error)),
                }
            }
            Handle::Ctl(_) => {
                // Key lines carry secrets; the copy lives only for this call.
                let line = zeroize::Zeroizing::new(String::from_utf8_lossy(data).into_owned());
                match ctl::write(&mut self.keyring, &line) {
                    Ok(CtlEffect::None) => Ok(accepted),
                    Ok(CtlEffect::ToggleDebug) => {
                        self.debug = !self.debug;
                        Ok(accepted)
                    }
                    Err(error) => Err(self.ctl_errno(&error)),
                }
            }
            Handle::Directory | Handle::Proto(_) | Handle::Log => Err(Errno::EACCES),
        }
    }

    fn readdir(
        &mut self,
        directory: &Node,
        _handle: &mut Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        let children: &[(&str, Node)] = match directory {
            Node::Root => &[("factotum", Node::Dir)],
            Node::Dir => &FILES,
            _ => return Err(Errno::ENOTDIR),
        };
        let start = usize::try_from(cookie).unwrap_or(usize::MAX);
        Ok(children
            .iter()
            .enumerate()
            .skip(start)
            .take(max_entries)
            .map(|(index, (name, node))| DirEntry {
                name: name.as_bytes().to_vec(),
                entry: self.describe(node, None),
                next: index as u64 + 1,
            })
            .collect())
    }

    fn release(&mut self, _node: Node, handle: Option<Handle>) {
        match handle {
            Some(Handle::Rpc(id)) => {
                // Dropping the conversation zeroes what it held; its job, if
                // any, is stopped and its outcome dropped.
                self.conversations.remove(&id);
                if let Some(jobs) = self.jobs.as_mut() {
                    jobs.cancel(id);
                }
            }
            Some(Handle::Log) => self.log_open = false,
            Some(Handle::Directory | Handle::Ctl(_) | Handle::Proto(_)) | None => {}
        }
    }
}

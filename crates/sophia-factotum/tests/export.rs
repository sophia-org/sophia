//! factotum's files through the `Export` interface the 9P core drives:
//! admission by class, 9front's tree and modes, the closed prompt files,
//! the exclusive log, and an rpc whose step waits on a job.

use sophia_9p::records::{Errno, OpenFlags};
use sophia_9p::{Access, AttachContext, ConnectionId, Export, Operation, ReadOutcome, WalkName};
use sophia_factotum::export::{FactotumExport, Handle, Node};
use sophia_factotum::jobs::{ConversationId, JobSink};
use sophia_factotum::proto::{ChannelClass, Job, JobOutcome, PamVerdict, Settings};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

const SESSION: ConnectionId = ConnectionId(1);
const USER: ConnectionId = ConnectionId(2);
const READ: OpenFlags = OpenFlags(0);
const READ_WRITE: OpenFlags = OpenFlags(2);

#[derive(Clone, Default)]
struct Jobs(Arc<Mutex<Vec<(ConversationId, Job)>>>);

impl JobSink for Jobs {
    fn submit(&mut self, conversation: ConversationId, job: Job) -> bool {
        self.0.lock().unwrap().push((conversation, job));
        true
    }
    fn cancel(&mut self, _conversation: ConversationId) {}
}

fn agent() -> (FactotumExport, Jobs, Sender<(ConversationId, JobOutcome)>) {
    let mut export = FactotumExport::new(Settings {
        owner: "tb".into(),
        pam_service: "sophia-lock".into(),
    });
    let jobs = Jobs::default();
    let (outcomes, receiver) = channel();
    export.attach_jobs(Box::new(jobs.clone()), receiver);
    export.admit(SESSION, ChannelClass::Session);
    export.admit(USER, ChannelClass::User);
    (export, jobs, outcomes)
}

fn attach(
    export: &mut FactotumExport,
    connection: ConnectionId,
    aname: &[u8],
) -> Result<Node, Errno> {
    export
        .attach(&AttachContext {
            connection,
            peer: None,
            uname: b"",
            aname,
            n_uname: u32::MAX,
        })
        .map(|attachment| attachment.root)
}

/// Checks then opens, as the core does.
fn open(
    export: &mut FactotumExport,
    connection: ConnectionId,
    node: Node,
    flags: OpenFlags,
) -> Result<Handle, Errno> {
    let epoch = sophia_9p::Epoch(connection.0);
    export.check(&Access {
        connection,
        epoch,
        node: &node,
        operation: Operation::Open(flags),
    })?;
    export.open(&node, flags)
}

fn rpc(export: &mut FactotumExport, handle: &mut Handle, request: &str) -> ReadOutcome {
    export
        .write(&Node::Rpc, handle, 0, request.as_bytes())
        .unwrap();
    export.read(&Node::Rpc, handle, 0, 4096).unwrap()
}

fn ready(text: &str) -> ReadOutcome {
    ReadOutcome::Ready(text.as_bytes().to_vec())
}

#[test]
fn only_admitted_connections_attach_and_the_tree_is_9fronts() {
    let (mut export, _, _) = agent();
    assert_eq!(
        attach(&mut export, ConnectionId(9), b""),
        Err(Errno::EACCES)
    );
    assert_eq!(attach(&mut export, SESSION, b"other"), Err(Errno::ENOENT));
    let root = attach(&mut export, SESSION, b"").unwrap();
    assert_eq!(root, Node::Root);
    let dir = export.lookup(&root, WalkName::Child(b"factotum")).unwrap();
    assert_eq!(attach(&mut export, SESSION, b"factotum"), Ok(dir));
    let mut handle = open(&mut export, SESSION, dir, OpenFlags(0o200000)).unwrap();
    let names = export
        .readdir(&dir, &mut handle, 0, 16)
        .unwrap()
        .into_iter()
        .map(|entry| (String::from_utf8(entry.name).unwrap(), entry.entry.qid_path))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            ("confirm".into(), 5),
            ("needkey".into(), 8),
            ("ctl".into(), 7),
            ("rpc".into(), 2),
            ("proto".into(), 4),
            ("log".into(), 6),
        ]
    );
    assert_eq!(
        export.lookup(&dir, WalkName::Child(b"secstore")),
        Err(Errno::ENOENT)
    );
}

#[test]
fn modes_and_closed_prompt_files_refuse_opens() {
    let (mut export, _, _) = agent();
    assert_eq!(
        open(&mut export, SESSION, Node::Proto, READ_WRITE).err(),
        Some(Errno::EACCES)
    );
    assert_eq!(
        open(&mut export, SESSION, Node::Confirm, READ_WRITE).err(),
        Some(Errno::EACCES),
        "no trusted prompt yet"
    );
    assert_eq!(
        open(&mut export, SESSION, Node::Needkey, READ_WRITE).err(),
        Some(Errno::EACCES)
    );
    let log = open(&mut export, SESSION, Node::Log, READ).unwrap();
    assert_eq!(
        open(&mut export, SESSION, Node::Log, READ).err(),
        Some(Errno(16))
    );
    export.release(Node::Log, Some(log));
    assert!(open(&mut export, SESSION, Node::Log, READ).is_ok());
}

#[test]
fn ctl_adds_keys_that_list_without_secrets() {
    let (mut export, _, _) = agent();
    let mut ctl = open(&mut export, SESSION, Node::Ctl, READ_WRITE).unwrap();
    export
        .write(
            &Node::Ctl,
            &mut ctl,
            0,
            b"key proto=pass user=tb !password=secret",
        )
        .unwrap();
    assert_eq!(
        export.read(&Node::Ctl, &mut ctl, 0, 4096),
        Ok(ready("key proto=pass user=tb !password?\n"))
    );
    assert_eq!(
        export.write(&Node::Ctl, &mut ctl, 0, b"key proto=pam"),
        Err(Errno::EOPNOTSUPP)
    );
}

#[test]
fn a_pam_login_waits_on_its_job_and_answers_its_verdict() {
    let (mut export, jobs, outcomes) = agent();
    let mut handle = open(&mut export, SESSION, Node::Rpc, READ_WRITE).unwrap();
    assert_eq!(
        rpc(&mut export, &mut handle, "start proto=pam role=login"),
        ready("ok")
    );
    assert_eq!(rpc(&mut export, &mut handle, "write tb"), ready("ok"));
    assert_eq!(
        rpc(&mut export, &mut handle, "write hunter2"),
        ReadOutcome::Pending
    );
    let (conversation, job) = jobs.0.lock().unwrap().pop().unwrap();
    let Job::Pam(request) = job;
    assert_eq!(request.secret.as_slice(), b"hunter2");
    assert_eq!(
        export.read(&Node::Rpc, &mut handle, 0, 4096),
        Ok(ReadOutcome::Pending),
        "a retried read waits for the verdict"
    );
    outcomes
        .send((conversation, JobOutcome::Pam(PamVerdict::Accepted)))
        .unwrap();
    assert_eq!(
        export.read(&Node::Rpc, &mut handle, 0, 4096),
        Ok(ready("ok"))
    );
}

#[test]
fn a_user_connection_cannot_run_the_pam_login() {
    let (mut export, jobs, _) = agent();
    let mut handle = open(&mut export, USER, Node::Rpc, READ_WRITE).unwrap();
    assert_eq!(
        rpc(&mut export, &mut handle, "start proto=pam role=login"),
        ready("error pam login not permitted")
    );
    assert!(jobs.0.lock().unwrap().is_empty());
}

#[test]
fn an_rpc_opened_without_a_check_is_the_least_privileged_class() {
    let (mut export, _, _) = agent();
    let mut handle = export.open(&Node::Rpc, READ_WRITE).unwrap();
    assert_eq!(
        rpc(&mut export, &mut handle, "start proto=pam role=login"),
        ready("error pam login not permitted")
    );
}

#[test]
fn file_level_rpc_errors_are_errno_values() {
    let (mut export, _, _) = agent();
    let mut handle = open(&mut export, SESSION, Node::Rpc, READ_WRITE).unwrap();
    assert_eq!(
        export.read(&Node::Rpc, &mut handle, 0, 4096),
        Err(Errno(42)),
        "no rpc pending"
    );
    export.write(&Node::Rpc, &mut handle, 0, b"read").unwrap();
    assert_eq!(
        export.write(&Node::Rpc, &mut handle, 0, b"read"),
        Err(Errno(16)),
        "already pending"
    );
    assert_eq!(
        export.read(&Node::Rpc, &mut handle, 0, 63),
        Err(Errno::EINVAL)
    );
}

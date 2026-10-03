//! The `rpc` file against 9front's `rpcwrite`/`rpcread`, with `pass` and the
//! Linux `pam` login. Jobs are completed by the test, as the agent's workers
//! would complete them.

use sophia_factotum::conversation::{Conversation, RpcFileError, RpcRead};
use sophia_factotum::ctl;
use sophia_factotum::keyring::Keyring;
use sophia_factotum::logbuf::LogBuf;
use sophia_factotum::proto::{ChannelClass, Env, Job, JobOutcome, PamVerdict, Settings};

struct Agent {
    keyring: Keyring,
    log: LogBuf,
    settings: Settings,
    sequence: u32,
}

impl Agent {
    fn new() -> Self {
        Self {
            keyring: Keyring::new(),
            log: LogBuf::new(),
            settings: Settings {
                owner: "tb".into(),
                pam_service: "sophia-lock".into(),
            },
            sequence: 0,
        }
    }

    fn env(&mut self, channel: ChannelClass) -> Env<'_> {
        Env {
            keyring: &mut self.keyring,
            log: &mut self.log,
            settings: &self.settings,
            channel,
            sequence: &mut self.sequence,
        }
    }

    /// One RPC: write, then a read of `count` bytes.
    fn rpc_on(
        &mut self,
        cx: &mut Conversation,
        channel: ChannelClass,
        request: &[u8],
        count: usize,
    ) -> RpcRead {
        cx.write(request).unwrap();
        cx.read(&mut self.env(channel), count)
    }

    fn rpc(&mut self, cx: &mut Conversation, request: &str) -> String {
        match self.rpc_on(cx, ChannelClass::Session, request.as_bytes(), 4096) {
            RpcRead::Reply(reply) => String::from_utf8(reply).unwrap(),
            other => panic!("{request}: {other:?}"),
        }
    }
}

#[test]
fn file_level_refusals() {
    let mut agent = Agent::new();
    let mut cx = Conversation::new();
    assert!(matches!(
        cx.read(&mut agent.env(ChannelClass::Session), 4096),
        RpcRead::Refused(RpcFileError::NoRpcPending)
    ));
    assert_eq!(cx.write(&[b'x'; 4096]), Err(RpcFileError::TooLarge));
    assert_eq!(cx.write(b"read"), Ok(4));
    assert_eq!(cx.write(b"read"), Err(RpcFileError::AlreadyPending));
    assert!(matches!(
        cx.read(&mut agent.env(ChannelClass::Session), 63),
        RpcRead::Refused(RpcFileError::ReadTooSmall)
    ));
}

#[test]
fn verbs_before_a_protocol_answer_in_9fronts_words() {
    let mut agent = Agent::new();
    let mut cx = Conversation::new();
    assert_eq!(agent.rpc(&mut cx, "frob"), "error unknown verb");
    assert_eq!(agent.rpc(&mut cx, "read"), "error no current protocol");
    assert_eq!(
        agent.rpc(&mut cx, "start user=x"),
        "error did not specify proto"
    );
    assert_eq!(
        agent.rpc(&mut cx, "start proto=nope"),
        "error unknown protocol nope"
    );
    assert_eq!(
        agent.rpc(&mut cx, "authinfo"),
        "error authentication unfinished"
    );
    assert_eq!(agent.rpc(&mut cx, "attr"), "ok ");
}

#[test]
fn pass_hands_back_the_matching_key() {
    let mut agent = Agent::new();
    ctl::write(&mut agent.keyring, "key proto=pass user=tb !password='s e'").unwrap();
    let mut cx = Conversation::new();
    assert_eq!(agent.rpc(&mut cx, "start proto=pass"), "ok");
    assert_eq!(agent.rpc(&mut cx, "attr"), "ok proto=pass user=tb");
    assert_eq!(agent.rpc(&mut cx, "read"), "ok tb 's e'");
    assert_eq!(
        agent.rpc(&mut cx, "read"),
        "ok tb 's e'",
        "pass never finishes"
    );
    assert_eq!(
        agent.rpc(&mut cx, "read x"),
        "error read needs no parameters"
    );
    assert_eq!(
        agent.rpc(&mut cx, "write anything"),
        "phase protocol phase error: write in state 0",
        "9front never installs pass's phase names"
    );
}

#[test]
fn pass_without_a_key_asks_for_one_with_a_sorted_template() {
    let mut agent = Agent::new();
    let mut cx = Conversation::new();
    assert_eq!(
        agent.rpc(&mut cx, "start proto=pass role=client"),
        "needkey !password? proto=pass user?"
    );
    assert_eq!(agent.rpc(&mut cx, "read"), "error no current protocol");
}

#[test]
fn a_read_too_small_for_the_answer_says_how_much_it_needs() {
    let mut agent = Agent::new();
    let password = "p".repeat(100);
    ctl::write(
        &mut agent.keyring,
        &format!("key proto=pass user=tb !password={password}"),
    )
    .unwrap();
    let mut cx = Conversation::new();
    assert_eq!(agent.rpc(&mut cx, "start proto=pass"), "ok");
    let RpcRead::Reply(reply) = agent.rpc_on(&mut cx, ChannelClass::Session, b"read", 64) else {
        panic!("a reply");
    };
    assert_eq!(reply, format!("toosmall {}", 3 + password.len()).as_bytes());
}

#[test]
fn a_second_start_closes_the_first() {
    let mut agent = Agent::new();
    ctl::write(&mut agent.keyring, "key proto=pass user=tb !password=x").unwrap();
    let mut cx = Conversation::new();
    assert_eq!(agent.rpc(&mut cx, "start proto=pass"), "ok");
    assert_eq!(agent.rpc(&mut cx, "start proto=pam role=login"), "ok");
    let logged = agent.log.read(4096).unwrap().unwrap();
    let logged = String::from_utf8(logged).unwrap();
    assert!(
        logged.contains("implicit close due to second start"),
        "{logged}"
    );
    assert!(!logged.contains("!password=x"));
}

/// Drives the pam login to its job and returns the job.
fn submit(agent: &mut Agent, cx: &mut Conversation, password: &str) -> Job {
    assert_eq!(agent.rpc(cx, "start proto=pam role=login"), "ok");
    assert_eq!(agent.rpc(cx, "write tb"), "ok");
    match agent.rpc_on(
        cx,
        ChannelClass::Session,
        format!("write {password}").as_bytes(),
        4096,
    ) {
        RpcRead::Started(job) => job,
        other => panic!("expected the pam job, got {other:?}"),
    }
}

#[test]
fn an_accepted_pam_login_establishes_with_the_owner_as_authinfo() {
    let mut agent = Agent::new();
    let mut cx = Conversation::new();
    let Job::Pam(request) = submit(&mut agent, &mut cx, "hunter2");
    assert_eq!(request.service, "sophia-lock");
    assert_eq!(request.user, "tb");
    assert_eq!(request.secret.as_slice(), b"hunter2");
    assert!(!format!("{request:?}").contains("hunter2"));

    assert!(matches!(
        cx.read(&mut agent.env(ChannelClass::Session), 4096),
        RpcRead::Waiting
    ));
    cx.finish_job(JobOutcome::Pam(PamVerdict::Accepted));
    let RpcRead::Reply(reply) = cx.read(&mut agent.env(ChannelClass::Session), 4096) else {
        panic!("the verdict's reply");
    };
    assert_eq!(reply, b"ok");
    assert_eq!(agent.rpc(&mut cx, "read"), "done haveai");
    let RpcRead::Reply(info) = agent.rpc_on(&mut cx, ChannelClass::Session, b"authinfo", 4096)
    else {
        panic!("authinfo");
    };
    assert_eq!(
        info,
        [
            b"ok ".as_slice(),
            &[2, 0],
            b"tb",
            &[2, 0],
            b"tb",
            &[0, 0],
            &[0, 0]
        ]
        .concat()
    );
}

#[test]
fn a_failed_pam_login_breaks_the_conversation() {
    for (verdict, reason) in [
        (PamVerdict::Rejected, "authentication failed"),
        (PamVerdict::Unavailable, "pam unavailable"),
        (PamVerdict::ConversationRefused, "pam conversation refused"),
        (PamVerdict::TimedOut, "pam timeout"),
        (PamVerdict::HelperFailed, "pam helper failed"),
    ] {
        let mut agent = Agent::new();
        let mut cx = Conversation::new();
        submit(&mut agent, &mut cx, "wrong");
        cx.finish_job(JobOutcome::Pam(verdict));
        let RpcRead::Reply(reply) = cx.read(&mut agent.env(ChannelClass::Session), 4096) else {
            panic!("the verdict's reply");
        };
        assert_eq!(String::from_utf8(reply).unwrap(), format!("error {reason}"));
        assert_eq!(
            agent.rpc(&mut cx, "write again"),
            format!("error {reason}"),
            "one verdict per attempt"
        );
        assert_eq!(
            agent.rpc(&mut cx, "authinfo"),
            "error authentication unfinished"
        );
    }
}

#[test]
fn pam_refuses_other_channels_users_and_roles() {
    let mut agent = Agent::new();
    let mut cx = Conversation::new();
    let RpcRead::Reply(reply) = agent.rpc_on(
        &mut cx,
        ChannelClass::User,
        b"start proto=pam role=login",
        4096,
    ) else {
        panic!("a reply");
    };
    assert_eq!(reply, b"error pam login not permitted");

    assert_eq!(
        agent.rpc(&mut cx, "start proto=pam"),
        "error role not specified"
    );
    assert_eq!(
        agent.rpc(&mut cx, "start proto=pam role=client"),
        "error unknown role client"
    );
    assert_eq!(
        agent.rpc(&mut cx, "start proto=pam role=login service=sudo"),
        "error unknown pam service"
    );
    assert_eq!(agent.rpc(&mut cx, "start proto=pam role=login"), "ok");
    assert_eq!(
        agent.rpc(&mut cx, "read"),
        "phase protocol phase error: read in state SNeedUser"
    );
    assert_eq!(agent.rpc(&mut cx, "write root"), "error user not permitted");
}

#[test]
fn pam_refuses_oversized_and_nul_bearing_secrets_without_a_job() {
    let mut agent = Agent::new();
    let mut cx = Conversation::new();
    assert_eq!(agent.rpc(&mut cx, "start proto=pam role=login"), "ok");
    assert_eq!(agent.rpc(&mut cx, "write tb"), "ok");
    let RpcRead::Reply(reply) = agent.rpc_on(&mut cx, ChannelClass::Session, b"write a\0b", 4096)
    else {
        panic!("no job for a NUL-bearing secret");
    };
    assert_eq!(reply, b"error bad password");

    let mut cx = Conversation::new();
    assert_eq!(agent.rpc(&mut cx, "start proto=pam role=login"), "ok");
    assert_eq!(agent.rpc(&mut cx, "write tb"), "ok");
    assert_eq!(
        agent.rpc(&mut cx, &format!("write {}", "x".repeat(513))),
        "error password too long"
    );
}

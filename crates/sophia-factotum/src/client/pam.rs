//! One PAM conversation, including clunk custody and an undelivered verdict.

use super::{ClientError, LoginVerdict, first_field};
use crate::secret::SecretBytes;
use sophia_9p::{Fid, Tag};
use sophia_9p_client::pipeline::{Pipeline, PipelineError, Reply};
use std::time::Instant;

#[derive(Clone, Copy)]
enum Step {
    Start,
    User,
    Password,
    AuthInfo,
}

enum State {
    Walk(Tag),
    OpenNext,
    Open(Tag),
    WriteNext(Step),
    Write(Tag, Step, usize),
    ReadNext(Step),
    Read(Tag, Step),
    ClunkNext,
    Clunk(Tag),
    Done,
}

pub(super) struct PamLogin {
    rpc: Fid,
    service: String,
    user: String,
    secret: Option<SecretBytes>,
    state: State,
    verdict: Option<LoginVerdict>,
    deadline: Instant,
}

impl PamLogin {
    pub fn begin(
        pipeline: &mut Pipeline,
        root: Fid,
        service: &str,
        user: &str,
        secret: &[u8],
        now: Instant,
        deadline: Instant,
    ) -> Result<Self, ClientError> {
        if now >= deadline {
            return Err(PipelineError::Timeout.into());
        }
        // Match PAM's bounds before retaining a secret. Service is trusted
        // configuration, but still bounded on this queued client path.
        if user.is_empty()
            || user.len() > 256
            || user.contains('\0')
            || service.is_empty()
            || service.len() > 256
            || service.contains('\0')
            || secret.len() > 512
            || secret.contains(&0)
        {
            return Err(ClientError::InvalidInput);
        }
        let (tag, rpc) = pipeline.walk(root, &[b"rpc"])?;
        Ok(Self {
            rpc,
            service: service.into(),
            user: user.into(),
            secret: Some(SecretBytes::from_slice(secret)),
            state: State::Walk(tag),
            verdict: None,
            deadline,
        })
    }

    pub fn owns(&self, tag: Tag) -> bool {
        match self.state {
            State::Walk(t)
            | State::Open(t)
            | State::Write(t, ..)
            | State::Read(t, ..)
            | State::Clunk(t) => t == tag,
            _ => false,
        }
    }

    pub fn check(&self, now: Instant) -> Result<(), ClientError> {
        if now >= self.deadline {
            Err(PipelineError::Timeout.into())
        } else {
            Ok(())
        }
    }

    pub fn advance(&mut self, pipeline: &mut Pipeline, now: Instant) -> Result<(), ClientError> {
        self.check(now)?;
        let next = match self.state {
            State::OpenNext => pipeline.lopen(self.rpc, 2).map(State::Open),
            State::WriteNext(step) => {
                // This temporary is zeroed even if capacity refuses the write.
                let request = zeroize::Zeroizing::new(match step {
                    Step::Start => format!(
                        "start proto=pam role=login service={}",
                        crate::attr::quote(&self.service)
                    )
                    .into_bytes(),
                    Step::User => [b"write ".as_slice(), self.user.as_bytes()].concat(),
                    Step::Password => {
                        let secret = self.secret.as_ref().expect("password not yet queued");
                        let mut bytes = Vec::with_capacity(6 + secret.len());
                        bytes.extend_from_slice(b"write ");
                        bytes.extend_from_slice(secret.as_slice());
                        bytes
                    }
                    Step::AuthInfo => b"authinfo".to_vec(),
                });
                let queued = if matches!(step, Step::Password) {
                    pipeline.write_secret(self.rpc, 0, &request)
                } else {
                    pipeline.write(self.rpc, 0, &request)
                };
                if queued.is_ok() && matches!(step, Step::Password) {
                    self.secret = None;
                }
                queued.map(|tag| State::Write(tag, step, request.len()))
            }
            State::ReadNext(step) => pipeline
                .read(self.rpc, 0, 4096)
                .map(|tag| State::Read(tag, step)),
            State::ClunkNext => pipeline.clunk(self.rpc).map(State::Clunk),
            _ => return Ok(()),
        };
        match next {
            Ok(state) => {
                self.state = state;
                Ok(())
            }
            // Retry the unsent step, never a password already queued.
            Err(PipelineError::Limit(_)) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn receive(&mut self, reply: Reply, now: Instant) -> Result<(), ClientError> {
        self.check(now)?;
        self.state = match (&self.state, reply) {
            (State::Walk(_), Reply::Walk(qids)) if qids.len() == 1 => State::OpenNext,
            (State::Open(_), Reply::Lopen { .. }) => State::WriteNext(Step::Start),
            (State::Write(_, step, size), Reply::Write(n)) if n as usize == *size => {
                State::ReadNext(*step)
            }
            (State::Read(_, step), Reply::Read(bytes)) => {
                if matches!(step, Step::AuthInfo) {
                    self.verdict = Some(match bytes.strip_prefix(b"ok ").and_then(first_field) {
                        Some(user) if user == self.user.as_bytes() => LoginVerdict::Accepted,
                        _ => LoginVerdict::Refused(
                            "authinfo names another user or is missing".into(),
                        ),
                    });
                    State::ClunkNext
                } else if bytes == b"ok" {
                    State::WriteNext(match step {
                        Step::Start => Step::User,
                        Step::User => Step::Password,
                        Step::Password => Step::AuthInfo,
                        Step::AuthInfo => unreachable!(),
                    })
                } else {
                    let reason = bytes.strip_prefix(b"error ").unwrap_or(&bytes);
                    self.verdict = Some(LoginVerdict::Refused(
                        String::from_utf8_lossy(reason).into_owned(),
                    ));
                    self.secret = None;
                    State::ClunkNext
                }
            }
            (State::Clunk(_), Reply::Clunk) => State::Done,
            (_, Reply::Error(errno)) => return Err(ClientError::Remote(errno)),
            _ => return Err(ClientError::Unexpected("PAM reply")),
        };
        Ok(())
    }

    pub fn take(&mut self, now: Instant) -> Result<Option<LoginVerdict>, ClientError> {
        self.check(now)?;
        if matches!(self.state, State::Done) {
            Ok(self.verdict.take())
        } else {
            Ok(None)
        }
    }
}

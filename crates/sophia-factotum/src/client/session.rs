//! One pipeline owns both classes of tags. Fatal failure closes it immediately;
//! no buffered authentication result survives loss of this owner.

use super::{ClientError, LoginVerdict, pam::PamLogin};
use crate::launch::{
    client::{Error as LaunchError, LaunchClient},
    wire::{Command, Response},
};
use sophia_9p::{Errno, Fid};
use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use std::{
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

pub struct SessionClient {
    pipeline: Option<Pipeline>,
    root: Fid,
    launch: Option<LaunchClient>,
    login: Option<PamLogin>,
}

impl SessionClient {
    /// Blocking startup, for the supervision thread. Both views belong to the
    /// same trusted Session connection; no new privileged stream is introduced.
    pub fn over(stream: UnixStream, timeout: Duration) -> Result<Self, ClientError> {
        let end = Instant::now() + timeout;
        let limits = PipelineLimits {
            msize: 16_384,
            ..PipelineLimits::default()
        };
        let mut pipeline = Pipeline::over(stream, limits, timeout)?;
        let (tag, root) = pipeline.attach(b"", b"factotum")?;
        match pipeline.wait(tag, end)? {
            Reply::Attach(_) => {}
            Reply::Error(errno) => return Err(ClientError::Remote(errno)),
            _ => return Err(ClientError::Unexpected("attach")),
        }
        let launch =
            LaunchClient::start(&mut pipeline, Instant::now(), end).map_err(ClientError::Launch)?;
        let mut client = Self {
            pipeline: Some(pipeline),
            root,
            launch: Some(launch),
            login: None,
        };
        loop {
            client.poll(Instant::now())?;
            if client.launch.as_ref().is_some_and(LaunchClient::ready) {
                return Ok(client);
            }
            client.wait_for_io(end.saturating_duration_since(Instant::now()))?;
        }
    }

    pub fn closed(&self) -> bool {
        self.pipeline.is_none()
    }

    /// Retire the shared transport and every buffered authentication result.
    pub fn close(&mut self) {
        self.pipeline = None;
        self.launch = None;
        self.login = None;
    }

    pub fn begin_login(
        &mut self,
        service: &str,
        user: &str,
        secret: &[u8],
        now: Instant,
        deadline: Instant,
    ) -> Result<(), ClientError> {
        let pipeline = self.pipeline.as_mut().ok_or(ClientError::Closed)?;
        if self.login.is_some() {
            return Err(ClientError::Busy);
        }
        match PamLogin::begin(pipeline, self.root, service, user, secret, now, deadline) {
            Ok(login) => {
                self.login = Some(login);
                Ok(())
            }
            Err(
                error @ (ClientError::Transport(PipelineError::Limit(_) | PipelineError::Timeout)
                | ClientError::InvalidInput),
            ) => Err(error),
            Err(error) => self.fail(error),
        }
    }

    /// No internal request queue. One launch exchange or undelivered result can
    /// coexist with one PAM login/result; Busy retains custody with the caller.
    pub fn submit_launch(
        &mut self,
        command: Command,
        now: Instant,
        deadline: Instant,
    ) -> Result<(), ClientError> {
        let pipeline = self.pipeline.as_mut().ok_or(ClientError::Closed)?;
        let launch = self
            .launch
            .as_mut()
            .expect("live pipeline owns launch client");
        let idle = launch.ready();
        match launch.submit(pipeline, command, now, deadline) {
            Ok(()) => Ok(()),
            Err(error @ (LaunchError::Busy | LaunchError::Transport(PipelineError::Limit(_)))) => {
                Err(ClientError::Launch(error))
            }
            Err(LaunchError::Deadline) if idle => Err(ClientError::Launch(LaunchError::Deadline)),
            Err(error) => self.fail(ClientError::Launch(error)),
        }
    }

    /// Bounded socket progress and tag dispatch. No waiting on PAM or readiness.
    pub fn poll(&mut self, now: Instant) -> Result<(), ClientError> {
        let result = self.progress(now);
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error),
        }
    }

    fn progress(&mut self, now: Instant) -> Result<(), ClientError> {
        self.advance(now)?;
        let pipeline = self.pipeline.as_mut().ok_or(ClientError::Closed)?;
        pipeline.poll()?;
        while let Some((tag, reply)) = pipeline.take_reply() {
            let launch = self.launch.as_mut().expect("live launch client");
            if launch.owns(tag) {
                launch
                    .receive(tag, reply, now)
                    .map_err(ClientError::Launch)?;
            } else if let Some(login) = self.login.as_mut().filter(|login| login.owns(tag)) {
                login.receive(reply, now)?;
            } else {
                return Err(ClientError::Unexpected("unowned reply tag"));
            }
        }
        // Queue the newly enabled step before a supervision thread waits for
        // readiness. Otherwise an unsent read could sleep until timeout.
        self.advance(now)
    }

    fn advance(&mut self, now: Instant) -> Result<(), ClientError> {
        let pipeline = self.pipeline.as_mut().ok_or(ClientError::Closed)?;
        if let Some(login) = self.login.as_mut() {
            login.advance(pipeline, now)?;
        }
        match self
            .launch
            .as_mut()
            .expect("live launch client")
            .advance(pipeline, now)
        {
            Ok(()) | Err(LaunchError::Transport(PipelineError::Limit(_))) => Ok(()),
            Err(error) => Err(ClientError::Launch(error)),
        }
    }

    pub fn take_login(&mut self, now: Instant) -> Result<Option<LoginVerdict>, ClientError> {
        if self.closed() {
            return Err(ClientError::Closed);
        }
        let Some(login) = self.login.as_mut() else {
            return Ok(None);
        };
        match login.take(now) {
            Ok(Some(verdict)) => {
                self.login = None;
                Ok(Some(verdict))
            }
            Ok(None) => Ok(None),
            Err(error) => self.fail(error),
        }
    }

    pub fn take_launch(
        &mut self,
        now: Instant,
    ) -> Result<Option<Result<Response, Errno>>, ClientError> {
        let launch = self.launch.as_mut().ok_or(ClientError::Closed)?;
        match launch.take(now) {
            Ok(result) => Ok(result),
            Err(error) => self.fail(ClientError::Launch(error)),
        }
    }

    /// Blocking compatibility path for off-owner-thread callers. Session's
    /// supervisor uses begin/poll/take so other work can enter between turns.
    pub fn login(
        &mut self,
        service: &str,
        user: &str,
        secret: &[u8],
        deadline: Instant,
    ) -> Result<LoginVerdict, ClientError> {
        self.begin_login(service, user, secret, Instant::now(), deadline)?;
        loop {
            self.poll(Instant::now())?;
            if let Some(verdict) = self.take_login(Instant::now())? {
                return Ok(verdict);
            }
            self.wait_for_io(deadline.saturating_duration_since(Instant::now()))?;
        }
    }

    fn wait_for_io(&mut self, maximum: Duration) -> Result<(), ClientError> {
        let result = self
            .pipeline
            .as_ref()
            .ok_or(ClientError::Closed)?
            .wait_for_io(maximum);
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error.into()),
        }
    }

    fn fail<T>(&mut self, error: ClientError) -> Result<T, ClientError> {
        // Drop the socket as well as buffered verdicts and zeroizing secrets.
        // Factotum's connection teardown reclaims its private control state.
        self.close();
        Err(error)
    }
}

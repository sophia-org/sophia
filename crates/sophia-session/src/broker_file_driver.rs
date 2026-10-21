//! Bounded synchronous bridge for Session's broker calls. The shared 9P service
//! remains the only socket driver; there is no envelope or reconnect fallback.
use crate::broker_file_owner::{BrokerCompletion, BrokerFileOwner};
use sophia_9p::Errno;
use sophia_protocol::broker_files::BrokerFileRequest;
use sophia_runtime::broker_files::BrokerFileService;
use std::{
    io,
    time::{Duration, Instant},
};

pub struct BrokerFileDriver<R: Copy + Eq> {
    service: BrokerFileService<BrokerFileOwner<R>>,
    next_request: Option<u64>,
    closed: bool,
}
impl<R: Copy + Eq> BrokerFileDriver<R> {
    /// Admission and protected launch have already happened in the service.
    pub fn new(service: BrokerFileService<BrokerFileOwner<R>>) -> Self {
        let next_request = service.export().custody().last_request().checked_add(1);
        Self {
            service,
            next_request,
            closed: false,
        }
    }
    pub fn owner(&self) -> &BrokerFileOwner<R> {
        self.service.export().owner()
    }
    pub fn close(&mut self) -> io::Result<()> {
        self.closed = true;
        self.service.close()
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    /// Service receipt acknowledgements and cleanup even between owner calls.
    pub fn poll(&mut self) -> io::Result<()> {
        let result = self.turn(Duration::ZERO);
        self.finish(result)
    }
    pub fn negotiate(&mut self, timeout: Duration) -> io::Result<()> {
        let result = deadline(timeout).and_then(|deadline| self.negotiate_until(deadline));
        self.finish(result)
    }
    pub fn request(
        &mut self,
        request: BrokerFileRequest<'_>,
        route: R,
        timeout: Duration,
    ) -> io::Result<BrokerCompletion> {
        let result =
            deadline(timeout).and_then(|deadline| self.request_until(request, route, deadline));
        self.finish(result)
    }
    fn finish<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            let _ = self.close();
        }
        result
    }
    fn turn(&mut self, wait: Duration) -> io::Result<()> {
        if self.closed || !self.service.turn(wait)? {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "broker service ended",
            ));
        }
        Ok(())
    }
    fn negotiate_until(&mut self, deadline: Instant) -> io::Result<()> {
        loop {
            let wait = remaining(deadline)?;
            if self.closed {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "broker closed"));
            }
            let custody = self.service.export().custody();
            if custody.negotiated() && custody.retained().is_none() {
                return Ok(());
            }
            self.turn(wait)?;
        }
    }
    fn request_until(
        &mut self,
        request: BrokerFileRequest<'_>,
        route: R,
        deadline: Instant,
    ) -> io::Result<BrokerCompletion> {
        self.negotiate_until(deadline)?;
        let id = self
            .next_request
            .ok_or_else(|| io::Error::other("broker request identity exhausted"))?;
        loop {
            let wait = remaining(deadline)?;
            let export = self.service.export_mut();
            export
                .owner_mut()
                .begin(id, request, route)
                .map_err(owner_error)?;
            match self.service.publish(id, request, Instant::now()) {
                Ok(()) => break,
                Err(error) => {
                    self.service.export_mut().owner_mut().abort_unpublished();
                    if error != Errno::EAGAIN {
                        return Err(owner_error(error));
                    }
                }
            }
            self.turn(wait)?;
        }
        self.next_request = id.checked_add(1);
        loop {
            if let Some(completion) = self.service.export_mut().owner_mut().take_completion() {
                return Ok(completion);
            }
            self.turn(remaining(deadline)?)?;
        }
    }
}
impl<R: Copy + Eq> Drop for BrokerFileDriver<R> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
fn owner_error(error: Errno) -> io::Error {
    io::Error::other(format!("broker owner: {error:?}"))
}
fn deadline(timeout: Duration) -> io::Result<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "broker deadline overflow"))
}
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|wait| !wait.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "broker deadline expired"))
}

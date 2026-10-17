//! Nonblocking version negotiation with caller time and a shared I/O budget.
use super::*;

pub struct VersionHandshake {
    stream: Option<UnixStream>,
    limits: PipelineLimits,
    output: Vec<u8>,
    written: usize,
    input: Vec<u8>,
    now: u64,
    deadline: u64,
    msize: Option<u32>,
    error: Option<PipelineError>,
}
impl VersionHandshake {
    /// Adopt a connected stream and queue Tversion without socket I/O. Time
    /// is caller-supplied monotonic milliseconds with one absolute deadline.
    pub fn new(
        stream: UnixStream,
        limits: PipelineLimits,
        now: u64,
        deadline: u64,
    ) -> Result<Self, PipelineError> {
        limits.validate()?;
        if deadline <= now {
            return Err(PipelineError::Timeout);
        }
        stream.set_read_timeout(None).map_err(io_err)?;
        stream.set_write_timeout(None).map_err(io_err)?;
        stream.set_nonblocking(true).map_err(io_err)?;
        let body = client_codec::version_body(limits.msize);
        let mut output = ((HEADER + body.len()) as u32).to_le_bytes().to_vec();
        output.push(client_codec::TVERSION);
        output.extend(client_codec::NOTAG.to_le_bytes());
        output.extend(body);
        Ok(Self {
            stream: Some(stream),
            limits,
            output,
            written: 0,
            input: vec![],
            now,
            deadline,
            msize: None,
            error: None,
        })
    }
    pub fn ready(&self) -> bool {
        self.msize.is_some() && self.stream.is_some() && self.error.is_none()
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.stream.as_ref().map(AsFd::as_fd)
    }
    pub fn io_interest(&self) -> IoInterest {
        IoInterest {
            readable: self.stream.is_some() && !self.ready(),
            writable: self.stream.is_some() && self.written < self.output.len(),
            immediate: self.ready(),
        }
    }
    /// Transfer ownership once; the pipeline has no fids and sends no second
    /// Tversion. The role must retain the original startup deadline.
    pub fn take_pipeline(&mut self) -> Option<Pipeline> {
        if !self.ready() {
            return None;
        }
        Some(Pipeline::negotiated(
            self.stream.take()?,
            self.limits,
            self.msize?,
        ))
    }
    pub fn close(&mut self) {
        self.stream.take();
        self.error
            .get_or_insert(PipelineError::Io(io::ErrorKind::UnexpectedEof));
    }
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), PipelineError> {
        self.poll_with_budget(now, &mut IoBudget::new(bytes))
    }
    pub fn poll_with_budget(
        &mut self,
        now: u64,
        budget: &mut IoBudget,
    ) -> Result<(), PipelineError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if now < self.now {
            return Err(PipelineError::Limit("clock regressed"));
        }
        self.now = now;
        let result = if now >= self.deadline {
            Err(PipelineError::Timeout)
        } else if self.ready() {
            Ok(())
        } else {
            self.service(budget)
        };
        if let Err(error) = &result {
            self.error = Some(error.clone());
            self.stream.take();
        }
        result
    }
    fn service(&mut self, budget: &mut IoBudget) -> Result<(), PipelineError> {
        let stream = self.stream.as_mut().ok_or(PipelineError::Poisoned)?;
        while self.written < self.output.len() && budget.write_calls != 0 && budget.write_bytes != 0
        {
            let end = self.output.len().min(self.written + budget.write_bytes);
            budget.write_calls -= 1;
            match stream.write(&self.output[self.written..end]) {
                Ok(0) => return Err(PipelineError::Io(io::ErrorKind::WriteZero)),
                Ok(n) => {
                    self.written += n;
                    budget.write_bytes -= n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(io_err(e)),
            }
        }
        if self.written != self.output.len() {
            return Ok(());
        }
        loop {
            let expected = if self.input.len() < 4 {
                4
            } else {
                let n = u32::from_le_bytes(self.input[..4].try_into().unwrap()) as usize;
                if n < HEADER || n > self.limits.msize as usize {
                    return Err(PipelineError::Protocol("version frame length"));
                }
                n
            };
            if self.input.len() == expected && expected != 4 {
                return self.accept();
            }
            if budget.read_bytes == 0 || budget.read_calls == 0 {
                return Ok(());
            }
            // Do not overread: following bytes must pass the pipeline's normal
            // tag validation after ownership transfers.
            let mut buffer = [0; 4096];
            let count = (expected - self.input.len())
                .min(buffer.len())
                .min(budget.read_bytes);
            budget.read_calls -= 1;
            match stream.read(&mut buffer[..count]) {
                Ok(0) => return Err(PipelineError::Io(io::ErrorKind::UnexpectedEof)),
                Ok(n) => {
                    self.input.extend_from_slice(&buffer[..n]);
                    budget.read_bytes -= n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(io_err(e)),
            }
        }
    }
    fn accept(&mut self) -> Result<(), PipelineError> {
        let kind = self.input[4];
        let tag = u16::from_le_bytes(self.input[5..7].try_into().unwrap());
        if tag != client_codec::NOTAG {
            return Err(PipelineError::Protocol("version reply tag"));
        }
        let body = &self.input[HEADER..];
        if kind == client_codec::RLERROR {
            return Err(client_codec::decode_rlerror(body)
                .map(PipelineError::Remote)
                .unwrap_or(PipelineError::Protocol("Rlerror shape")));
        }
        if kind != client_codec::RVERSION {
            return Err(PipelineError::Protocol("version reply type"));
        }
        let (msize, version) =
            client_codec::decode_rversion(body).ok_or(PipelineError::Protocol("Rversion shape"))?;
        client_codec::accept_rversion(self.limits.msize, msize, version)
            .map_err(PipelineError::Protocol)?;
        self.msize = Some(msize);
        Ok(())
    }
}

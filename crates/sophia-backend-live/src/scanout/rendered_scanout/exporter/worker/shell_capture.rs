//! Capture work stays on the existing context-owning renderer worker.
use super::*;
use sophia_renderer_live::{
    LiveRendererSnapshotBudget, LiveRendererSnapshotEpoch, LiveShellCaptureCustody,
    LiveShellCaptureExecutor, LiveShellCaptureInbox,
};
use std::os::fd::BorrowedFd;

impl NativeGbmRendererWorkerCore {
    pub(crate) fn request_shell_capture(
        &self,
        epoch: Arc<LiveRendererSnapshotEpoch>,
        budget: Arc<LiveRendererSnapshotBudget>,
        custody: LiveShellCaptureCustody,
    ) -> io::Result<()> {
        let mut pending = self
            .capture_install
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if pending.is_some() || self.control.is_shutdown() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let (completion_sender, receive) = sync_channel(1);
        self.command_sender
            .try_send(WorkerCommand::EnableShellCapture {
                epoch,
                budget,
                custody,
                completion_sender,
            })
            .map_err(|e| match e {
                TrySendError::Full(_) => io::Error::from(io::ErrorKind::WouldBlock),
                TrySendError::Disconnected(_) => io::Error::from(io::ErrorKind::BrokenPipe),
            })?;
        *pending = Some(receive);
        Ok(())
    }

    pub(crate) fn poll_shell_capture(&self) -> io::Result<Option<LiveShellCaptureInbox>> {
        let mut pending = self
            .capture_install
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(receive) = pending.as_ref() else {
            return Ok(None);
        };
        match receive.try_recv() {
            Ok(result) => {
                *pending = None;
                result.map(Some)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                *pending = None;
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
    }

    pub(crate) fn close_shell_capture(&self) -> io::Result<()> {
        self.command_sender
            .try_send(WorkerCommand::CloseShellCapture)
            .map_err(|e| match e {
                TrySendError::Full(_) => io::ErrorKind::WouldBlock.into(),
                TrySendError::Disconnected(_) => io::ErrorKind::BrokenPipe.into(),
            })
    }
}

impl NativeGbmRendererWorker {
    pub(crate) fn request_shell_capture(
        &self,
        epoch: Arc<LiveRendererSnapshotEpoch>,
        budget: Arc<LiveRendererSnapshotBudget>,
        custody: LiveShellCaptureCustody,
    ) -> io::Result<()> {
        self.core.request_shell_capture(epoch, budget, custody)
    }
    pub(crate) fn poll_shell_capture(&self) -> io::Result<Option<LiveShellCaptureInbox>> {
        self.core.poll_shell_capture()
    }
    pub(crate) fn close_shell_capture(&self) -> io::Result<()> {
        self.core.close_shell_capture()
    }
}

// Declared after the context in run_worker: Drop closes capture before context
// destruction on normal return and unwind. The backend also retains custody
// outside this worker, across replacements. No teardown path claims completion.
pub(super) struct Captures {
    executor: Option<LiveShellCaptureExecutor>,
    owner: Option<(LiveShellCaptureCustody, Arc<LiveRendererSnapshotBudget>)>,
    commands: sophia_wake::Wake,
    follow_up: Option<Duration>,
    wake_failed: bool,
    #[cfg(test)]
    fail_service_once: bool,
}

impl Captures {
    pub(super) fn new(control: &WorkerControl) -> io::Result<Self> {
        let commands = sophia_wake::Wake::new()?;
        control.command_wake.set(commands.notifier());
        Ok(Self {
            executor: None,
            owner: None,
            commands,
            follow_up: None,
            wake_failed: false,
            #[cfg(test)]
            fail_service_once: false,
        })
    }

    pub(super) fn install(
        &mut self,
        available: bool,
        epoch: Arc<LiveRendererSnapshotEpoch>,
        budget: Arc<LiveRendererSnapshotBudget>,
        custody: LiveShellCaptureCustody,
    ) -> io::Result<LiveShellCaptureInbox> {
        if self.wake_failed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if !available {
            return Err(io::ErrorKind::NotConnected.into());
        }
        if self.executor.is_some() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if !epoch.is_valid()
            || self.owner.as_ref().is_some_and(|(old, old_budget)| {
                !old.same_owner(&custody) || !Arc::ptr_eq(old_budget, &budget)
            })
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let (inbox, executor) = custody.channel(epoch, budget.clone())?;
        self.owner = Some((custody, budget));
        self.executor = Some(executor);
        self.follow_up = None;
        Ok(inbox)
    }

    pub(super) fn close(&mut self) {
        if let Some(mut executor) = self.executor.take() {
            executor.close();
        }
        self.follow_up = None;
    }

    pub(super) fn service<D: AsFd>(
        &mut self,
        context: &mut Option<NativeGbmRenderedScanoutContext<D>>,
    ) -> io::Result<()> {
        // Clear before producers/queue inspection, never after checking empty.
        if !self.wake_failed
            && let Err(error) = self.commands.clear()
        {
            self.wake_failed = true;
            self.close();
            return Err(error);
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_service_once) {
            self.close();
            return Err(io::Error::other("supplied capture service error"));
        }
        let Some(executor) = self.executor.as_mut() else {
            return Ok(());
        };
        let Some(context) = context.as_mut() else {
            self.close();
            return Ok(());
        };
        match executor.service(context, Instant::now(), 4) {
            Ok(progress) => {
                self.follow_up = follow_up(progress.budget_spent, progress.pending_fences)
            }
            Err(error) => {
                self.close();
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fail_next_service(&mut self) {
        self.fail_service_once = true;
    }

    pub(super) fn next(
        &self,
        receive: &Receiver<WorkerCommand>,
    ) -> io::Result<Option<WorkerCommand>> {
        let Some(executor) = self.executor.as_ref() else {
            return receive
                .recv()
                .map(Some)
                .map_err(|_| io::ErrorKind::BrokenPipe.into());
        };
        next_ready_command(receive, &self.commands, executor.as_fd(), self.follow_up)
    }
}

fn follow_up(budget_spent: bool, pending_fences: usize) -> Option<Duration> {
    if budget_spent {
        Some(Duration::ZERO)
    } else if pending_fences != 0 {
        Some(Duration::from_millis(1))
    } else {
        None
    }
}

fn next_ready_command(
    receive: &Receiver<WorkerCommand>,
    commands: &sophia_wake::Wake,
    requests: BorrowedFd<'_>,
    timeout: Option<Duration>,
) -> io::Result<Option<WorkerCommand>> {
    match receive.try_recv() {
        Ok(command) => return Ok(Some(command)),
        Err(TryRecvError::Disconnected) => return Err(io::ErrorKind::BrokenPipe.into()),
        Err(TryRecvError::Empty) => (),
    }
    let mut fds = [
        rustix::event::PollFd::new(commands, rustix::event::PollFlags::IN),
        rustix::event::PollFd::new(&requests, rustix::event::PollFlags::IN),
    ];
    let timeout = timeout
        .map(rustix::time::Timespec::try_from)
        .transpose()
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    match rustix::event::poll(&mut fds, timeout.as_ref()) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl Drop for Captures {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
#[path = "../../../../../tests/support/renderer_worker_shell_capture.rs"]
mod tests;

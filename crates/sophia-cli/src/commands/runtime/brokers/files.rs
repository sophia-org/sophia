use sophia_9p_client::pipeline::{IoInterest, Pipeline, PipelineLimits, Reply};
use sophia_broker::{MetadataBroker, MetadataBrokerRejection, file_client};
use sophia_broker_client::{
    Error as StateError,
    session::{Error, Session},
    startup::Opening,
};
use sophia_protocol::broker_files::BrokerFileRequest;
use std::{
    io,
    os::fd::BorrowedFd,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    super::run_required_protection_probe()?;
    let path = PathBuf::from(
        std::env::var_os(sophia_runtime::broker_files::SOPHIA_BROKER_9P_SOCKET_ENV)
            .ok_or("metadata broker requires SOPHIA_BROKER_9P_SOCKET")?,
    );
    let start = Instant::now();
    let mut wire = Pipeline::connect(&path, PipelineLimits::default(), Duration::from_secs(5))?;
    let (tag, root) = wire.attach(b"", b"")?;
    if !matches!(
        wire.wait(tag, start + Duration::from_secs(5))?,
        Reply::Attach(_)
    ) {
        return Err("broker attach refused".into());
    }
    let now = || start.elapsed().as_millis() as u64;
    let mut opening = Opening::from_attached(wire, root, now(), 5000).map_err(sdk_error)?;
    while !opening.ready() {
        opening.dispatch(now(), 65536).map_err(sdk_error)?;
        if !opening.ready() {
            wait(
                opening.fd(),
                opening.io_interest().map_err(sdk_error)?,
                opening.deadline(),
                now(),
            )?;
        }
    }
    let opened = opening.into_opened().map_err(sdk_error)?;
    let max_surfaces = opened.limits.max_surfaces as usize;
    let mut session = Session::new(opened, 2, 2, 5000).map_err(sdk_error)?;
    let default_class_only = std::env::var_os("SOPHIA_BROKER_DEFAULT_DISCLOSURE").as_deref()
        == Some(std::ffi::OsStr::new("class-only"));
    let mut broker = MetadataBroker::new();
    let mut retry_at = None;
    loop {
        let time = now();
        session.dispatch(time, 65536).map_err(sdk_error)?;
        while session.event().is_some() {
            session.consume(time).map_err(sdk_error)?;
        }
        match session.acknowledge(time) {
            Ok(()) | Err(Error::State(StateError::Busy)) => (),
            Err(error) => return Err(sdk_error(error).into()),
        }
        if session.transaction_ready()
            && let Some(pending) = session.request()
        {
            let id = pending.id;
            let request = file_client::request(pending).map_err(sdk_error)?;
            let result = if matches!(request,BrokerFileRequest::SurfaceAdmitted {surface,..}
                if surface.is_valid() && broker.rule_for(surface).is_none() && broker.len()>=max_surfaces)
            {
                Err(MetadataBrokerRejection::CapacityExhausted)
            } else {
                broker.file_request(request, default_class_only)
            };
            let (rows, count) = file_client::response_rows(&result).map_err(sdk_error)?;
            session
                .respond(id, &rows[..count], time)
                .map_err(sdk_error)?;
        }
        // Retry only the SDK's retained immutable submission. Never run the
        // reducer twice for one request; backpressure retains its original due time.
        if session.retry_ready() {
            let due = *retry_at.get_or_insert(time.saturating_add(4));
            if time >= due {
                session.retry(time).map_err(sdk_error)?;
                retry_at = None;
            }
        } else {
            retry_at = None;
        }
        let deadline = [session.deadline(), retry_at].into_iter().flatten().min();
        wait(
            session.fd(),
            session.io_interest().map_err(sdk_error)?,
            deadline,
            now(),
        )?;
    }
}

fn sdk_error(error: impl std::fmt::Debug) -> io::Error {
    io::Error::other(format!("broker SDK: {error:?}"))
}
fn wait(
    fd: Option<BorrowedFd<'_>>,
    interest: IoInterest,
    deadline: Option<u64>,
    now: u64,
) -> io::Result<()> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    if interest.immediate {
        return Ok(());
    }
    let fd = fd.ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "broker disconnected"))?;
    let mut flags = PollFlags::empty();
    if interest.readable {
        flags |= PollFlags::IN;
    }
    if interest.writable {
        flags |= PollFlags::OUT;
    }
    let timeout = deadline
        .map(|due| Timespec::try_from(Duration::from_millis(due.saturating_sub(now))))
        .transpose()
        .map_err(io::Error::other)?;
    match poll(&mut [PollFd::new(&fd, flags)], timeout.as_ref()) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

//! Private installation/wait seams; real mailbox, no GPU completion supplied.
use super::*;
use sophia_renderer_live::{
    LiveShellCaptureOutcome, LiveShellCaptureRequest, LiveShellCaptureSource,
};
use std::os::unix::net::UnixStream;

fn request(
    epoch: Arc<LiveRendererSnapshotEpoch>,
    budget: Arc<LiveRendererSnapshotBudget>,
) -> LiveShellCaptureRequest {
    let (fd, _) = UnixStream::pair().unwrap();
    LiveShellCaptureRequest {
        source: LiveShellCaptureSource {
            fd: fd.into(),
            width: 4,
            height: 4,
            stride: 16,
            modifier: 0,
            offset: 0,
        },
        image_id: LiveRendererImageId::from_raw(1),
        reservation: budget.reserve_capture(64).unwrap(),
        budget,
        epoch,
    }
}

#[test]
fn capture_install_preserves_owner_budget_and_closes_queued_custody() {
    let control = WorkerControl::default();
    let mut captures = Captures::new(&control).unwrap();
    let budget = LiveRendererSnapshotBudget::new(4, 256);
    let custody = LiveShellCaptureCustody::default();
    let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
    assert!(
        captures
            .install(false, epoch.clone(), budget.clone(), custody.clone())
            .is_err()
    );
    let mut inbox = captures
        .install(true, epoch.clone(), budget.clone(), custody.clone())
        .unwrap();
    assert!(
        captures
            .install(true, epoch.clone(), budget.clone(), custody.clone())
            .is_err()
    );
    inbox
        .submit(
            request(epoch.clone(), budget.clone()),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    captures.close();
    assert!(!epoch.is_valid());
    let batch = inbox.take_completed(Instant::now()).unwrap();
    assert_eq!(batch.receipts.len(), 1);
    assert!(matches!(
        batch.receipts[0].outcome(),
        LiveShellCaptureOutcome::Returned { .. }
    ));
    assert_eq!(budget.usage(), (1, 64));
    drop(batch);
    assert_eq!(budget.usage(), (0, 0));
    let next = LiveRendererSnapshotEpoch::new(1, 3, 3);
    assert!(
        captures
            .install(
                true,
                next.clone(),
                LiveRendererSnapshotBudget::new(4, 256),
                custody.clone()
            )
            .is_err()
    );
    assert!(
        captures
            .install(
                true,
                next.clone(),
                budget.clone(),
                LiveShellCaptureCustody::default()
            )
            .is_err()
    );
    let _next = captures
        .install(true, next.clone(), budget, custody)
        .unwrap();
    drop(captures);
    assert!(
        !next.is_valid(),
        "owner unwind/drop closes before context destruction"
    );
}

#[test]
fn request_readiness_wakes_without_a_render_command_or_idle_timer() {
    let control = Arc::new(WorkerControl::default());
    let commands = sophia_wake::Wake::new().unwrap();
    control.command_wake.set(commands.notifier());
    commands.clear().unwrap();
    let (send, receive) = sync_channel(1);
    let _send = wrap_commands(send, &control);
    let requests = sophia_wake::Wake::new().unwrap();
    let notify = requests.notifier();
    let (done, observed) = sync_channel(1);
    // Only move the wait fds, never the !Send capture executor.
    let thread = std::thread::spawn(move || {
        let result = next_ready_command(&receive, &commands, requests.as_fd(), None);
        done.send(result.map(|r| r.is_none())).unwrap();
    });
    notify.notify();
    let result = observed.recv_timeout(Duration::from_secs(2));
    control.shutdown(); // Watchdog wake prevents a failed test stranding a thread.
    thread.join().unwrap();
    assert!(result.unwrap().unwrap());
}

#[test]
fn fence_only_follow_up_and_budget_yield_do_not_need_new_commands() {
    assert_eq!(follow_up(false, 0), None);
    assert_eq!(follow_up(true, 0), Some(Duration::ZERO));
    let timeout = follow_up(false, 1);
    assert!(timeout.is_some());
    let commands = sophia_wake::Wake::new().unwrap();
    let wake = commands.notifier();
    let requests = sophia_wake::Wake::new().unwrap();
    let (send, receive) = sync_channel(1);
    let (done, observed) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = next_ready_command(&receive, &commands, requests.as_fd(), timeout);
        done.send(result.map(|r| r.is_none())).unwrap();
    });
    let result = observed.recv_timeout(Duration::from_secs(2));
    wake.notify();
    thread.join().unwrap();
    drop(send);
    assert!(result.unwrap().unwrap());
}

#[test]
fn command_publication_and_shutdown_wake_the_same_wait() {
    let control = Arc::new(WorkerControl::default());
    let commands = sophia_wake::Wake::new().unwrap();
    control.command_wake.set(commands.notifier());
    commands.clear().unwrap();
    let (send, receive) = sync_channel(1);
    let send = wrap_commands(send, &control);
    send.try_send(WorkerCommand::CloseShellCapture).unwrap();
    assert!(commands.wait(Some(Instant::now())).unwrap());
    commands.clear().unwrap();
    assert!(matches!(
        receive.try_recv(),
        Ok(WorkerCommand::CloseShellCapture)
    ));
    control.shutdown();
    assert!(commands.wait(Some(Instant::now())).unwrap());
}

#[test]
fn real_worker_refuses_install_without_context_and_shuts_down() {
    let control = Arc::new(WorkerControl::default());
    let service_control = control.clone();
    let (send, receive) = sync_channel(4);
    let send = wrap_commands(send, &control);
    let thread = std::thread::spawn(move || {
        super::super::service::run_worker::<std::fs::File>(
            Err(io::Error::other("no GPU in test")),
            Vec::new(),
            receive,
            service_control,
        )
    });
    let (reply, receive) = sync_channel(1);
    // A capture-local failure must not destroy the renderer command owner.
    send.try_send(WorkerCommand::FailCaptureService).unwrap();
    send.try_send(WorkerCommand::EnableShellCapture {
        epoch: LiveRendererSnapshotEpoch::new(1, 2, 3),
        budget: LiveRendererSnapshotBudget::new(4, 256),
        custody: LiveShellCaptureCustody::default(),
        completion_sender: reply,
    })
    .unwrap();
    let result = receive.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::NotConnected));
    control.shutdown();
    let _ = send.try_send(WorkerCommand::Shutdown);
    thread.join().unwrap();
}

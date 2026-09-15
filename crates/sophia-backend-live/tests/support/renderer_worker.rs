#![cfg(test)]

use super::reusable_cpu_buffer_damage;
use sophia_protocol::Size;

#[test]
fn changed_cpu_buffer_without_snapshot_repaints_the_full_output() {
    let size = Size {
        width: 3840,
        height: 960,
    };
    let damage = reusable_cpu_buffer_damage(1, None, 2, None, size);

    assert_eq!(damage.len(), 1);
    assert_eq!(damage[0].width, size.width);
    assert_eq!(damage[0].height, size.height);
}

#[test]
fn unchanged_cpu_buffer_requires_no_rewrite() {
    let damage = reusable_cpu_buffer_damage(
        7,
        None,
        7,
        None,
        Size {
            width: 1920,
            height: 1080,
        },
    );

    assert!(damage.is_empty());
}

// The shared worker's routing invariants, driven over its own channels with
// no device behind it. `validation/tla/SharedWorkerService.tla` states these;
// a render that fails for want of a GPU still exercises every one of them,
// because they are about which output a message belongs to rather than about
// pixels.

use super::service::run_worker;
use super::{
    LiveRendererWorkerOutputKey, LiveRendererWorkerRequestId, WorkerCommand, WorkerOutcome,
    WorkerResult,
};
use crate::api::LiveGbmEglFrameTargetRecord;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

const SETTLE: Duration = Duration::from_secs(2);

fn worker_channel() -> (SyncSender<WorkerCommand>, std::thread::JoinHandle<()>) {
    let (commands, command_receiver) = sync_channel(32);
    let thread = std::thread::spawn(move || {
        run_worker::<std::fs::File>(
            Err(std::io::Error::other("no render device in this test")),
            Vec::new(),
            command_receiver,
            std::sync::Arc::new(super::WorkerControl::default()),
        )
    });
    (commands, thread)
}

fn register(
    commands: &SyncSender<WorkerCommand>,
    output: LiveRendererWorkerOutputKey,
) -> Receiver<WorkerResult> {
    register_claim(
        commands,
        output,
        std::sync::Arc::new(super::OutputClaim::default()),
    )
}

fn register_claim(
    commands: &SyncSender<WorkerCommand>,
    output: LiveRendererWorkerOutputKey,
    claim: std::sync::Arc<super::OutputClaim>,
) -> Receiver<WorkerResult> {
    let (reply, results) = sync_channel(2);
    commands
        .send(WorkerCommand::Register {
            output,
            reply,
            frame_slot_metrics: super::LiveRendererFrameSlotMetricsHandle::default(),
            claim,
        })
        .expect("worker accepts a registration");
    results
}

fn render(commands: &SyncSender<WorkerCommand>, output: LiveRendererWorkerOutputKey, request: u64) {
    commands
        .send(WorkerCommand::Render {
            output,
            request_id: LiveRendererWorkerRequestId(request),
            target: LiveGbmEglFrameTargetRecord::new(Size {
                width: 1920,
                height: 1080,
            }),
            frame: super::PendingRenderedFrame::Cpu {
                native: None,
                frame: sophia_renderer_live::LiveCpuComposedFrame {
                    size: Size {
                        width: 1920,
                        height: 1080,
                    },
                    stride: 1920 * 4,
                    format: 0,
                    bytes: std::sync::Arc::new(vec![0; 8]),
                },
                checksum: 1,
                damage_snapshot: None,
            },
            preferred_modifiers: Vec::new(),
            output_format: None,
        })
        .expect("worker accepts a render");
}

/// `ResponsesRouteToTheirOutput`: a result reaches the output that asked and
/// no other. On a per-head worker this was true by position; sharing the
/// thread makes it a claim, and the model violates the invariant at depth 5
/// when every result is routed to one fixed output instead.
#[test]
fn a_result_reaches_only_the_output_that_asked() {
    let (commands, thread) = worker_channel();
    let first = LiveRendererWorkerOutputKey::from_raw(1);
    let second = LiveRendererWorkerOutputKey::from_raw(2);
    let first_results = register(&commands, first);
    let second_results = register(&commands, second);

    render(&commands, second, 7);

    let result = second_results
        .recv_timeout(SETTLE)
        .expect("the output that asked receives its result");
    assert_eq!(result.output, second);
    assert_eq!(result.request_id, LiveRendererWorkerRequestId(7));
    assert!(
        first_results.try_recv().is_err(),
        "an output that asked for nothing must receive nothing"
    );

    drop(commands);
    let _ = thread.join();
}

/// Two outputs answering to one key would share slots, leases, and a reply
/// route while each believed it had its own. The key is composed to make that
/// impossible; the worker refuses it anyway, so the guarantee is checked
/// rather than argued.
#[test]
fn a_duplicate_output_registration_is_refused() {
    let (commands, thread) = worker_channel();
    let output = LiveRendererWorkerOutputKey::from_raw(3);
    let first_results = register(&commands, output);
    let usurper_results = register(&commands, output);

    render(&commands, output, 11);

    let result = first_results
        .recv_timeout(SETTLE)
        .expect("the first registration keeps the route");
    assert_eq!(result.output, output);
    assert!(
        usurper_results.try_recv().is_err(),
        "a duplicate registration must not take over an established route"
    );

    drop(commands);
    let _ = thread.join();
}

/// A render for an output the worker does not know has nowhere to report and
/// no slots to draw into. Dropping it is the only honest answer; delivering
/// it to whoever polled first is the misroute the model forbids.
#[test]
fn a_render_for_an_unknown_output_is_dropped_not_misdelivered() {
    let (commands, thread) = worker_channel();
    let known = LiveRendererWorkerOutputKey::from_raw(4);
    let known_results = register(&commands, known);

    render(&commands, LiveRendererWorkerOutputKey::from_raw(99), 13);
    render(&commands, known, 14);

    let result = known_results
        .recv_timeout(SETTLE)
        .expect("the registered output still gets served");
    assert_eq!(result.output, known);
    assert_eq!(
        result.request_id,
        LiveRendererWorkerRequestId(14),
        "the unknown output's request must not be answered on this route"
    );

    drop(commands);
    let _ = thread.join();
}

/// Deregistration returns an output's state. A later render for it is then a
/// render for an output the worker does not know, and is dropped rather than
/// drawn into slots that no longer belong to anyone.
#[test]
fn a_deregistered_output_is_served_no_further() {
    let (commands, thread) = worker_channel();
    let leaving = LiveRendererWorkerOutputKey::from_raw(5);
    let staying = LiveRendererWorkerOutputKey::from_raw(6);
    let claim = std::sync::Arc::new(super::OutputClaim::default());
    let leaving_results = register_claim(&commands, leaving, std::sync::Arc::clone(&claim));
    let staying_results = register(&commands, staying);

    commands
        .send(WorkerCommand::Deregister {
            output: leaving,
            claim,
        })
        .expect("worker accepts a deregistration");
    render(&commands, leaving, 21);
    render(&commands, staying, 22);

    let result = staying_results
        .recv_timeout(SETTLE)
        .expect("the attached output is still served");
    assert_eq!(result.output, staying);
    assert!(
        leaving_results.try_recv().is_err(),
        "a detached output must receive nothing further"
    );
    assert!(matches!(result.outcome, WorkerOutcome::Failed(_)));

    drop(commands);
    let _ = thread.join();
}

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/renderer_worker_lifecycle.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/renderer_worker_correlation.rs"
));

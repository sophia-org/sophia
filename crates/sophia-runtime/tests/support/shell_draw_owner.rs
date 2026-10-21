//! Owner transitions with supplied admission; real socket admission is tested
//! separately by shell_draw. Completions are deliberately withheld to test custody.
use super::draw::DrawJobs;
use super::*;
use crate::shell_transport::files::{JournalBounds, ShellFileWire, ShellFiles};
use crate::{ContentEpochRegistry, IsbValidationWorker};
use sophia_draw::{Batch, Command};
use sophia_protocol::*;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    transport: ShellComponentTransport,
    registry: ContentEpochRegistry,
    worker: IsbValidationWorker,
    jobs: DrawJobs,
    batch: Batch,
    _peer: UnixStream,
    directory: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "draw-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let grant = ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        };
        let mut limits = ContentLimits::prototype(grant);
        limits.max_live_resources = 1;
        let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        registry.admit(limits.clone()).unwrap();
        let mut export = ShellFiles::awaiting_negotiation(
            1,
            "bar",
            false,
            JournalBounds {
                bytes: 65536,
                reserve_bytes: 12288,
            },
            100,
            Instant::now(),
        );
        export
            .complete_negotiation(Some((vec![], limits.clone())), 0)
            .unwrap();
        let (server, peer) = UnixStream::pair().unwrap();
        transport.wire = Some(Box::new(
            ShellFileWire::adopt(server, export, None, false).unwrap(),
        ));
        transport.connection_epoch = 1;
        transport.store_grant = grant;
        transport.content_grant = Some(grant);
        transport.content_limits = Some(limits);
        Self {
            transport,
            registry,
            worker: IsbValidationWorker::new(4, 512 * 1024).unwrap(),
            jobs: DrawJobs::default(),
            batch: Batch {
                transaction: 1,
                grant,
                resource: ContentResourceId {
                    id: 1,
                    generation: 1,
                },
                width: 64,
                height: 24,
                commands: vec![Command::Rect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 24,
                    rgb: [10, 20, 30],
                }],
            },
            _peer: peer,
            directory,
        }
    }
    fn enqueue(&mut self) {
        self.transport
            .files_mut()
            .unwrap()
            .export_mut()
            .write_draw(&self.batch.encode().unwrap())
            .unwrap();
    }
    fn service(&mut self, now: u64, instant: Instant) {
        self.jobs
            .service(
                &mut self.transport,
                &mut self.registry,
                &mut self.worker,
                now,
                instant,
            )
            .unwrap();
    }
    fn completion(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(done) = self.worker.take_completed().unwrap() {
                assert!(self.jobs.receive(done).is_ok());
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::yield_now();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
#[test]
fn draw_cancellation_and_expiry_hold_charges_until_completion() {
    for timeout in [false, true] {
        let mut f = Fixture::new();
        f.enqueue();
        f.service(0, Instant::now());
        assert_eq!(f.jobs.pending(), 1);
        let bytes = f.batch.pixel_bytes() as u64;
        assert_eq!(f.registry.accounting().memory.staging, bytes);
        if timeout {
            f.service(1, Instant::now() + Duration::from_secs(3));
        } else {
            f.registry
                .resources_mut(f.batch.grant)
                .unwrap()
                .cancel(
                    TransactionId::from_raw(2),
                    &ContentResourceCancel {
                        grant: f.batch.grant,
                        resource: f.batch.resource,
                    },
                )
                .unwrap();
            f.service(1, Instant::now());
        }
        assert_eq!(f.registry.accounting().memory.staging, bytes);
        f.completion();
        assert_eq!(
            f.jobs.pending(),
            1,
            "receiving completion alone does not settle the store"
        );
        f.service(2, Instant::now());
        assert_eq!(f.jobs.pending(), 0);
        assert_eq!(f.registry.accounting().memory.staging, 0);
        assert_eq!(f.registry.accounting().memory.resident, 0);
        let event = f
            .registry
            .resources_mut(f.batch.grant)
            .unwrap()
            .take_event()
            .unwrap();
        let ShellContentRecord::ResourceStatus(status) = event.record else {
            panic!("status")
        };
        assert_eq!(status.status, if timeout { 3 } else { 4 });
        assert_eq!(
            status.reason,
            if timeout {
                ContentReason::Timeout
            } else {
                ContentReason::Cancelled
            } as u16
        );
    }
}
#[test]
fn disconnected_draw_settles_the_retired_epoch_without_acceptance() {
    let mut f = Fixture::new();
    f.enqueue();
    f.service(0, Instant::now());
    f.transport.disconnect(&mut f.registry).unwrap();
    assert_eq!(
        f.registry.accounting().memory.staging,
        f.batch.pixel_bytes() as u64
    );
    f.service(1, Instant::now());
    f.completion();
    f.service(2, Instant::now());
    assert_eq!(f.jobs.pending(), 0);
    assert_eq!(f.registry.accounting().memory.staging, 0);
    assert_eq!(f.registry.accounting().memory.resident, 0);
}
#[test]
fn expired_queue_and_full_resource_budget_start_no_raster_job() {
    let mut f = Fixture::new();
    f.enqueue();
    f.service(0, Instant::now() + Duration::from_secs(3));
    assert_eq!(f.jobs.pending(), 0);
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(f.registry.accounting().memory.staging, 0);
    f.batch.transaction += 1;
    f.enqueue();
    f.service(1, Instant::now());
    f.completion();
    f.service(2, Instant::now());
    assert_eq!(
        f.registry.accounting().memory.resident,
        f.batch.pixel_bytes() as u64
    );
    f.batch.transaction += 1;
    f.batch.resource.id += 1;
    f.enqueue();
    f.service(3, Instant::now());
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(f.jobs.pending(), 0);
    assert_eq!(f.registry.accounting().memory.staging, 0);
    assert_eq!(
        f.registry.accounting().memory.resident,
        f.batch.pixel_bytes() as u64
    );
}

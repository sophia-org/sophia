//! Real Session policy, coordinator, source and recipient with supplied native
//! facts. No device access; native rendering has separate isolated evidence.
#![cfg(test)]
use super::backend::PresentedCapture;
use super::*;
use sophia_protocol::output_capture_request::OutputCaptureRequest;

struct Backend {
    targets: BTreeMap<OutputId, LiveOutputCaptureTarget>,
    ready: bool,
    retained: bool,
    queued: u64,
    foreign: bool,
}
impl CaptureBackend for Backend {
    fn target(&self, output: OutputId) -> Option<LiveOutputCaptureTarget> {
        self.targets.get(&output).cloned()
    }
    fn allowed(&self, output: OutputId, names: &[String]) -> bool {
        names.contains(&format!("out{}", output.raw()))
    }
    fn queue(&mut self, _: OutputId) -> Result<Option<u64>, Status> {
        self.queued += 1;
        Ok(Some(self.queued))
    }
    fn retains(&self, _: OutputId, _: u64) -> bool {
        self.retained
    }
    fn presented(&mut self, output: OutputId, frame: u64) -> Option<PresentedCapture> {
        if !self.ready {
            return None;
        }
        let target = self.targets.get(&output)?;
        Some(PresentedCapture {
            validity: Default::default(),
            owner: target.owner + u64::from(self.foreign),
            output,
            head: target.primary,
            target_generation: target.heads[0].target_generation,
            frame,
            width: 2,
            height: 2,
            pixels: vec![17; 16].into(),
        })
    }
}
struct Fixture {
    owner: SessionCaptures,
    backend: Backend,
}
impl Fixture {
    fn new() -> Self {
        let registry = Arc::new(Mutex::new(NamespaceRegistry::new(31).unwrap()));
        let source_namespace = registry
            .lock()
            .unwrap()
            .create_namespace(
                NamespaceProfile::Confined,
                NamespaceCapabilities::NONE.with_publish(NamespacePortalCapability::ScreenCapture),
            )
            .id;
        let fence = PortalExecutionFence::default();
        let mut recipients = Vec::new();
        let mut targets = BTreeMap::new();
        for id in [7, 8] {
            let namespace = registry
                .lock()
                .unwrap()
                .create_namespace(
                    NamespaceProfile::Confined,
                    NamespaceCapabilities::NONE
                        .with_request(NamespacePortalCapability::ScreenCapture),
                )
                .id;
            let queue = Arc::new(CaptureRequestQueue::new(fence.clone()));
            let objects = Arc::new(
                PortalObjectRecipient::new(namespace).with_capture_requests(queue.clone()),
            );
            recipients.push(Recipient {
                allowed: vec![format!("out{id}")],
                queue,
                objects,
                binding: None,
                pending: None,
            });
            let output = OutputId::from_raw(id);
            let primary = sophia_engine::RenderHeadId::from_raw(id);
            targets.insert(
                output,
                LiveOutputCaptureTarget {
                    owner: 51,
                    output,
                    primary,
                    heads: vec![sophia_engine::HeadRenderTarget {
                        head: primary,
                        output,
                        target_generation: 3,
                        native_size: Size {
                            width: 2,
                            height: 2,
                        },
                        scale: 1,
                        refresh_millihz: 60_000,
                        transform: OutputTransform::Normal,
                        mapping: OutputHeadMapping::Fit,
                    }],
                },
            );
        }
        Self {
            owner: SessionCaptures {
                recipients,
                registry,
                source_namespace,
                fence,
                budget: CaptureRetentionBudget::default(),
                generations: BTreeMap::new(),
                next_generation: 1,
            },
            backend: Backend {
                targets,
                ready: false,
                retained: true,
                queued: 0,
                foreign: false,
            },
        }
    }
    fn request(&self, recipient: usize, serial: u64, output: u64) {
        self.owner.recipients[recipient]
            .queue
            .submit(OutputCaptureRequest {
                serial,
                output: OutputId::from_raw(output),
            })
            .unwrap();
    }
    fn turn(&mut self) {
        self.owner.service(&mut self.backend);
    }
    fn ready(&mut self, recipient: usize) -> u64 {
        self.backend.ready = true;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.turn();
            let phase = self.owner.recipients[recipient].queue.status().state;
            if phase == Status::Ready {
                return self.owner.recipients[recipient].objects.current().unwrap();
            }
            assert!(
                matches!(phase, Status::Queued | Status::Capturing),
                "unexpected {phase:?}"
            );
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn session_waits_for_presentation_and_replacement_stales_only_its_output() {
    let mut f = Fixture::new();
    f.request(0, 1, 7);
    f.turn();
    assert!(f.owner.recipients[0].objects.current().is_err());
    let first = f.ready(0);
    let old =
        OutputCaptureFrameInfo::decode(&f.owner.recipients[0].objects.read(first, 0, 64).unwrap())
            .unwrap();
    assert_eq!(
        f.owner.recipients[0]
            .objects
            .read_frame(first, 0, 16)
            .unwrap(),
        vec![17; 16]
    );
    f.request(1, 1, 8);
    let other = f.ready(1);
    f.backend
        .targets
        .get_mut(&OutputId::from_raw(7))
        .unwrap()
        .heads[0]
        .target_generation += 1;
    f.turn();
    assert_eq!(
        f.owner.recipients[0].objects.read(first, 0, 64),
        Err(sophia_9p::Errno::ESTALE)
    );
    assert!(
        f.owner.recipients[1]
            .objects
            .read_frame(other, 0, 16)
            .is_ok()
    );
    f.request(0, 2, 7);
    let fresh = f.ready(0);
    let new =
        OutputCaptureFrameInfo::decode(&f.owner.recipients[0].objects.read(fresh, 0, 64).unwrap())
            .unwrap();
    assert!(new.output_generation > old.output_generation);
    assert!(new.presentation_generation > old.presentation_generation);
    f.backend.targets.remove(&OutputId::from_raw(7));
    f.turn();
    assert_eq!(
        f.owner.recipients[0].objects.read_frame(fresh, 0, 16),
        Err(sophia_9p::Errno::ESTALE)
    );
}

#[test]
fn session_refuses_foreign_output_pixels_and_pressure_without_disturbing_existing_grants() {
    let mut f = Fixture::new();
    f.request(0, 1, 8);
    f.turn();
    assert_eq!(f.owner.recipients[0].queue.status().state, Status::Denied);
    assert_eq!(f.backend.queued, 0);
    f.request(0, 2, 7);
    f.turn();
    f.backend.ready = true;
    f.backend.foreign = true;
    f.turn();
    assert_eq!(
        f.owner.recipients[0].queue.status().state,
        Status::Unavailable
    );
    assert_eq!(f.owner.budget.usage().unwrap().bytes, 0);
    f.backend.foreign = false;
    f.owner.budget = CaptureRetentionBudget::new(16, 1).unwrap();
    f.request(0, 3, 7);
    let current = f.ready(0);
    f.request(1, 1, 8);
    f.turn();
    assert_eq!(f.owner.recipients[1].queue.status().state, Status::Pressure);
    assert!(
        f.owner.recipients[0]
            .objects
            .read_frame(current, 0, 16)
            .is_ok()
    );
}

#[test]
fn session_lock_cancels_waiting_native_work_without_unlock_revival() {
    let mut f = Fixture::new();
    f.request(0, 1, 7);
    f.turn();
    f.owner.fence.block();
    f.owner.fence.unblock();
    f.backend.ready = true;
    f.turn();
    assert_eq!(
        f.owner.recipients[0].queue.status().state,
        Status::Cancelled
    );
    assert!(f.owner.recipients[0].objects.current().is_err());
    assert_eq!(f.owner.budget.usage().unwrap().bytes, 0);
    f.request(0, 2, 7);
    let current = f.ready(0);
    f.owner.fence.block();
    assert_eq!(
        f.owner.recipients[0].objects.read_frame(current, 0, 16),
        Err(sophia_9p::Errno::ESTALE)
    );
    f.turn();
    assert_eq!(f.owner.budget.usage().unwrap().bytes, 0);
}

#[test]
fn session_retries_a_superseded_frame_within_the_original_request() {
    let mut f = Fixture::new();
    f.request(0, 1, 7);
    f.turn();
    f.backend.retained = false;
    f.turn();
    f.turn();
    assert_eq!(f.backend.queued, 2);
    f.backend.retained = true;
    let current = f.ready(0);
    let info = OutputCaptureFrameInfo::decode(
        &f.owner.recipients[0].objects.read(current, 0, 64).unwrap(),
    )
    .unwrap();
    assert_eq!(info.presentation_generation, 2);
}

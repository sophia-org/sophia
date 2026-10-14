//! Session's output-only capture admission and presentation join.
use super::*;
use sophia_backend_live::LiveOutputCaptureTarget;
use sophia_portal::{
    capture_frame::{CaptureReservation, CaptureRetentionBudget},
    capture_requests::{CaptureRequestQueue, PendingCaptureRequest},
    coordinator::{PortalPolicy, PortalTransferCoordinator, PortalTransferFailure},
    execution_fence::PortalExecutionFence,
    handlers::OutputCapturePayloadHandler,
    output_capture_source::OutputCaptureSource,
    recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{
    output_capture::*, output_capture_request::OutputCaptureState as Status,
    portal_files::PortalFileLimits, *,
};
use std::sync::mpsc;

mod backend;
mod source;
use backend::{CaptureBackend, NativeCaptureBackend};
#[path = "../../../tests/support/session_output_capture.rs"]
mod tests;

struct Policy {
    registry: Arc<Mutex<NamespaceRegistry>>,
    source: NamespaceId,
    target: NamespaceId,
}
impl PortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        self.registry.lock().ok()?.namespace(id)
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        let transfer = &request.transfer;
        transfer.source_namespace == self.source
            && transfer.target_namespace == self.target
            && transfer.kind == PortalTransferKind::ScreenCapture
            && transfer.mime_type.as_deref() == Some(OUTPUT_CAPTURE_MIME)
            && transfer.byte_size == OUTPUT_CAPTURE_RECORD_LEN as u64
    }
}
struct Binding {
    target: LiveOutputCaptureTarget,
    source: Arc<source::Source>,
    _coordinator: PortalTransferCoordinator,
}
struct Pending {
    work: PendingCaptureRequest,
    generation: u64,
    target: LiveOutputCaptureTarget,
    reservation: Option<CaptureReservation>,
    frame: Option<u64>,
    completion: Option<mpsc::Receiver<Result<(), PortalTransferFailure>>>,
}
struct Recipient {
    allowed: Vec<String>,
    queue: Arc<CaptureRequestQueue>,
    objects: Arc<PortalObjectRecipient>,
    binding: Option<Binding>,
    pending: Option<Pending>,
}
pub(super) struct SessionCaptures {
    recipients: Vec<Recipient>,
    registry: Arc<Mutex<NamespaceRegistry>>,
    source_namespace: NamespaceId,
    fence: PortalExecutionFence,
    budget: CaptureRetentionBudget,
    generations: BTreeMap<OutputId, (LiveOutputCaptureTarget, u64)>,
    next_generation: u64,
}

impl LiveApplicationLaunches {
    pub(in crate::live_session) fn revoke_captures(&mut self) {
        if let Some(captures) = self.captures.as_mut() {
            captures.generations.clear();
            for recipient in &mut captures.recipients {
                recipient.finish(Status::Cancelled);
            }
        }
    }
    pub(in crate::live_session) fn prepare_captures(
        &mut self,
        registry: Arc<Mutex<NamespaceRegistry>>,
        fence: PortalExecutionFence,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(prepared) = self.prepared.as_mut() else {
            return Ok(());
        };
        let mut recipients = Vec::new();
        for (name, target) in &mut prepared.targets {
            let recipe = &prepared.policy.recipes[name];
            if recipe.profile != NamespaceRecipeProfile::Observer
                || recipe.capture_outputs.is_empty()
            {
                continue;
            }
            let queue = Arc::new(CaptureRequestQueue::new(fence.clone()));
            let objects = Arc::new(
                PortalObjectRecipient::new(target.context.id).with_capture_requests(queue.clone()),
            );
            target.portal.1 = objects.clone();
            recipients.push(Recipient {
                allowed: recipe.capture_outputs.clone(),
                queue,
                objects,
                binding: None,
                pending: None,
            });
        }
        if recipients.is_empty() {
            return Ok(());
        }
        let source_namespace = registry
            .lock()
            .map_err(|_| "capture namespace registry poisoned")?
            .create_namespace(
                NamespaceProfile::Confined,
                NamespaceCapabilities::NONE.with_publish(NamespacePortalCapability::ScreenCapture),
            )
            .id;
        self.captures = Some(SessionCaptures {
            recipients,
            registry,
            source_namespace,
            fence,
            budget: CaptureRetentionBudget::default(),
            generations: BTreeMap::new(),
            next_generation: 1,
        });
        Ok(())
    }

    pub(in crate::live_session) fn service_captures(
        &mut self,
        native: Option<&mut LiveProductionNativeScanout>,
        runtime: Option<&mut LiveProductionVisualRuntime>,
        scene: &LiveProductionCpuScene,
    ) {
        let Some(captures) = self.captures.as_mut() else {
            return;
        };
        captures.service(&mut NativeCaptureBackend {
            native,
            runtime,
            scene,
        });
    }
}

impl SessionCaptures {
    fn service(&mut self, backend: &mut impl CaptureBackend) {
        self.generations
            .retain(|output, (target, _)| backend.target(*output).as_ref() == Some(target));
        for recipient in &mut self.recipients {
            let valid_binding = recipient.binding.as_ref().is_none_or(|binding| {
                self.fence.live()
                    && (recipient.pending.is_some() || recipient.objects.current().is_ok())
                    && backend.target(binding.target.output).as_ref() == Some(&binding.target)
            });
            if !valid_binding {
                recipient.revoke();
            }
            if recipient.pending.as_ref().is_some_and(|pending| {
                !recipient.queue.current(pending.work)
                    || backend.target(pending.target.output).as_ref() != Some(&pending.target)
            }) {
                recipient.finish(Status::Cancelled);
            }
            if recipient.pending.is_none()
                && let Some(work) = recipient.queue.take()
            {
                let output = work.request().output;
                let Some(target) = backend.target(output) else {
                    recipient.queue.finish(work, Status::Unavailable);
                    continue;
                };
                if !backend.allowed(output, &recipient.allowed) {
                    recipient.queue.finish(work, Status::Denied);
                    continue;
                }
                let generation = if let Some((_, generation)) = self.generations.get(&output) {
                    *generation
                } else {
                    let Some(next) = self.next_generation.checked_add(1) else {
                        recipient.queue.finish(work, Status::Unavailable);
                        continue;
                    };
                    let generation = self.next_generation;
                    self.next_generation = next;
                    self.generations
                        .insert(output, (target.clone(), generation));
                    generation
                };
                let Some(primary) = target.heads.iter().find(|head| head.head == target.primary)
                else {
                    recipient.queue.finish(work, Status::Unavailable);
                    continue;
                };
                let Some(reservation) = self.budget.reserve_storage(
                    u32::try_from(primary.native_size.width).unwrap_or(0),
                    u32::try_from(primary.native_size.height).unwrap_or(0),
                ) else {
                    recipient.queue.finish(work, Status::Pressure);
                    continue;
                };
                recipient.revoke();
                recipient.pending = Some(Pending {
                    work,
                    generation,
                    target,
                    reservation: Some(reservation),
                    frame: None,
                    completion: None,
                });
            }
            if let Err(status) =
                recipient.advance(backend, &self.registry, self.source_namespace, &self.fence)
            {
                recipient.finish(status);
            }
        }
    }
}

impl Recipient {
    fn revoke(&mut self) {
        if let Some(binding) = self.binding.take() {
            binding.source.withdraw();
        }
        self.objects.revoke();
    }
    fn finish(&mut self, status: Status) {
        if let Some(pending) = self.pending.take() {
            self.queue.finish(pending.work, status);
        }
        if status != Status::Ready {
            self.revoke();
        }
    }
    fn advance(
        &mut self,
        backend: &mut impl CaptureBackend,
        registry: &Arc<Mutex<NamespaceRegistry>>,
        source_namespace: NamespaceId,
        fence: &PortalExecutionFence,
    ) -> Result<(), Status> {
        let Some(pending) = self.pending.as_mut() else {
            return Ok(());
        };
        if !self.queue.current(pending.work) {
            return Err(Status::Cancelled);
        }
        if let Some(completion) = &pending.completion {
            return match completion.try_recv() {
                Ok(Ok(())) => {
                    self.finish(Status::Ready);
                    Ok(())
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => Err(Status::Cancelled),
                Err(mpsc::TryRecvError::Empty) => Ok(()),
            };
        }
        let output = pending.target.output;
        if pending.frame.is_none() {
            pending.frame = backend.queue(output)?;
            return Ok(());
        }
        let Some(capture) = backend.presented(output, pending.frame.unwrap()) else {
            if !backend.retains(output, pending.frame.unwrap()) {
                pending.frame = None;
            }
            return Ok(());
        };
        if capture.frame != pending.frame.unwrap()
            || capture.owner != pending.target.owner
            || capture.output != output
            || capture.head != pending.target.primary
            || !pending.target.heads.iter().any(|head| {
                head.head == capture.head && head.target_generation == capture.target_generation
            })
        {
            return Err(Status::Unavailable);
        }
        let info = OutputCaptureFrameInfo {
            output,
            output_generation: pending.generation,
            presentation_generation: capture.frame,
            width: capture.width,
            height: capture.height,
        };
        let image = pending
            .reservation
            .take()
            .ok_or(Status::Pressure)?
            .complete_with_info(info, capture.pixels)
            .ok_or(Status::Unavailable)?;
        let source = Arc::new(source::Source {
            image: OutputCaptureSource::new(output, pending.generation)
                .ok_or(Status::Unavailable)?,
            validity: capture.validity,
        });
        if !source.image.publish(Arc::new(image)) {
            return Err(Status::Unavailable);
        }
        let coordinator = PortalTransferCoordinator::start(
            Arc::new(Policy {
                registry: registry.clone(),
                source: source_namespace,
                target: self.objects.namespace(),
            }),
            source.clone(),
            self.objects.clone(),
            Arc::new(OutputCapturePayloadHandler),
            fence.clone(),
            PortalFileLimits::defaults(1),
        )
        .map_err(|_| Status::Unavailable)?;
        pending.completion = Some(coordinator.enqueue_at_revision(
            PortalTransfer {
                transfer: PortalTransferId::from_raw(pending.work.request().serial),
                source_namespace,
                target_namespace: self.objects.namespace(),
                kind: PortalTransferKind::ScreenCapture,
                mime_type: Some(OUTPUT_CAPTURE_MIME.into()),
                byte_size: OUTPUT_CAPTURE_RECORD_LEN as u64,
                decision: PortalDecision::Pending,
                generation: pending.generation,
            },
            pending.work.execution_revision(),
        ));
        self.binding = Some(Binding {
            target: pending.target.clone(),
            source,
            _coordinator: coordinator,
        });
        Ok(())
    }
}

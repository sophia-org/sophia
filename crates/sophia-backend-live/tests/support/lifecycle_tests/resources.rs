use super::*;

pub(crate) fn upload(
    store: &mut sophia_runtime::ContentResourceStore,
    grant: ContentGrant,
    resource: ContentResourceId,
) -> sophia_runtime::ContentResourceLease {
    store
        .begin_isb(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 4,
                height_px: 2,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 32,
            },
            0,
        )
        .unwrap();
    let usage = store.usage();
    assert_eq!((usage.staging, usage.reserved_resident), (32, 32));
    assert!(usage.staging + usage.resident + usage.retiring <= 128);
    assert!(usage.backing <= 128);
    store
        .complete_isb(
            grant,
            resource,
            &mut Some(Ok(crate::test_content_isb::validated(&[0xff; 32]))),
            0,
        )
        .unwrap();
    while store.take_event().is_some() {}
    store.lease(grant, resource).unwrap()
}

pub(crate) fn shell_frame(
    output: HeadlessOutput,
    candidate: u64,
    lease: sophia_runtime::ContentResourceLease,
) -> LiveShellContentFrame {
    let grant = lease.description().grant;
    LiveShellContentFrame {
        output: output.id,
        content_output: ContentOutputId {
            id: output.id.raw(),
            generation: 1,
        },
        grant,
        candidate_generation: candidate,
        interaction_generation: candidate,
        popouts: Vec::new(),
        images: vec![CompositorContentImage {
            node: CompositorNodeId::ShellContent {
                grant,
                output: output.id,
                candidate,
                surface: 0,
                placement: 0,
            },
            generation: lease.description().resource.generation,
            output_size_px: output.size,
            geometry_px: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            size_px: Size {
                width: 4,
                height: 2,
            },
            stride: 16,
            format: u32::from_le_bytes(*b"AR24"),
            resource: lease.into(),
        }],
        targets: vec![PresentedContentTarget {
            continuity: sophia_engine::ContentTargetContinuity::mint(),
            scale_generation: 1,
            grant,
            output: ContentOutputId {
                id: output.id.raw(),
                generation: 1,
            },
            candidate_generation: candidate,
            presentation_epoch: 0,
            interaction_generation: candidate,
            allocation: ContentAllocationId {
                id: output.id.raw(),
                generation: 1,
            },
            allocation_logical: ContentLogicalRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            allocation_pixel: ContentPixelRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            target_id: 1,
            target_generation: candidate,
            action_id: candidate,
            bounds_px: ContentPixelRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
        }],
        allocations: vec![],
    }
}

pub(super) fn retire(
    store: &mut sophia_runtime::ContentResourceStore,
    grant: ContentGrant,
    resource: ContentResourceId,
) {
    store
        .retire(
            TransactionId::from_raw(4),
            &ContentResourceRetire { grant, resource },
        )
        .unwrap();
}

pub(super) fn released(store: &mut sophia_runtime::ContentResourceStore) -> Vec<ContentResourceId> {
    store.collect();
    let mut result = vec![];
    while let Some(event) = store.take_event() {
        match event.record {
            ShellContentRecord::ResourceReleased(value) => result.push(value.resource),
            other => panic!("unexpected retirement event: {other:?}"),
        }
    }
    for resource in &result {
        store.confirm_isb_release(*resource).unwrap();
    }
    result
}

/// What an accept wrote into `runtime` for `layer` on `output`, for controls
/// outside this module: the installed frame, and the grant whose retirement
/// claim is bound to native `frame`.
#[cfg(feature = "experimental-combined-frame-job")]
pub(crate) fn installed_shell(
    runtime: &LiveProductionVisualRuntime,
    output: OutputId,
    layer: LiveShellContentLayer,
    frame: crate::LiveProductionNativeFrameId,
) -> (Option<LiveShellContentFrame>, Option<ContentGrant>) {
    let key = (output, layer);
    (
        runtime
            .shell_content
            .get(&key)
            .map(|owned| owned.frame.clone()),
        runtime
            .queued_shell_retirements
            .get(&(output, frame))
            .and_then(|bound| bound.get(&key))
            .copied(),
    )
}

/// A scoped `tracing` subscriber for the binding-record controls (gate 4
/// binding repair): every `sophia_shell_native_binding` message, in order,
/// each with the state of `marker` at the moment it arrived. The marker is
/// the recording native's commit flag in the order control and absent
/// elsewhere. Thread-local through `tracing::dispatcher::with_default`;
/// never installed globally.
#[cfg(feature = "experimental-combined-frame-job")]
pub(crate) struct BindingEvents {
    marker: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    seen: std::sync::Mutex<Vec<(String, Option<bool>)>>,
}

#[cfg(feature = "experimental-combined-frame-job")]
impl BindingEvents {
    pub(crate) fn new(marker: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>) -> Self {
        // While exactly one dispatcher is registered, tracing-core computes a
        // new callsite's interest from the registering thread's own default.
        // A parallel test without a subscriber that reaches the emitter first
        // would cache "never" and this subscriber would lose the record. A
        // second registered dispatcher makes tracing consult every one.
        static SECOND: std::sync::OnceLock<tracing::Dispatch> = std::sync::OnceLock::new();
        SECOND.get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::new()));
        Self {
            marker,
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn seen(&self) -> Vec<(String, Option<bool>)> {
        self.seen.lock().unwrap().clone()
    }
}

#[cfg(feature = "experimental-combined-frame-job")]
impl tracing::Subscriber for BindingEvents {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target() == "sophia_scanout_evidence"
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        if message.0.starts_with("sophia_shell_native_binding ") {
            let marker = self
                .marker
                .as_ref()
                .map(|marker| marker.load(std::sync::atomic::Ordering::SeqCst));
            self.seen.lock().unwrap().push((message.0, marker));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

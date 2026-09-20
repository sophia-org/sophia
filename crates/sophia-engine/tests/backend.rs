mod support;
use support::*;

#[test]
fn headless_engine_exposes_deterministic_output() {
    let engine = HeadlessEngine::default();
    let output = engine.output();

    assert_eq!(output.id, OutputId::from_raw(1));
    assert_eq!(
        output.size,
        sophia_protocol::Size {
            width: 1280,
            height: 720,
        }
    );
    assert_eq!(output.scale, 1);
}

#[test]
fn engine_head_registry_tracks_heads_and_logical_views() {
    let output = OutputId::from_raw(7);
    let first = HeadRenderTarget {
        head: RenderHeadId::from_raw(1),
        output,
        target_generation: 1,
        native_size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 2,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    };
    let sibling = HeadRenderTarget {
        head: RenderHeadId::from_raw(2),
        refresh_millihz: 59_000,
        ..first
    };
    let mut registry = EngineHeadRegistry::new();

    assert_eq!(registry.admit(first), EngineHeadRegistryUpdate::Inserted);
    assert_eq!(registry.admit(sibling), EngineHeadRegistryUpdate::Inserted);

    assert_eq!(registry.output_count(), 1);
    assert_eq!(registry.head_count(), 2);
    assert_eq!(registry.head(RenderHeadId::from_raw(2)), Some(&sibling));
    assert_eq!(
        registry.logical_output(output),
        Some(HeadlessOutput {
            id: output,
            size: first.native_size,
            scale: 2,
        })
    );
    // The first admitted member owns logical pacing; the sibling consumes the
    // resulting generations independently.
    assert_eq!(registry.primary_head(output), Some(first.head));
    assert_eq!(registry.logical_refresh_millihz(output), 60_000);
    assert_eq!(
        registry.set_primary_head(output, sibling.head),
        EngineLogicalOutputUpdate::Updated
    );
    assert_eq!(registry.logical_refresh_millihz(output), 59_000);
    assert_eq!(
        registry.remove_head(RenderHeadId::from_raw(2)),
        Some(sibling)
    );
    assert_eq!(registry.head_count(), 1);
    assert_eq!(registry.remove_output(output), 1);
    assert!(registry.primary_engine_output().is_none());
}

#[test]
fn engine_head_registry_rejects_unbounded_growth() {
    let mut registry = EngineHeadRegistry::new();
    let mut next_head = 1u64;
    for index in 0..sophia_engine::MAX_DRM_KMS_OUTPUTS {
        let raw = u64::try_from(index + 1).unwrap();
        assert_eq!(
            registry.admit(head_target(next_head, raw)),
            EngineHeadRegistryUpdate::Inserted
        );
        next_head += 1;
    }
    assert_eq!(
        registry.admit(head_target(next_head, 99)),
        EngineHeadRegistryUpdate::OutputCapacityExceeded
    );

    for _ in 1..sophia_engine::MAX_HEADS_PER_OUTPUT {
        next_head += 1;
        assert_eq!(
            registry.admit(head_target(next_head, 1)),
            EngineHeadRegistryUpdate::Inserted
        );
    }
    next_head += 1;
    assert_eq!(
        registry.admit(head_target(next_head, 1)),
        EngineHeadRegistryUpdate::HeadCapacityExceeded
    );
}

#[test]
fn engine_head_registry_rejects_stale_target_generations() {
    let mut registry = EngineHeadRegistry::new();
    let target = head_target(1, 1);
    assert_eq!(registry.admit(target), EngineHeadRegistryUpdate::Inserted);

    // A shape change must advance the target generation; silently mutating a
    // target under an unchanged generation would relabel stale prepared work.
    let reshaped_same_generation = HeadRenderTarget {
        native_size: Size {
            width: 2560,
            height: 1440,
        },
        ..target
    };
    assert_eq!(
        registry.admit(reshaped_same_generation),
        EngineHeadRegistryUpdate::StaleTargetGeneration
    );
    let reshaped = HeadRenderTarget {
        target_generation: 2,
        ..reshaped_same_generation
    };
    assert_eq!(registry.admit(reshaped), EngineHeadRegistryUpdate::Replaced);
    assert_eq!(
        registry.admit(target),
        EngineHeadRegistryUpdate::StaleTargetGeneration
    );
    assert_eq!(registry.head(RenderHeadId::from_raw(1)), Some(&reshaped));
}

#[test]
fn engine_head_registry_denies_head_reassignment_across_outputs() {
    let mut registry = EngineHeadRegistry::new();
    assert_eq!(
        registry.admit(head_target(1, 1)),
        EngineHeadRegistryUpdate::Inserted
    );
    assert_eq!(
        registry.admit(head_target(1, 2)),
        EngineHeadRegistryUpdate::HeadOwnedByOtherOutput
    );
    assert_eq!(
        registry.admit(HeadRenderTarget {
            head: RenderHeadId::INVALID,
            ..head_target(1, 1)
        }),
        EngineHeadRegistryUpdate::InvalidHead
    );
}

#[test]
fn engine_head_registry_keeps_logical_view_independent_of_unequal_heads() {
    let output = OutputId::from_raw(1);
    let mut registry = EngineHeadRegistry::new();
    assert_eq!(
        registry.admit(head_target(1, 1)),
        EngineHeadRegistryUpdate::Inserted
    );
    assert_eq!(
        registry.admit(HeadRenderTarget {
            head: RenderHeadId::from_raw(2),
            native_size: Size {
                width: 2560,
                height: 1440,
            },
            ..head_target(2, 1)
        }),
        EngineHeadRegistryUpdate::Inserted
    );

    assert_eq!(
        registry.set_logical_output(HeadlessOutput {
            id: output,
            size: Size {
                width: 2048,
                height: 1152,
            },
            scale: 1,
        }),
        EngineLogicalOutputUpdate::Updated
    );
    assert_eq!(
        registry.logical_output(output),
        Some(HeadlessOutput {
            id: output,
            size: Size {
                width: 2048,
                height: 1152,
            },
            scale: 1,
        })
    );
}

#[test]
fn head_target_can_seed_engine_output() {
    let target = HeadRenderTarget {
        head: RenderHeadId::from_raw(1),
        output: OutputId::from_raw(8),
        target_generation: 1,
        native_size: Size {
            width: 2560,
            height: 1440,
        },
        scale: 1,
        refresh_millihz: 144_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    };
    let mut registry = EngineHeadRegistry::new();
    assert!(registry.admit(target).is_admitted());
    let engine = HeadlessEngine::new(registry.logical_output(target.output).unwrap());
    let frame = engine
        .plan_frame(
            FramePlanRequest {
                output: OutputId::from_raw(8),
                frame_serial: 1,
            },
            Vec::new(),
        )
        .unwrap();

    assert_eq!(frame.output_size, target.native_size);
    assert_eq!(frame.output_scale, target.scale);
}

fn head_target(head: u64, output: u64) -> HeadRenderTarget {
    HeadRenderTarget {
        head: RenderHeadId::from_raw(head),
        output: OutputId::from_raw(output),
        target_generation: 1,
        native_size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    }
}

#[test]
fn libinput_event_source_accepts_registered_device_events_in_order() {
    let mut source = LibinputEventSource::new();
    let device = LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    };
    source.register_device(device);

    assert_eq!(source.device(DeviceId::from_raw(2)), Some(&device));
    assert_eq!(source.devices().count(), 1);
    assert_eq!(
        source.push_event(motion_event(1, 10.0, 20.0)),
        LibinputEventIngest::Accepted
    );
    assert_eq!(
        source.push_event(motion_event(2, 11.0, 21.0)),
        LibinputEventIngest::Accepted
    );

    let events = source.drain_events();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].serial, 1);
    assert_eq!(events[1].serial, 2);
    assert_eq!(source.pending_len(), 0);
    assert_eq!(source.remove_device(DeviceId::from_raw(2)), Some(device));
}

#[test]
fn libinput_event_source_registers_on_an_announcement_and_forgets_on_a_departure() {
    let mut source = LibinputEventSource::new();
    let mut announced = motion_event(1, 0.0, 0.0);
    announced.device = DeviceId::from_raw(300);
    announced.kind = InputEventKind::DeviceAdded {
        keyboard: true,
        pointer: false,
        touch: false,
        virtual_bus: false,
    };
    let mut departed = announced.clone();
    departed.serial = 3;
    departed.kind = InputEventKind::DeviceRemoved;
    let mut typed = motion_event(2, 0.0, 0.0);
    typed.device = DeviceId::from_raw(300);
    typed.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };

    assert_eq!(
        source.push_event(departed.clone()),
        LibinputEventIngest::UnknownDevice
    );
    assert_eq!(source.push_event(announced), LibinputEventIngest::Accepted);
    assert_eq!(
        source.device(DeviceId::from_raw(300)),
        Some(&LibinputDeviceDescriptor {
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(300),
            kind: LibinputDeviceKind::Keyboard,
        })
    );
    assert_eq!(
        source.push_event(typed.clone()),
        LibinputEventIngest::Accepted
    );
    assert_eq!(source.push_event(departed), LibinputEventIngest::Accepted);
    assert_eq!(source.device(DeviceId::from_raw(300)), None);
    assert_eq!(source.push_event(typed), LibinputEventIngest::UnknownDevice);
    assert_eq!(source.pending_len(), 3);
}

#[test]
fn libinput_event_source_rejects_unknown_or_wrong_seat_events() {
    let mut source = LibinputEventSource::new();
    source.register_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(9),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Keyboard,
    });

    assert_eq!(
        source.push_event(motion_event(1, 0.0, 0.0)),
        LibinputEventIngest::SeatMismatch
    );

    let mut unknown_device_event = motion_event(2, 0.0, 0.0);
    unknown_device_event.device = DeviceId::from_raw(99);
    assert_eq!(
        source.push_event(unknown_device_event),
        LibinputEventIngest::UnknownDevice
    );
    assert_eq!(source.pending_len(), 0);
}

#[test]
fn libinput_physical_input_adapter_polls_ready_events_without_blocking() {
    let mut source = LibinputEventSource::new();
    source.register_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let poller = QueuedInputPoller::new(vec![
        motion_event(1, 10.0, 20.0),
        motion_event(2, 11.0, 21.0),
    ]);
    let mut adapter = LibinputPhysicalInputAdapter::new(poller, source);

    let report = adapter.poll_once().unwrap();

    assert_eq!(report.polled, 2);
    assert_eq!(report.accepted, 2);
    assert!(report.rejected.is_empty());
    assert_eq!(adapter.source().pending_len(), 2);
    let events = adapter.source_mut().drain_events();
    assert_eq!(events[0].serial, 1);
    assert_eq!(events[1].serial, 2);

    let empty_report = adapter.poll_once().unwrap();
    assert_eq!(empty_report.polled, 0);
    assert_eq!(empty_report.accepted, 0);
    assert!(empty_report.rejected.is_empty());
}

#[test]
fn libinput_physical_input_adapter_reports_rejected_events() {
    let mut source = LibinputEventSource::new();
    source.register_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(9),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let mut unknown_device_event = motion_event(2, 0.0, 0.0);
    unknown_device_event.device = DeviceId::from_raw(99);
    let poller = QueuedInputPoller::new(vec![motion_event(1, 0.0, 0.0), unknown_device_event]);
    let mut adapter = LibinputPhysicalInputAdapter::new(poller, source);

    let report = adapter.poll_once().unwrap();

    assert_eq!(report.polled, 2);
    assert_eq!(report.accepted, 0);
    assert_eq!(
        report.rejected,
        vec![
            LibinputEventIngest::SeatMismatch,
            LibinputEventIngest::UnknownDevice,
        ]
    );
    assert_eq!(adapter.source().pending_len(), 0);
}

#[derive(Clone, Debug)]
struct StaticHeadRegistryBackend {
    heads: Vec<HeadRenderTarget>,
}

impl OutputDiscoveryBackend for StaticHeadRegistryBackend {
    fn discover_outputs(&self) -> IoResult<EngineHeadRegistry> {
        let mut registry = EngineHeadRegistry::new();
        for target in &self.heads {
            assert!(registry.admit(*target).is_admitted());
        }
        Ok(registry)
    }
}

#[test]
fn live_backend_discovery_can_seed_headless_assembly_without_policy_changes() {
    let output_backend = StaticHeadRegistryBackend {
        heads: vec![head_target(1, 1)],
    };
    let input_backend = StaticInputDiscoveryBackend::new(vec![LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    }]);

    let report = discover_live_compositor_backend(&output_backend, &input_backend);

    assert!(report.is_ready());
    assert_eq!(report.status, LiveCompositorBackendDiscoveryStatus::Ready);
    assert_eq!(
        report.selected_output,
        Some(HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        })
    );
    assert_eq!(report.input_source.devices().count(), 1);

    let assembly = report
        .into_headless_assembly(QueuedInputPoller::default(), RendererSelection::CpuFallback)
        .expect("ready backend discovery should create a deterministic assembly");
    assert_eq!(
        assembly.outputs().primary_engine_output(),
        Some(HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        })
    );
}

#[test]
fn live_backend_discovery_fails_closed_when_no_outputs_exist() {
    let output_backend = StaticHeadRegistryBackend { heads: Vec::new() };
    let input_backend = StaticInputDiscoveryBackend::new(vec![LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    }]);

    let report = discover_live_compositor_backend(&output_backend, &input_backend);

    assert_eq!(
        report.status,
        LiveCompositorBackendDiscoveryStatus::NoOutputs
    );
    assert!(!report.is_ready());
    assert_eq!(report.selected_output, None);
    assert_eq!(report.input_source.devices().count(), 0);
    assert!(
        report
            .into_headless_assembly(QueuedInputPoller::default(), RendererSelection::CpuFallback)
            .is_none()
    );
}

#[derive(Clone, Debug)]
struct FailingOutputBackend;

impl OutputDiscoveryBackend for FailingOutputBackend {
    fn discover_outputs(&self) -> IoResult<EngineHeadRegistry> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        ))
    }
}

#[test]
fn live_backend_discovery_reports_output_errors_without_starting_assembly() {
    let input_backend = StaticInputDiscoveryBackend::new(Vec::new());

    let report = discover_live_compositor_backend(&FailingOutputBackend, &input_backend);

    assert_eq!(
        report.status,
        LiveCompositorBackendDiscoveryStatus::OutputDiscoveryFailed {
            message: "denied".to_owned(),
        }
    );
    assert_eq!(report.outputs.outputs().count(), 0);
    assert_eq!(report.selected_output, None);
    assert!(
        report
            .into_headless_assembly(QueuedInputPoller::default(), RendererSelection::CpuFallback)
            .is_none()
    );
}

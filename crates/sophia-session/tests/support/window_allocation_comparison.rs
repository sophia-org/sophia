#![cfg(test)]

use super::*;
use sophia_backend_live::{
    LiveOutputAllocationContext, LivePresentLayoutComparison, LiveProductionRetiredLayoutWitness,
    LiveRenderDeviceNodeIdentity, LiveRendererFrameCorrelation, LiveScanoutLayoutWitness,
    renderer_image_for_present,
};
use sophia_engine::{DirectScanoutVerdict, RenderHeadId};
use sophia_protocol::{
    BufferHandle, BufferSource, CommittedSurfaceState, DRM_FORMAT_ARGB8888 as AR24,
    DRM_FORMAT_XRGB8888 as XR24, OutputId, Region, Size, SurfaceTransactionKey, TransactionId,
};
use sophia_renderer_live::LiveCompositionTrace;
use sophia_x_authority::{
    XDrmDeviceHint, XPresentLayoutComparison, XRenderDeviceIdentity,
    XServerFrontendDmaBufImportFormat, XWindowAllocationContext, XWindowAllocationPreference,
    XWindowAllocationPreferences,
};

struct ComparisonFixture {
    publisher: LiveWindowAllocationPublisher,
    evidence: LivePresentLayoutComparison,
    surface: CommittedSurfaceState,
    topology: u64,
    output: OutputId,
    context: LiveOutputAllocationContext,
    device: LiveRenderDeviceNodeIdentity,
}

type ComparisonChange = (&'static str, fn(&mut ComparisonFixture));

impl ComparisonFixture {
    fn new(format: u32) -> Self {
        let surface = SurfaceId::new(7, 3);
        let output = OutputId::from_raw(1);
        let transaction = TransactionId::from_raw(71);
        let target_buffer = BufferSource::DmaBuf { handle: 91 };
        let device = LiveRenderDeviceNodeIdentity {
            device: 4,
            inode: 77,
            device_number: rustix::fs::makedev(226, 128),
        };
        let context = LiveOutputAllocationContext {
            generation: 9,
            head: RenderHeadId::from_raw(2),
            target_generation: 5,
        };
        let geometry = Rect {
            x: 10,
            y: 20,
            width: 16,
            height: 16,
        };
        Self {
            publisher: LiveWindowAllocationPublisher {
                generation: 17,
                applied: Some(XWindowAllocationPreferences {
                    generation: 17,
                    topology_generation: 6,
                    windows: vec![XWindowAllocationPreference {
                        surface,
                        device: XDrmDeviceHint {
                            major: 226,
                            minor: 128,
                        },
                        identity: Some(XRenderDeviceIdentity {
                            device: device.device,
                            inode: device.inode,
                            device_number: device.device_number,
                        }),
                        context: Some(XWindowAllocationContext {
                            generation: context.generation,
                            output,
                        }),
                        formats: vec![
                            XServerFrontendDmaBufImportFormat {
                                format: XR24,
                                modifiers: vec![0, 11],
                            },
                            XServerFrontendDmaBufImportFormat {
                                format: AR24,
                                modifiers: vec![0, 22],
                            },
                        ],
                    }],
                }),
                ..Default::default()
            },
            evidence: LivePresentLayoutComparison {
                candidate: SurfaceTransactionKey {
                    transaction,
                    surface,
                    target_buffer,
                },
                retired: LiveProductionRetiredLayoutWitness {
                    witness: LiveScanoutLayoutWitness {
                        source_image: renderer_image_for_present(transaction),
                        alternative: LiveRendererFrameCorrelation {
                            native: None,
                            request: None,
                            trace: Some(LiveCompositionTrace {
                                output,
                                head: context.head,
                                scene_generation: 41,
                            }),
                            direct_scanout: Some(DirectScanoutVerdict::CompositionRequired(
                                "refused",
                            )),
                        },
                        format,
                        original_modifier: 99,
                        alternative_modifier: if format == XR24 { 11 } else { 22 },
                    },
                    device,
                    head: context.head,
                    target_generation: context.target_generation,
                    context_generation: context.generation,
                },
            },
            surface: CommittedSurfaceState::with_source(
                surface,
                8,
                geometry,
                target_buffer,
                Size {
                    width: 16,
                    height: 16,
                },
                Region::empty(),
            ),
            topology: 6,
            output,
            context,
            device,
        }
    }

    fn compare(&self) -> Option<(TransactionId, XPresentLayoutComparison)> {
        self.publisher.compare_retired(
            &self.evidence,
            &self.surface,
            self.topology,
            self.output,
            self.context,
            self.device,
        )
    }

    fn preference(&mut self) -> &mut XWindowAllocationPreference {
        &mut self.publisher.applied.as_mut().unwrap().windows[0]
    }
}

#[test]
fn retired_comparisons_use_exact_format_membership_and_current_owner_geometry() {
    for format in [XR24, AR24] {
        let mut fixture = ComparisonFixture::new(format);
        // Placement is compared again at the frontend; this boundary forwards
        // the current owner view, not geometry captured at Present submission.
        fixture.surface.geometry.x += 40;
        let (transaction, comparison) = fixture.compare().expect("matching retired witness");
        assert_eq!(transaction, fixture.evidence.candidate.transaction);
        assert_eq!(comparison.surface, fixture.surface.surface);
        assert_eq!(comparison.buffer, BufferHandle::from_raw(91));
        assert_eq!(comparison.format, format);
        assert_eq!(comparison.original_modifier, 99);
        assert_eq!(
            comparison.alternative_modifier,
            if format == XR24 { 11 } else { 22 }
        );
        assert_eq!(comparison.geometry, fixture.surface.geometry);
        assert_eq!(comparison.preference_generation, 17);
        assert_eq!(comparison.topology_generation, 6);
        assert_eq!(
            comparison.native_context,
            fixture.preference().context.unwrap()
        );
        assert_eq!(
            comparison.device_identity,
            fixture.preference().identity.unwrap()
        );
    }
}

#[test]
fn retired_comparisons_require_current_native_and_acknowledged_contexts() {
    let cases: &[ComparisonChange] = &[
        ("topology", |f| f.topology += 1),
        ("output", |f| f.output = OutputId::from_raw(2)),
        ("head", |f| f.context.head = RenderHeadId::from_raw(3)),
        ("retired head", |f| {
            f.evidence.retired.head = RenderHeadId::from_raw(3)
        }),
        ("target generation", |f| f.context.target_generation += 1),
        ("device incarnation", |f| f.device.inode += 1),
        ("device filesystem", |f| f.device.device += 1),
        ("device number", |f| f.device.device_number += 1),
        ("same-valued native rollback", |f| {
            f.context.generation += 1;
            f.preference().context.as_mut().unwrap().generation += 1;
        }),
        ("unacknowledged context", |f| {
            f.context.generation += 1;
            f.evidence.retired.context_generation += 1;
        }),
        ("acknowledged output", |f| {
            f.preference().context.as_mut().unwrap().output = OutputId::from_raw(2)
        }),
        ("missing acknowledged context", |f| {
            f.preference().context = None
        }),
        ("acknowledged device incarnation", |f| {
            f.preference().identity.as_mut().unwrap().inode += 1
        }),
        ("missing acknowledged device", |f| {
            f.preference().identity = None
        }),
        ("missing trace", |f| {
            f.evidence.retired.witness.alternative.trace = None
        }),
    ];
    for (name, change) in cases {
        let mut fixture = ComparisonFixture::new(AR24);
        assert!(fixture.compare().is_some());
        change(&mut fixture);
        assert_eq!(fixture.compare(), None, "{name}");
    }
}

#[test]
fn retired_comparisons_refuse_replaced_subjects_and_unhelpful_format_rows() {
    let cases: &[ComparisonChange] = &[
        ("surface generation", |f| {
            f.surface.surface = SurfaceId::new(7, 4)
        }),
        ("replaced backing", |f| {
            f.surface.content = sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::DmaBuf { handle: 92 },
                Size {
                    width: 16,
                    height: 16,
                },
            );
        }),
        ("non-DMA candidate", |f| {
            f.evidence.candidate.target_buffer = BufferSource::CpuBuffer { handle: 91 }
        }),
        ("preference surface generation", |f| {
            f.preference().surface = SurfaceId::new(7, 4)
        }),
        ("missing exact format", |f| {
            f.preference().formats.retain(|row| row.format != AR24)
        }),
        ("alternative only in other format", |f| {
            f.evidence.retired.witness.alternative_modifier = 11
        }),
        ("alternative missing", |f| {
            f.evidence.retired.witness.alternative_modifier = 33
        }),
        ("original still preferred", |f| {
            f.evidence.retired.witness.original_modifier = 22
        }),
    ];
    for (name, change) in cases {
        let mut fixture = ComparisonFixture::new(AR24);
        assert!(fixture.compare().is_some());
        change(&mut fixture);
        assert_eq!(fixture.compare(), None, "{name}");
    }
}

#[test]
fn unacknowledged_preferences_never_replace_the_applied_comparison_generation() {
    let mut fixture = ComparisonFixture::new(AR24);
    let mut pending = fixture.publisher.applied.as_ref().unwrap().clone();
    pending.generation = 18;
    pending.windows[0].context.as_mut().unwrap().generation += 1;
    let (_sender, receiver) = std::sync::mpsc::sync_channel(1);
    fixture.publisher.pending = Some((pending, receiver));
    fixture.publisher.generation = 18;

    // An outstanding publication does not wait or fabricate its acceptance.
    assert_eq!(fixture.compare().unwrap().1.preference_generation, 17);
    fixture.context.generation += 1;
    fixture.evidence.retired.context_generation += 1;
    assert_eq!(fixture.compare(), None);

    let accepted = fixture.publisher.pending.as_ref().unwrap().0.clone();
    fixture.publisher.applied = Some(accepted);
    assert_eq!(fixture.compare().unwrap().1.preference_generation, 18);
    fixture.publisher.applied = None;
    assert_eq!(fixture.compare(), None);
}

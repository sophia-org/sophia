#![cfg(test)]

use super::*;

fn request(framebuffer: u64, other_property: u64) -> LibdrmNativeAtomicCommitRequest {
    let mut request = crate::LibdrmNativeRecordedAtomicRequest::new();
    let plane = drm::control::from_u32::<drm::control::plane::Handle>(13).unwrap();
    for (property, value) in [(104, framebuffer), (105, other_property)] {
        request.add_property(
            plane,
            drm::control::from_u32::<drm::control::property::Handle>(property).unwrap(),
            drm::control::property::Value::UnsignedRange(value),
        );
    }
    request
        .finish(LibdrmNativeAtomicCommitRequestScope::PageFlip, (13, 104))
        .test_only()
}

fn observation(framebuffer: u64, errno: Option<i32>) -> LibdrmNativeAtomicTestReport {
    let request = request(framebuffer, 9);
    LibdrmNativeAtomicTestReport {
        status: if errno.is_some() {
            LibdrmNativeAtomicCommitSubmitStatus::Rejected
        } else {
            LibdrmNativeAtomicCommitSubmitStatus::Submitted
        },
        request: request.evidence(),
        error_kind: errno.map(|errno| std::io::Error::from_raw_os_error(errno).kind()),
        raw_os_error: errno,
        request_scope: request.reduced_scope(),
        commit_flags: request.reduced_flags(),
    }
}

fn report() -> LiveScanoutLayoutProbeReport {
    let alternative = crate::LiveRendererFrameCorrelation {
        native: None,
        request: None,
        trace: Some(sophia_renderer_live::LiveCompositionTrace {
            output: sophia_protocol::OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(2),
            scene_generation: 3,
        }),
        direct_scanout: Some(sophia_engine::DirectScanoutVerdict::CompositionRequired(
            "refused",
        )),
    };
    let mut original = alternative;
    original.direct_scanout = Some(sophia_engine::DirectScanoutVerdict::Eligible);
    LiveScanoutLayoutProbeReport {
        source_image: sophia_renderer_live::LiveRendererImageId::from_raw(8),
        original,
        alternative,
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        original_modifier: 7,
        alternative_modifier: 0,
        tests: LiveScanoutLayoutComparison {
            status: LibdrmNativeAtomicTestPairStatus::Tested,
            original: Some(LiveScanoutLayoutOriginalTest::Atomic(observation(
                14,
                Some(22),
            ))),
            alternative: Some(observation(15, None)),
        },
    }
}

#[test]
fn layout_witness_requires_the_exact_current_request_and_correlation() {
    let report = report();
    let mut descriptor = LiveRendererScanoutBufferDescriptor::new(
        sophia_protocol::Size {
            width: 4,
            height: 4,
        },
        16,
        report.format,
        15,
    );
    descriptor.modifier = Some(0);
    let current = request(15, 9).evidence().unwrap();
    assert!(
        report
            .witness_for_current(Some(report.alternative), descriptor, current)
            .is_some()
    );
    for changed in [request(16, 9), request(15, 10), request(15, 9).blocking()] {
        assert!(
            report
                .witness_for_current(
                    Some(report.alternative),
                    descriptor,
                    changed.evidence().unwrap()
                )
                .is_none()
        );
    }
    let mut newer = report.alternative;
    newer.trace.as_mut().unwrap().scene_generation += 1;
    for correlation in [None, Some(newer), Some(report.original)] {
        assert!(
            report
                .witness_for_current(correlation, descriptor, current)
                .is_none()
        );
    }
    let mut changed = report;
    changed.tests.alternative.as_mut().unwrap().request = None;
    assert!(
        changed
            .witness_for_current(Some(report.alternative), descriptor, current)
            .is_none()
    );
    let mut changed = report;
    let Some(LiveScanoutLayoutOriginalTest::Atomic(original)) = &mut changed.tests.original else {
        panic!("fixture owns an atomic observation");
    };
    original.request = Some(current);
    assert!(
        changed
            .witness_for_current(Some(report.alternative), descriptor, current)
            .is_none(),
        "same framebuffer is not an alternate layout comparison"
    );
}

#[test]
fn framebuffer_refusal_needs_an_equivalent_fresh_success_and_exact_committing_request() {
    let mut report = report();
    let current = request(15, 9).evidence().unwrap();
    report.tests.original = Some(LiveScanoutLayoutOriginalTest::Framebuffer {
        intended_request: Some(current),
        error_kind: std::io::ErrorKind::InvalidInput,
        raw_os_error: Some(22),
    });
    let mut descriptor = LiveRendererScanoutBufferDescriptor::new(
        sophia_protocol::Size {
            width: 4,
            height: 4,
        },
        16,
        report.format,
        15,
    );
    descriptor.modifier = Some(0);
    assert!(
        report
            .witness_for_current(Some(report.alternative), descriptor, current)
            .is_some()
    );
    for case in 0..11 {
        let mut changed = report;
        match case {
            0 => changed.tests.status = LibdrmNativeAtomicTestPairStatus::RequestMismatch,
            1 => changed.tests.original = None,
            2..=5 => {
                let Some(LiveScanoutLayoutOriginalTest::Framebuffer {
                    intended_request,
                    error_kind,
                    raw_os_error,
                }) = &mut changed.tests.original
                else {
                    unreachable!()
                };
                match case {
                    2 => *intended_request = None,
                    3 => *intended_request = request(15, 10).evidence(),
                    4 => *raw_os_error = Some(12),
                    5 => *error_kind = std::io::ErrorKind::Other,
                    _ => unreachable!(),
                }
            }
            6 => changed.tests.alternative = Some(observation(15, Some(22))),
            7 => changed.tests.alternative = None,
            8 => changed.tests.alternative.as_mut().unwrap().request = request(16, 9).evidence(),
            9 => changed.original_modifier = changed.alternative_modifier,
            10 => {
                changed.tests.alternative.as_mut().unwrap().status =
                    LibdrmNativeAtomicCommitSubmitStatus::WouldBlock
            }
            _ => unreachable!(),
        }
        assert!(
            changed
                .witness_for_current(Some(report.alternative), descriptor, current)
                .is_none(),
            "case {case}"
        );
    }
    for mutated in [request(16, 9), request(15, 10), request(15, 9).blocking()] {
        assert!(
            report
                .witness_for_current(
                    Some(report.alternative),
                    descriptor,
                    mutated.evidence().unwrap()
                )
                .is_none()
        );
    }
}

#[test]
fn layout_comparison_requires_the_same_native_frame_on_both_sources() {
    let mut report = report();
    let owner = crate::NativeFrameOwner::new();
    let output = sophia_protocol::OutputId::from_raw(1);
    let head = sophia_engine::RenderHeadId::from_raw(2);
    let exact = owner.frame(output, head, 3, 4);
    report.original.native = Some(exact);
    report.alternative.native = Some(exact);
    let mut descriptor = LiveRendererScanoutBufferDescriptor::new(
        sophia_protocol::Size {
            width: 4,
            height: 4,
        },
        16,
        report.format,
        15,
    );
    descriptor.modifier = Some(0);
    let current = request(15, 9).evidence().unwrap();
    assert!(
        report
            .witness_for_current(Some(report.alternative), descriptor, current)
            .is_some()
    );
    for native in [
        None,
        Some(crate::NativeFrameOwner::new().frame(output, head, 3, 4)),
        Some(owner.frame(output, head, 3, 5)),
    ] {
        report.original.native = native;
        assert!(
            report
                .witness_for_current(Some(report.alternative), descriptor, current)
                .is_none()
        );
    }
}

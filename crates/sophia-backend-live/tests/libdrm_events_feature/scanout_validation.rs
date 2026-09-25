/// A validating commit asks about the exact framebuffer that would flip and
/// changes nothing. `validate_prepared_native_primary_plane_scanout` returns
/// the prepared scanout either way, because its resources are still owed a
/// submit or a cancel: an answer is not a disposal.
#[test]
fn a_validating_commit_asks_without_flipping() {
    let device = full_primary_plane_scanout_device();
    let selection = select_native_primary_plane_target(&device);
    let mut prepared =
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            &device,
            selection,
            scanout_descriptor(Size {
                width: 1280,
                height: 720,
            }),
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip(),
        );

    let (verdict, prepared_again) = validate_prepared_native_primary_plane_scanout(
        &device,
        prepared
            .prepared
            .take()
            .expect("successful preparation retains an affine owner"),
    );

    assert_eq!(verdict, LibdrmNativeAtomicCommitSubmitStatus::Submitted);
    assert_eq!(device.commits.get(), 1, "the test itself is a commit");
    assert_eq!(
        device.test_only_commits(),
        1,
        "a validating commit must carry TEST_ONLY, or it changed the screen"
    );
    assert_eq!(
        device.resources.destroyed_framebuffers.get(),
        0,
        "asking about a framebuffer must not destroy it"
    );

    // The same request then performs the flip it was asked about.
    let submitted = submit_prepared_native_primary_plane_scanout(&device, prepared_again);
    assert_eq!(
        submitted.status,
        LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(device.commits.get(), 2);
    assert_eq!(
        device.test_only_commits(),
        1,
        "the flip itself is performed, not asked about"
    );
}

/// A driver that refuses the buffer answers rather than fails. The caller
/// composes instead, and the prepared scanout comes back so its resources can
/// be cancelled.
#[test]
fn a_refused_validating_commit_is_an_answer_not_a_fault() {
    let device = full_primary_plane_scanout_device().accepting_commits(0);
    let selection = select_native_primary_plane_target(&device);
    let mut prepared =
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            &device,
            selection,
            scanout_descriptor(Size {
                width: 1280,
                height: 720,
            }),
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip(),
        );

    let (verdict, prepared_again) = validate_prepared_native_primary_plane_scanout(
        &device,
        prepared
            .prepared
            .take()
            .expect("successful preparation retains an affine owner"),
    );

    assert_eq!(verdict, LibdrmNativeAtomicCommitSubmitStatus::Rejected);
    let cancelled = cancel_prepared_native_primary_plane_scanout(&device, prepared_again);
    assert_eq!(
        cancelled.status,
        LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed,
        "a refused frame releases its framebuffer rather than leaking it"
    );
}

/// A validating commit never carries a page-flip event: there is no flip to
/// report, and the kernel refuses the pair outright. The policy clears it so
/// the two facts never have to agree in two places.
#[test]
fn a_validating_policy_carries_no_page_flip_event() {
    let validating = LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip().validating();

    assert!(validating.test_only);
    assert!(!validating.page_flip_event);
    // Scope is unchanged: asking about a page flip is still a page flip's
    // worth of state, not a modeset.
    assert_eq!(
        validating.expected_request_scope(),
        LibdrmNativeAtomicCommitRequestScope::PageFlip
    );
}

/// A rejected combined commit retries with the primary alone.
///
/// The cursor and the frame share one request, so a cursor-side refusal
/// takes the frame with it -- a failure class that cannot exist on the
/// legacy ioctl. `NoFrameLostToCursor` is the model's answer: the retry is
/// prepared beside the combined request, and the driver refusing the first
/// gets the same frame again without its passenger. The result says the
/// cursor was dropped, so the caller leaves the position pending instead of
/// recording a cursor the plane is not showing.
#[test]
fn a_rejected_combined_commit_retries_without_its_cursor() {
    let mut device = full_primary_plane_scanout_device();
    device.reject_commits_before = 1;
    let selection = select_native_primary_plane_target(&device);
    let cursor = sophia_backend_live::LibdrmNativeAtomicCursor {
        plane: drm::control::from_u32(61).unwrap(),
        properties: cursor_plane_property_handles(),
        placement: Some(sophia_backend_live::LibdrmNativeCursorPlacement {
            framebuffer: drm::control::from_u32(9).unwrap(),
            x: 40,
            y: 30,
            width: 64,
            height: 64,
        }),
    };
    let mut prepared =
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            &device,
            selection,
            scanout_descriptor(Size {
                width: 1280,
                height: 720,
            }),
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip().with_cursor(cursor),
        );

    let submitted = submit_prepared_native_primary_plane_scanout(
        &device,
        prepared
            .prepared
            .take()
            .expect("successful preparation retains an affine owner"),
    );
    assert_eq!(
        submitted.status,
        LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip,
        "the frame survives the cursor's refusal"
    );
    assert!(
        submitted.cursor_dropped,
        "and the result says the cursor did not ride"
    );
    assert_eq!(device.commits.get(), 2, "one refusal, one retry");
}

/// The retry is spent only on rejection: an accepted combined commit is one
/// commit, and the cursor is aboard it.
#[test]
fn an_accepted_combined_commit_does_not_retry() {
    let device = full_primary_plane_scanout_device();
    let selection = select_native_primary_plane_target(&device);
    let cursor = sophia_backend_live::LibdrmNativeAtomicCursor {
        plane: drm::control::from_u32(61).unwrap(),
        properties: cursor_plane_property_handles(),
        placement: None,
    };
    let mut prepared =
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            &device,
            selection,
            scanout_descriptor(Size {
                width: 1280,
                height: 720,
            }),
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip().with_cursor(cursor),
        );
    let submitted = submit_prepared_native_primary_plane_scanout(
        &device,
        prepared
            .prepared
            .take()
            .expect("successful preparation retains an affine owner"),
    );
    assert_eq!(
        submitted.status,
        LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert!(!submitted.cursor_dropped);
    assert_eq!(device.commits.get(), 1);
}

/// A rejected commit with no cursor aboard has nothing to drop, and stays a
/// rejection -- the retry must not resurrect ordinary failures.
#[test]
fn a_rejected_commit_without_a_cursor_stays_rejected() {
    let mut device = full_primary_plane_scanout_device();
    device.reject_commits_before = 1;
    let selection = select_native_primary_plane_target(&device);
    let mut prepared =
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            &device,
            selection,
            scanout_descriptor(Size {
                width: 1280,
                height: 720,
            }),
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip(),
        );
    let submitted = submit_prepared_native_primary_plane_scanout(
        &device,
        prepared
            .prepared
            .take()
            .expect("successful preparation retains an affine owner"),
    );
    assert_eq!(
        submitted.status,
        LibdrmNativePrimaryPlaneScanoutSubmitStatus::AtomicSubmitFailed
    );
    assert!(!submitted.cursor_dropped);
    assert_eq!(device.commits.get(), 1);
}

fn cursor_plane_property_handles() -> sophia_backend_live::LibdrmNativeCursorPlanePropertyHandles {
    let handle = |raw: u32| drm::control::from_u32(raw).unwrap();
    sophia_backend_live::LibdrmNativeCursorPlanePropertyHandles::new(
        handle(204),
        handle(205),
        handle(206),
        handle(207),
        handle(208),
        handle(209),
        handle(210),
        handle(211),
        handle(212),
        handle(213),
    )
}

#[test]
fn detailed_validation_preserves_errno_without_classifying_its_cause() {
    for error in [
        io::Error::from_raw_os_error(22), // EINVAL
        io::Error::from_raw_os_error(16), // EBUSY
        io::Error::from_raw_os_error(5),  // EIO
        io::Error::from_raw_os_error(11), // EAGAIN
        io::Error::other("non-OS failure"),
        io::Error::new(io::ErrorKind::WouldBlock, "non-OS pending"),
    ] {
        let expected_kind = error.kind();
        let expected_errno = error.raw_os_error();
        let mut device = full_primary_plane_scanout_device();
        device.submit = Err(error);
        let selection = select_native_primary_plane_target(&device);
        let prepared = sophia_backend_live::prepare_native_primary_plane_scanout_from_selection_and_renderer_dma_bufs_with_policy(
            &device,
            selection,
            scanout_descriptor(Size { width: 1280, height: 720 }),
            [Some(std::fs::File::open("/dev/null").unwrap().into()), None, None, None],
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip(),
        ).prepared.expect("fixture must own an imported framebuffer");
        let (report, prepared) =
            sophia_backend_live::validate_prepared_native_primary_plane_scanout_detailed(
                &device, prepared,
            );
        assert_eq!(
            report.status,
            if expected_kind == io::ErrorKind::WouldBlock {
                LibdrmNativeAtomicCommitSubmitStatus::WouldBlock
            } else {
                LibdrmNativeAtomicCommitSubmitStatus::Rejected
            }
        );
        assert_prepared_test_request(&report);
        assert_eq!(report.error_kind, Some(expected_kind));
        assert_eq!(report.raw_os_error, expected_errno);
        assert_eq!(
            report.request_scope,
            LibdrmNativeAtomicCommitRequestScope::PageFlip
        );
        assert!(report.commit_flags.test_only);
        assert!(!report.commit_flags.page_flip_event);
        assert!(!report.commit_flags.allow_modeset);
        assert_eq!(
            device.commits(),
            1,
            "detailed reporting must not issue another ioctl"
        );
        assert_eq!(device.test_only_commits(), 1);
        assert_eq!(device.imported_buffers(), 1);
        assert_eq!(device.destroyed_framebuffers(), 0);
        assert_eq!(device.closed_buffers(), 0);
        let cancelled = cancel_prepared_native_primary_plane_scanout(&device, prepared);
        assert_eq!(
            cancelled.status,
            LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed
        );
        assert_eq!(device.destroyed_framebuffers(), 1);
        assert_eq!(device.closed_buffers(), 1);
        assert_eq!(device.commits(), 1);
    }
}

#[test]
fn detailed_validation_success_records_test_flags_and_preserves_the_real_request() {
    let device = full_primary_plane_scanout_device();
    let selection = select_native_primary_plane_target(&device);
    let prepared =
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            &device,
            selection,
            scanout_descriptor(Size {
                width: 1280,
                height: 720,
            }),
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip(),
        )
        .prepared
        .unwrap();
    let (report, prepared) =
        sophia_backend_live::validate_prepared_native_primary_plane_scanout_detailed(
            &device, prepared,
        );
    assert_eq!(
        report.status,
        LibdrmNativeAtomicCommitSubmitStatus::Submitted
    );
    assert_prepared_test_request(&report);
    assert_eq!(report.error_kind, None);
    assert_eq!(report.raw_os_error, None);
    assert!(report.commit_flags.test_only && !report.commit_flags.page_flip_event);
    assert_eq!(device.commits(), 1);
    assert_eq!(device.test_only_commits(), 1);
    assert_eq!(device.destroyed_framebuffers(), 0);
    let submitted = submit_prepared_native_primary_plane_scanout(&device, prepared);
    assert_eq!(
        submitted.status,
        LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    let flags = submitted.commit_flags.unwrap();
    assert!(!flags.test_only && flags.page_flip_event);
    assert_eq!(device.commits(), 2);
    assert_eq!(
        device.test_only_commits(),
        1,
        "the returned owner still carries the committing request"
    );
}

fn assert_prepared_test_request(report: &sophia_backend_live::LibdrmNativeAtomicTestReport) {
    let request = report
        .request
        .expect("prepared primary plane retains canonical request facts");
    assert_eq!(request.flags, report.commit_flags);
    assert_eq!(request.scope, report.request_scope);
    assert_eq!(
        request.primary_framebuffer(),
        (u32::from(plane_handle()), 104)
    );
    let framebuffer = request
        .properties()
        .iter()
        .find(|row| (row.object, row.property) == request.primary_framebuffer())
        .expect("the exact framebuffer property must be present");
    assert_eq!(
        framebuffer.value,
        u64::from(u32::from(framebuffer_handle()))
    );
}

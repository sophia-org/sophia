use sophia_renderer_live::LiveRendererScanoutBufferExportDetail as Detail;
use sophia_session::diagnostics::reduced_record;

const ALL_DETAILS: &[Detail] = &[
    Detail::Exported,
    Detail::WorkerPending,
    Detail::WorkerQueueFull,
    Detail::WorkerDisconnected,
    Detail::WorkerStalled,
    Detail::InvalidTarget,
    Detail::ComposeRefused,
    Detail::BackendDeviceUnavailable,
    Detail::GbmDeviceUnavailable,
    Detail::EglUnavailable,
    Detail::EglDisplayUnavailable,
    Detail::EglInitializeFailed,
    Detail::EglBindApiFailed,
    Detail::EglConfigUnavailable,
    Detail::GbmSurfaceUnavailable,
    Detail::EglSurfaceUnavailable,
    Detail::EglContextUnavailable,
    Detail::EglMakeCurrentFailed,
    Detail::GlSmokeFailed,
    Detail::CpuLayerUploadFailed,
    Detail::DmaBufImageCreateFailed,
    Detail::DmaBufImageBindFailed,
    Detail::CompositionDrawFailed,
    Detail::CompositionFinishFailed,
    Detail::EglImageDestroyFailed,
    Detail::DmaBufImportFailed,
    Detail::EglSwapBuffersFailed,
    Detail::FrontBufferLockFailed,
    Detail::InvalidBufferDescriptor,
    Detail::InvalidRendererImageId,
    Detail::DmaBufDescriptorMismatch,
    Detail::DmaBufImportCacheFull,
    Detail::RendererImageStoreFull,
    Detail::PendingFrameMissing,
    Detail::ExportedDescriptorMissing,
    Detail::ExportedOwnerMissing,
    Detail::WorkerLeaseIdExhausted,
    Detail::FrameSlotIncarnationExhausted,
    Detail::RetainedBufferMissing,
];

fn kept(record: &str) {
    assert_eq!(reduced_record(record).as_deref(), Some(record), "{record}");
}

#[test]
fn each_rare_worker_status_keeps_exactly_its_fields() {
    for record in [
        "sophia_renderer_worker schema=3 status=soft_stall output=2 request=17 age_ms=104",
        "sophia_renderer_worker schema=3 status=hard_stall output=2 request=17 age_ms=1043 abandon_after_ms=10000",
        "sophia_renderer_worker schema=3 status=stall_recovered output=2 request=17 age_ms=2210",
        "sophia_renderer_worker schema=3 status=failed output=2 request=0 detail=PendingFrameMissing",
        "sophia_renderer_worker schema=3 status=result_misrouted output=2 observed=3 request=17",
        "sophia_renderer_worker schema=3 status=result_misrouted output=2 expected=4 observed=3 request=17",
        "sophia_live_present_defer schema=1 status=output_busy defers=8 transaction=1650791 output=1 in_flight=true cleanup_pending=false pending_frame=true",
    ] {
        kept(record);
    }
}

#[test]
fn every_detail_variant_name_is_admitted_on_failure_only() {
    for detail in ALL_DETAILS {
        kept(&format!(
            "sophia_renderer_worker schema=3 status=failed output=1 request=9 detail={detail:?}"
        ));
        assert_eq!(
            reduced_record(&format!(
                "sophia_renderer_worker schema=3 status=hard_stall output=1 request=9 age_ms=1000 detail={detail:?}"
            ))
            .as_deref(),
            Some("sophia_renderer_worker schema=3 status=hard_stall output=1 request=9 age_ms=1000"),
            "{detail:?} outside a failure"
        );
    }
    let base = "sophia_renderer_worker schema=3 status=failed output=1 request=9";
    for detail in [
        "NotAVariant",
        "pendingframemissing",
        "Some(Exported)",
        "Exported,",
        "\"Exported\"",
        "",
    ] {
        assert_eq!(
            reduced_record(&format!("{base} detail={detail}")).as_deref(),
            Some(base),
            "{detail}"
        );
    }
}

#[test]
fn fields_are_scoped_to_their_status() {
    for (record, reduced) in [
        (
            "sophia_renderer_worker schema=3 status=failed output=1 request=9 age_ms=5 abandon_after_ms=10000 observed=2",
            "sophia_renderer_worker schema=3 status=failed output=1 request=9",
        ),
        (
            "sophia_renderer_worker schema=3 status=soft_stall output=1 request=9 age_ms=101 abandon_after_ms=10000 expected=2",
            "sophia_renderer_worker schema=3 status=soft_stall output=1 request=9 age_ms=101",
        ),
        (
            "sophia_renderer_worker schema=3 status=result_misrouted output=1 observed=2 request=9 age_ms=4",
            "sophia_renderer_worker schema=3 status=result_misrouted output=1 observed=2 request=9",
        ),
        (
            "sophia_live_present_defer schema=1 status=output_busy defers=1 transaction=2 output=1 in_flight=false cleanup_pending=true pending_frame=false request=4 age_ms=9",
            "sophia_live_present_defer schema=1 status=output_busy defers=1 transaction=2 output=1 in_flight=false cleanup_pending=true pending_frame=false",
        ),
    ] {
        assert_eq!(reduced_record(record).as_deref(), Some(reduced), "{record}");
    }
}

#[test]
fn private_and_malformed_values_never_cross() {
    for (record, reduced) in [
        (
            "sophia_renderer_worker schema=3 status=hard_stall output=2 request=17 age_ms=1043 abandon_after_ms=10000 path=/private title=secret error=EGL_BAD_MATCH payload=1",
            "sophia_renderer_worker schema=3 status=hard_stall output=2 request=17 age_ms=1043 abandon_after_ms=10000",
        ),
        (
            "sophia_renderer_worker schema=3 status=soft_stall output=-1 request=0x11 age_ms=18446744073709551616",
            "sophia_renderer_worker schema=3 status=soft_stall",
        ),
        (
            "sophia_renderer_worker schema=3 status=soft_stall output=18446744073709551615 request=007 age_ms=1.5",
            "sophia_renderer_worker schema=3 status=soft_stall output=18446744073709551615 request=007",
        ),
        (
            "sophia_renderer_worker schema=2 status=failed output=2 request=1 detail=WorkerStalled",
            "sophia_renderer_worker status=failed output=2 request=1 detail=WorkerStalled",
        ),
        (
            "sophia_live_present_defer schema=1 status=output_busy defers=8 transaction=1 output=1 in_flight=True cleanup_pending=1 pending_frame=yes",
            "sophia_live_present_defer schema=1 status=output_busy defers=8 transaction=1 output=1",
        ),
        // The first occurrence of a key decides; a later one cannot replace it.
        (
            "sophia_renderer_worker schema=3 status=failed output=2 request=5 detail=WorkerStalled output=9 status=hard_stall detail=Exported",
            "sophia_renderer_worker schema=3 status=failed output=2 request=5 detail=WorkerStalled",
        ),
    ] {
        assert_eq!(reduced_record(record).as_deref(), Some(reduced), "{record}");
    }
}

#[test]
fn chatter_and_unknown_statuses_are_refused_whole() {
    for record in [
        "sophia_renderer_worker schema=2 status=request_submitted request=1 requests=1",
        "sophia_renderer_worker schema=2 status=request_received request=1 expected=1 age_ms=3",
        "sophia_renderer_worker schema=2 status=render_started output=1 request=1 frame_kind=mixed",
        "sophia_renderer_worker schema=3 status=startup_failed reason=image_import_devices error=private",
        "sophia_renderer_worker schema=3 status=duplicate_output output=1",
        "sophia_renderer_worker schema=1 status=cpu_buffer_reuse_failed detail=CompositionDrawFailed",
        "sophia_renderer_worker schema=3 output=2 request=1 age_ms=1043",
        "sophia_renderer_worker schema=3 status= output=2",
        "sophia_live_present_defer schema=1 status=output_idle defers=1 transaction=1 output=1",
        "sophia_live_present_defer schema=1 defers=1 transaction=1 output=1",
    ] {
        assert_eq!(reduced_record(record), None, "{record}");
    }
}

#[test]
fn only_the_first_sixteen_fields_are_read() {
    let mut record = String::from("sophia_renderer_worker schema=3 status=hard_stall");
    for index in 0..14 {
        record.push_str(&format!(" filler{index}=1"));
    }
    record.push_str(" output=2 request=3 age_ms=1000 abandon_after_ms=10000");
    assert_eq!(
        reduced_record(&record).as_deref(),
        Some("sophia_renderer_worker schema=3 status=hard_stall")
    );
}

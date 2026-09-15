mod evidence;
mod policy;
mod reduced_context;

use super::super::{RealAtomicScanoutPageFlipSession, RealAtomicScanoutPageFlipWaitPolicy};
use crate::prelude::*;

use evidence::reduced_smoke_evidence_for_phase;
use policy::submit_policy_for_smoke_phase;
use reduced_context::reduced_rendered_context_status_from_native;

#[derive(Debug)]
pub(super) struct RealAtomicScanoutSubmittedSmokePhase {
    pub(super) phase: LibdrmNativeAtomicScanoutSmokePhase,
    pub(super) scanout_target: LiveKmsScanoutTargetStatus,
    pub(super) rendered_context: Option<LibdrmNativeRenderedScanoutContextStatus>,
    pub(super) export_status: LiveRendererScanoutBufferExportStatus,
    pub(super) export_detail: LiveRendererScanoutBufferExportDetail,
    pub(super) submit: LibdrmNativePrimaryPlaneScanoutSubmitResult,
    pub(super) submission:
        Option<LiveRenderedPrimaryPlaneScanoutSubmission<NativeGbmRenderedScanoutOwner>>,
}

impl RealAtomicScanoutSubmittedSmokePhase {
    pub(super) fn evidence(
        &self,
        poll: Option<&LibdrmPageFlipEventPollReport>,
        callback: Option<&LivePageFlipCallbackReport>,
        retire: Option<&LibdrmNativePrimaryPlaneScanoutRetireResult>,
    ) -> LibdrmNativeAtomicScanoutSmokeEvidence {
        reduced_smoke_evidence_for_phase(
            self.phase,
            LibdrmNativeScanoutPipelineReports {
                scanout_target: self.scanout_target,
                rendered_context: self.rendered_context,
                gbm_export: self.export_status,
                gbm_export_detail: self.export_detail,
                submit: Some(&self.submit),
                poll,
                callback,
                retire,
            },
        )
    }
}

impl RealAtomicScanoutPageFlipSession {
    pub fn initialize_persistent_native_gbm_scanout<P, R>(
        &mut self,
        runtime: &mut LiveBackendRuntimeAssembly<P>,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<R>,
    ) -> Result<(), LibdrmNativeAtomicScanoutSmokeEvidence>
    where
        P: NonBlockingInputPoller,
        R: RenderDeviceDiscoveryBackend,
    {
        self.initialize_persistent_native_gbm_scanout_for_selection(
            runtime,
            exporter,
            self.selection(),
        )
    }

    pub fn initialize_persistent_native_gbm_scanout_for_selection<P, R>(
        &mut self,
        runtime: &mut LiveBackendRuntimeAssembly<P>,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<R>,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> Result<(), LibdrmNativeAtomicScanoutSmokeEvidence>
    where
        P: NonBlockingInputPoller,
        R: RenderDeviceDiscoveryBackend,
    {
        let mut submitted = self.submit_native_gbm_rendered_primary_plane_smoke_phase_with_policy(
            LibdrmNativeAtomicScanoutSmokePhase::InitialModeset,
            exporter,
            LibdrmNativePrimaryPlaneScanoutSubmitPolicy::blocking_modeset(),
            selection,
        )?;
        let Some(submission) = submitted.submission.take() else {
            let mut evidence = submitted.evidence(None, None, None);
            evidence.status = LibdrmNativeAtomicScanoutSmokeStatus::RetainedResourceMissing;
            return Err(evidence);
        };
        if !runtime.adopt_presented_rendered_primary_plane_scanout(submission) {
            let mut evidence = submitted.evidence(None, None, None);
            evidence.status = LibdrmNativeAtomicScanoutSmokeStatus::RetainedResourceMissing;
            return Err(evidence);
        }
        Ok(())
    }

    pub fn run_native_gbm_rendered_primary_plane_smoke_phase<R>(
        &mut self,
        phase: LibdrmNativeAtomicScanoutSmokePhase,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<R>,
        intake: &mut LivePageFlipCallbackIntake,
        wait_policy: RealAtomicScanoutPageFlipWaitPolicy,
    ) -> LibdrmNativeAtomicScanoutSmokeEvidence
    where
        R: RenderDeviceDiscoveryBackend,
    {
        let mut submitted =
            match self.submit_native_gbm_rendered_primary_plane_smoke_phase(phase, exporter) {
                Ok(submitted) => submitted,
                Err(evidence) => return evidence,
            };

        let Some(submission) = submitted.submission.take() else {
            let mut evidence = submitted.evidence(None, None, None);
            evidence.status = LibdrmNativeAtomicScanoutSmokeStatus::RetainedResourceMissing;
            return evidence;
        };
        let page_flip =
            self.wait_for_rendered_submitted_page_flip_retirement(intake, submission, wait_policy);
        submitted.evidence(
            Some(&page_flip.poll),
            page_flip.callback_report.as_ref(),
            page_flip.retired.as_ref(),
        )
    }

    pub(super) fn submit_native_gbm_rendered_primary_plane_smoke_phase<R>(
        &mut self,
        phase: LibdrmNativeAtomicScanoutSmokePhase,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<R>,
    ) -> Result<RealAtomicScanoutSubmittedSmokePhase, LibdrmNativeAtomicScanoutSmokeEvidence>
    where
        R: RenderDeviceDiscoveryBackend,
    {
        self.submit_native_gbm_rendered_primary_plane_smoke_phase_with_policy(
            phase,
            exporter,
            submit_policy_for_smoke_phase(phase),
            self.selection(),
        )
    }

    pub(super) fn submit_native_gbm_rendered_primary_plane_smoke_phase_with_policy<R>(
        &mut self,
        phase: LibdrmNativeAtomicScanoutSmokePhase,
        exporter: &mut NativeGbmRenderedScanoutBufferDiscoveryExporter<R>,
        submit_policy: LibdrmNativePrimaryPlaneScanoutSubmitPolicy,
        selected: LibdrmNativePrimaryPlaneSelection,
    ) -> Result<RealAtomicScanoutSubmittedSmokePhase, LibdrmNativeAtomicScanoutSmokeEvidence>
    where
        R: RenderDeviceDiscoveryBackend,
    {
        let target = LiveGbmEglFrameTargetRecord::new(selected.size());
        let scanout_target = reduced_scanout_target_status_from_native_selection(
            LiveKmsScanoutTargetStatus::Ready,
            target,
            &LibdrmNativePrimaryPlaneSelectionResult {
                status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                selection: Some(selected),
            },
        );
        if scanout_target != LiveKmsScanoutTargetStatus::Ready {
            return Err(reduced_smoke_evidence_for_phase(
                phase,
                LibdrmNativeScanoutPipelineReports {
                    scanout_target,
                    rendered_context: None,
                    gbm_export: LiveRendererScanoutBufferExportStatus::Unavailable,
                    gbm_export_detail:
                        LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable,
                    submit: None,
                    poll: None,
                    callback: None,
                    retire: None,
                },
            ));
        }

        let export = exporter.export_rendered_scanout_buffer(target).normalized();
        let rendered_context =
            reduced_rendered_context_status_from_native(exporter.context_status());
        if export.status != LiveRendererScanoutBufferExportStatus::Exported {
            return Err(reduced_smoke_evidence_for_phase(
                phase,
                LibdrmNativeScanoutPipelineReports {
                    scanout_target,
                    rendered_context,
                    gbm_export: export.status,
                    gbm_export_detail: export.detail,
                    submit: None,
                    poll: None,
                    callback: None,
                    retire: None,
                },
            ));
        }

        let (Some(descriptor), Some(owned_buffer)) = (export.descriptor, export.owner) else {
            let mut evidence = reduced_smoke_evidence_for_phase(
                phase,
                LibdrmNativeScanoutPipelineReports {
                    scanout_target,
                    rendered_context,
                    gbm_export: export.status,
                    gbm_export_detail: export.detail,
                    submit: None,
                    poll: None,
                    callback: None,
                    retire: None,
                },
            );
            evidence.status = LibdrmNativeAtomicScanoutSmokeStatus::RetainedResourceMissing;
            return Err(evidence);
        };

        let prime_fds = owned_buffer.export_scanout_dma_buf_fds().ok().flatten();
        let mut submit = if let Some(prime_fds) = prime_fds {
            submit_native_primary_plane_scanout_from_selection_and_renderer_dma_bufs_with_policy(
                self.card(),
                LibdrmNativePrimaryPlaneSelectionResult {
                    status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                    selection: Some(selected),
                },
                descriptor,
                prime_fds.into_plane_fds(),
                submit_policy,
            )
        } else {
            submit_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
                self.card(),
                LibdrmNativePrimaryPlaneSelectionResult {
                    status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                    selection: Some(selected),
                },
                descriptor,
                submit_policy,
            )
        };
        if submit.status != LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
        {
            return Err(reduced_smoke_evidence_for_phase(
                phase,
                LibdrmNativeScanoutPipelineReports {
                    scanout_target,
                    rendered_context,
                    gbm_export: export.status,
                    gbm_export_detail: export.detail,
                    submit: Some(&submit),
                    poll: None,
                    callback: None,
                    retire: None,
                },
            ));
        }
        let Some(submission) = submit.submission.take() else {
            let mut evidence = reduced_smoke_evidence_for_phase(
                phase,
                LibdrmNativeScanoutPipelineReports {
                    scanout_target,
                    rendered_context,
                    gbm_export: export.status,
                    gbm_export_detail: export.detail,
                    submit: Some(&submit),
                    poll: None,
                    callback: None,
                    retire: None,
                },
            );
            evidence.status = LibdrmNativeAtomicScanoutSmokeStatus::RetainedResourceMissing;
            return Err(evidence);
        };

        Ok(RealAtomicScanoutSubmittedSmokePhase {
            phase,
            scanout_target,
            rendered_context,
            export_status: export.status,
            export_detail: export.detail,
            submit,
            submission: Some(LiveRenderedPrimaryPlaneScanoutSubmission {
                scanout_buffer: owned_buffer,
                correlation: export.correlation,
                primary_plane: submission,
                submitted_after_page_flip_serial: None,
                layout_witness: None,
            }),
        })
    }
}

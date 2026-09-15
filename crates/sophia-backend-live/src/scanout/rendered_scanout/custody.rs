//! Persistent custody shared by singleton and mirrored native heads.
//!
//! A successful flip and destruction of its predecessor are independent facts.
//! Cleanup storage is reserved before transferring any displayed/prepared owner.

use crate::prelude::*;

const CLEANUP_CAPACITY: usize = 3;

#[derive(Debug)]
enum Retiring {
    Reserved,
    Submission(BoxedRenderedPrimaryPlaneScanoutSubmission),
    Cleanup(BoxedRenderedPrimaryPlaneScanoutCleanup),
}

#[derive(Debug)]
struct SubmittedOwner {
    payload: BoxedRenderedPrimaryPlaneScanoutSubmission,
    predecessor_cleanup_slot: usize,
}

#[derive(Debug, Default)]
pub(crate) struct PersistentScanoutCustody {
    submitted: Option<SubmittedOwner>,
    displayed: Option<BoxedRenderedPrimaryPlaneScanoutSubmission>,
    retiring: [Option<Retiring>; CLEANUP_CAPACITY],
    cleanup_cursor: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScanoutCleanupOutcome {
    pub correlation: Option<crate::LiveRendererFrameCorrelation>,
    pub destroy: LibdrmNativePrimaryPlaneResourceDestroyStatus,
    pub released: bool,
}

#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "bounded stack result carries both presentation and predecessor cleanup facts without allocating on each physical retirement"
)]
pub(crate) enum PersistentFlipOutcome {
    NoSubmission,
    Waiting,
    IdentityMismatch,
    Presented {
        correlation: Option<crate::LiveRendererFrameCorrelation>,
        layout_witness: Option<crate::LiveScanoutLayoutWitness>,
        previous_cleanup: Option<ScanoutCleanupOutcome>,
    },
}

impl PersistentScanoutCustody {
    pub(crate) fn submitted(&self) -> Option<&BoxedRenderedPrimaryPlaneScanoutSubmission> {
        self.submitted.as_ref().map(|owner| &owner.payload)
    }

    pub(crate) fn displayed(&self) -> Option<&BoxedRenderedPrimaryPlaneScanoutSubmission> {
        self.displayed.as_ref()
    }

    pub(crate) fn cleanup_pending(&self) -> bool {
        self.retiring
            .iter()
            .any(|owner| matches!(owner, Some(Retiring::Submission(_) | Retiring::Cleanup(_))))
    }

    pub(crate) fn can_submit(&self) -> bool {
        self.submitted.is_none() && !self.cleanup_pending() && self.free_cleanup_slot().is_some()
    }

    #[expect(
        clippy::result_large_err,
        reason = "refusal returns the actual existing owner without allocating on an ownership or cleanup handoff"
    )]
    pub(crate) fn accept_submission(
        &mut self,
        submission: BoxedRenderedPrimaryPlaneScanoutSubmission,
    ) -> Result<(), BoxedRenderedPrimaryPlaneScanoutSubmission> {
        if !self.can_submit() {
            return Err(submission);
        }
        let slot = self
            .free_cleanup_slot()
            .expect("capacity checked before transfer");
        self.retiring[slot] = Some(Retiring::Reserved);
        self.submitted = Some(SubmittedOwner {
            payload: submission,
            predecessor_cleanup_slot: slot,
        });
        Ok(())
    }

    #[expect(
        clippy::result_large_err,
        reason = "refusal returns the actual existing owner without allocating on an ownership or cleanup handoff"
    )]
    pub(crate) fn adopt_displayed(
        &mut self,
        submission: BoxedRenderedPrimaryPlaneScanoutSubmission,
    ) -> Result<(), BoxedRenderedPrimaryPlaneScanoutSubmission> {
        if self.submitted.is_some() || self.displayed.is_some() {
            return Err(submission);
        }
        self.displayed = Some(submission);
        Ok(())
    }

    pub(crate) fn can_cancel_prepared(&self) -> bool {
        self.free_cleanup_slot().is_some()
    }

    pub(crate) fn can_adopt_displayed(&self) -> bool {
        self.submitted.is_none() && self.displayed.is_none()
    }

    pub(crate) fn can_retire_displayed(&self) -> bool {
        self.displayed.is_none() || self.free_cleanup_slot().is_some()
    }

    fn free_cleanup_slot(&self) -> Option<usize> {
        self.retiring.iter().position(Option::is_none)
    }

    /// `expected` is absent only for legacy runtime helpers without native authority.
    pub(crate) fn present<D: LibdrmNativePrimaryPlaneResourceDevice>(
        &mut self,
        device: &D,
        callback: &LivePageFlipCallbackReport,
        expected: Option<crate::LiveNativeFrameIdentity>,
    ) -> PersistentFlipOutcome {
        let Some(submission) = self.submitted() else {
            return PersistentFlipOutcome::NoSubmission;
        };
        if submission.correlation().and_then(|frame| frame.native) != expected {
            return PersistentFlipOutcome::IdentityMismatch;
        }
        if callback.decision != LivePageFlipCallbackDecision::Accepted
            || callback.event.status != LivePageFlipEventStatus::Presented
            || submission
                .submitted_after_page_flip_serial
                .is_some_and(|baseline| {
                    callback
                        .event
                        .frame_serial
                        .is_none_or(|serial| serial <= baseline)
                })
        {
            return PersistentFlipOutcome::Waiting;
        }
        let SubmittedOwner {
            payload: mut submission,
            predecessor_cleanup_slot: slot,
        } = self.submitted.take().expect("validated submission");
        debug_assert!(matches!(self.retiring[slot], Some(Retiring::Reserved)));
        self.retiring[slot] = None;
        let correlation = submission.correlation();
        let layout_witness = submission.layout_witness();
        submission.clear_completion_fence();
        let previous_cleanup = self.displayed.replace(submission).map(|previous| {
            self.retiring[slot] = Some(Retiring::Submission(previous));
            self.retry_slot(device, slot)
        });
        PersistentFlipOutcome::Presented {
            correlation,
            layout_witness,
            previous_cleanup,
        }
    }

    /// Refusal leaves the displayed payload in its original cell.
    pub(crate) fn retire_displayed<D: LibdrmNativePrimaryPlaneResourceDevice>(
        &mut self,
        device: &D,
    ) -> Result<Option<ScanoutCleanupOutcome>, ()> {
        if self.displayed.is_none() {
            return Ok(None);
        }
        let slot = self.free_cleanup_slot().ok_or(())?;
        self.retiring[slot] = self.displayed.take().map(Retiring::Submission);
        Ok(Some(self.retry_slot(device, slot)))
    }

    pub(crate) fn retry_cleanup<D: LibdrmNativePrimaryPlaneResourceDevice>(
        &mut self,
        device: &D,
    ) -> Option<ScanoutCleanupOutcome> {
        let slot = (0..CLEANUP_CAPACITY)
            .map(|offset| (self.cleanup_cursor + offset) % CLEANUP_CAPACITY)
            .find(|slot| {
                matches!(
                    self.retiring[*slot],
                    Some(Retiring::Submission(_) | Retiring::Cleanup(_))
                )
            })?;
        Some(self.retry_slot(device, slot))
    }

    pub(crate) fn accept_cleanup(
        &mut self,
        cleanup: BoxedRenderedPrimaryPlaneScanoutCleanup,
    ) -> Result<(), BoxedRenderedPrimaryPlaneScanoutCleanup> {
        let Some(slot) = self.free_cleanup_slot() else {
            return Err(cleanup);
        };
        self.retiring[slot] = Some(Retiring::Cleanup(cleanup));
        Ok(())
    }

    pub(crate) fn discard_submitted<D: LibdrmNativePrimaryPlaneResourceDevice>(
        &mut self,
        device: &D,
    ) -> Option<ScanoutCleanupOutcome> {
        let SubmittedOwner {
            payload,
            predecessor_cleanup_slot: slot,
        } = self.submitted.take()?;
        self.retiring[slot] = Some(Retiring::Submission(payload));
        Some(self.retry_slot(device, slot))
    }

    pub(crate) fn retire_transient<D: LibdrmNativePrimaryPlaneResourceDevice>(
        &mut self,
        device: &D,
        callback: &LivePageFlipCallbackReport,
    ) -> Option<crate::LiveRenderedPrimaryPlaneScanoutRetireResult<Box<dyn std::any::Any>>> {
        let SubmittedOwner {
            payload,
            predecessor_cleanup_slot: slot,
        } = self.submitted.take()?;
        let mut result =
            crate::retire_rendered_primary_plane_scanout_after_page_flip(device, payload, callback);
        if let Some(payload) = result.submission.take() {
            self.submitted = Some(SubmittedOwner {
                payload,
                predecessor_cleanup_slot: slot,
            });
        } else {
            self.retiring[slot] = result.cleanup.take().map(Retiring::Cleanup);
        }
        Some(result)
    }

    #[expect(
        clippy::result_large_err,
        reason = "refusal returns the actual existing owner without allocating on an ownership or cleanup handoff"
    )]
    pub(crate) fn cancel_prepared<D, Owner>(
        &mut self,
        device: &D,
        prepared: crate::LivePreparedRenderedPrimaryPlaneScanout<Owner>,
    ) -> Result<ScanoutCleanupOutcome, crate::LivePreparedRenderedPrimaryPlaneScanout<Owner>>
    where
        D: LibdrmNativePrimaryPlaneResourceDevice,
        Owner: 'static,
    {
        let Some(slot) = self.free_cleanup_slot() else {
            return Err(prepared);
        };
        let correlation = prepared.correlation();
        let result = crate::cancel_prepared_rendered_primary_plane_scanout(device, prepared);
        let released = result.cleanup.is_none();
        self.retiring[slot] = result.cleanup.map(|cleanup| {
            Retiring::Cleanup(
                cleanup.map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
            )
        });
        Ok(ScanoutCleanupOutcome {
            correlation,
            destroy: result.destroy,
            released,
        })
    }

    fn retry_slot<D: LibdrmNativePrimaryPlaneResourceDevice>(
        &mut self,
        device: &D,
        slot: usize,
    ) -> ScanoutCleanupOutcome {
        // Failed attempts advance too: one stuck framebuffer cannot starve a sibling owner.
        self.cleanup_cursor = (slot + 1) % CLEANUP_CAPACITY;
        let (correlation, destroy, cleanup) =
            match self.retiring[slot].take().expect("owned cleanup") {
                Retiring::Reserved => unreachable!("submitted frame owns its cleanup reservation"),
                Retiring::Submission(submission) => {
                    let LiveRenderedPrimaryPlaneScanoutSubmission {
                        scanout_buffer,
                        correlation,
                        primary_plane,
                        ..
                    } = submission;
                    let result = primary_plane.retire(device);
                    let cleanup = result.cleanup.map(|primary_plane| {
                        LiveRenderedPrimaryPlaneScanoutCleanup {
                            scanout_buffer,
                            correlation,
                            primary_plane,
                        }
                    });
                    (correlation, result.status, cleanup)
                }
                Retiring::Cleanup(cleanup) => {
                    let correlation = cleanup.correlation();
                    let result =
                        crate::retry_rendered_primary_plane_scanout_cleanup(device, cleanup);
                    (correlation, result.destroy, result.cleanup)
                }
            };
        let released = cleanup.is_none();
        self.retiring[slot] = cleanup.map(Retiring::Cleanup);
        ScanoutCleanupOutcome {
            correlation,
            destroy,
            released,
        }
    }
}

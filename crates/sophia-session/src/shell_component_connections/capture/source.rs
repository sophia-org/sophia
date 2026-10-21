//! Pre-use custody while the existing native owner prepares an endpoint.
//! No wire admission: the future intake must negotiate formats and reserve
//! logical credit before calling this private owner. A source is never borrowed
//! by a renderer until submission moves it out of this table.
use super::*;
use crate::shell_capture_owner::MAX_JOBS;
use sophia_protocol::shell_files::ShellSourceDisposition;
use sophia_renderer_live::{LiveRendererImageId, LiveShellCaptureRequest, LiveShellCaptureSource};
use sophia_runtime::{ContentCaptureFailure, ContentCaptureReservation};
use std::time::Instant;

#[must_use = "Return refusal custody to the intake owner; never discard its reservation"]
pub(crate) struct Source {
    pub binding: Binding,
    pub source: LiveShellCaptureSource,
    pub reservation: ContentCaptureReservation,
    /// Intake must derive this from the negotiated validation/transfer timeout;
    /// callers may not extend that bound while an endpoint is being prepared.
    pub deadline: Instant,
}

struct Held {
    binding: Binding,
    source: LiveShellCaptureSource,
    reservation: Option<ContentCaptureReservation>,
    deadline: Instant,
    failure: Option<ContentCaptureFailure>,
}

#[derive(Default)]
pub(crate) struct Sources {
    held: [Option<Held>; MAX_JOBS],
}

impl Sources {
    pub(super) fn pending(&self) -> usize {
        self.held.iter().flatten().count()
    }
    pub(super) fn contains(&self, binding: Binding) -> bool {
        self.held
            .iter()
            .flatten()
            .any(|h| h.binding.conflicts(&binding))
    }
}

impl ShellComponentConnections {
    /// Supplied-source seam until negotiated DMA intake is installed. Refusal
    /// returns the exact fd and ticket, including capacity/identity failures.
    #[allow(dead_code)]
    // Keep bounded custody on the stack; refusing capacity must not require
    // another allocation merely to return the caller's owned source/ticket.
    #[allow(clippy::result_large_err)]
    pub(crate) fn retain_capture_source(
        &mut self,
        source: Source,
        now: u64,
    ) -> Result<(), (ContentStoreError, Source)> {
        let b = source.binding;
        let error = if !b.matches_source(&source.source)
            || source.reservation.description().grant != b.grant
            || *source.reservation.metadata() != b.metadata
            || self.capture_sources.contains(b)
            || self.capture.as_ref().is_some_and(|o| o.contains(b))
        {
            Some(ContentStoreError::Stale)
        } else if self.capture_pending() >= MAX_JOBS {
            Some(ContentStoreError::Budget)
        } else {
            self.prepare_capture_endpoint(
                ComponentConnectionKey {
                    slot: b.slot,
                    grant: b.grant,
                },
                &source.reservation,
                now,
            )
            .err()
        };
        if let Some(error) = error {
            return Err((error, source));
        }
        let slot = self
            .capture_sources
            .held
            .iter_mut()
            .find(|h| h.is_none())
            .expect("bounded sources");
        *slot = Some(Held {
            binding: b,
            source: source.source,
            reservation: Some(source.reservation),
            deadline: source.deadline,
            failure: None,
        });
        Ok(())
    }

    /// Destination byte bound must come from the renderer allocation policy,
    /// not the source byte count. A missing endpoint or full physical budget
    /// leaves source and logical credit here. No caller can claim completion.
    #[allow(dead_code)]
    pub(crate) fn submit_retained_capture_source(
        &mut self,
        binding: Binding,
        image_id: LiveRendererImageId,
        allocation_bound: u64,
        now: u64,
        instant: Instant,
    ) -> Result<(), ContentStoreError> {
        let index = self
            .capture_sources
            .held
            .iter()
            .position(|h| h.as_ref().is_some_and(|h| h.binding == binding))
            .ok_or(ContentStoreError::Stale)?;
        let held = self.capture_sources.held[index]
            .take()
            .expect("held source");
        let prepared = (|| {
            if held.failure.is_some() || instant >= held.deadline {
                return Err(ContentStoreError::Revoked);
            }
            let destination = self
                .prepare_capture_endpoint(
                    ComponentConnectionKey {
                        slot: binding.slot,
                        grant: binding.grant,
                    },
                    held.reservation.as_ref().ok_or(ContentStoreError::Stale)?,
                    now,
                )?
                .ok_or(ContentStoreError::Incomplete)?;
            let reservation = destination
                .budget
                .reserve_capture(allocation_bound)
                .map_err(|_| ContentStoreError::Budget)?;
            Ok((destination, reservation))
        })();
        let (destination, reservation) = match prepared {
            Ok(value) => value,
            Err(error) => {
                self.capture_sources.held[index] = Some(held);
                return Err(error);
            }
        };
        let request = LiveShellCaptureRequest {
            source: held.source,
            image_id,
            budget: destination.budget,
            reservation,
            epoch: destination.epoch,
        };
        match self.submit_capture(
            binding,
            request,
            held.reservation.expect("prepared ticket"),
            held.deadline,
            now,
        ) {
            Ok(_) => Ok(()),
            Err(rejected) => {
                // Submission refusal proved no renderer use; return to the
                // same slot and release only the unused physical reservation.
                self.capture_sources.held[index] = Some(Held {
                    binding,
                    source: rejected.request.source,
                    reservation: Some(rejected.reservation),
                    deadline: held.deadline,
                    failure: Some(ContentCaptureFailure::Renderer),
                });
                Err(ContentStoreError::Incomplete)
            }
        }
    }

    pub(super) fn service_capture_sources(
        &mut self,
        now: u64,
        instant: Instant,
    ) -> Vec<(Binding, ShellTransportError)> {
        self.service_capture_sources_with(now, instant, |connections, binding, disposition| {
            publish_current(connections, binding, |connection| {
                connection
                    .transport
                    .release_capture_image_source(binding.grant, binding.metadata, disposition)
                    .map(|_| ())
            })
        })
    }

    pub(super) fn service_capture_sources_with(
        &mut self,
        now: u64,
        instant: Instant,
        mut publish: impl FnMut(
            &mut [Connection],
            Binding,
            ShellSourceDisposition,
        ) -> Result<Publication, ShellTransportError>,
    ) -> Vec<(Binding, ShellTransportError)> {
        let mut errors = Vec::new();
        for index in 0..MAX_JOBS {
            let Some(mut held) = self.capture_sources.held[index].take() else {
                continue;
            };
            let b = held.binding;
            if held.failure.is_none() {
                held.failure = if instant >= held.deadline {
                    Some(ContentCaptureFailure::Timeout)
                } else {
                    match self.prepare_capture_endpoint(
                        ComponentConnectionKey {
                            slot: b.slot,
                            grant: b.grant,
                        },
                        held.reservation.as_ref().expect("unsettled source"),
                        now,
                    ) {
                        Ok(_) => None,
                        // A regressed clock cannot decide or settle custody.
                        Err(ContentStoreError::ClockRegression) => {
                            errors.push((b, ContentStoreError::ClockRegression.into()));
                            self.capture_sources.held[index] = Some(held);
                            continue;
                        }
                        Err(_) => Some(ContentCaptureFailure::Withdrawn),
                    }
                };
            }
            let result = if let Some(failure) = held.failure {
                // This table exclusively owns an unsubmitted descriptor. The
                // exact ticket must settle before any SourceReleased attempt;
                // retain the fd/slot through status and journal backpressure.
                let settled = if held.reservation.is_some() {
                    self.epochs
                        .fail_capture(&mut held.reservation, failure, now)
                } else {
                    Ok(())
                };
                match settled {
                    Err(error) => Err(error.into()),
                    Ok(()) => publish(
                        &mut self.connections,
                        b,
                        ShellSourceDisposition::RefusedBeforeUse,
                    )
                    .map(|_| true),
                }
            } else {
                Ok(false)
            };
            match result {
                Ok(true) => (),
                Ok(false) => self.capture_sources.held[index] = Some(held),
                Err(error) => {
                    errors.push((b, error));
                    self.capture_sources.held[index] = Some(held);
                }
            }
        }
        errors
    }
}

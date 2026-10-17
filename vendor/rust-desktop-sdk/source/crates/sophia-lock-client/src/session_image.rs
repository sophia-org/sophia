use super::*;
use crate::{ImageAdmission, ImageBeginError, ImageError, LockImageAdapter};
use sophia_image_producer::{ProducerError, ProducerMetadata, ReadyEvents};
use std::os::fd::OwnedFd;

impl Session {
    pub fn image(&self) -> Option<&LockImageAdapter> {
        self.image.as_ref()
    }
    pub fn image_fd(&self) -> Option<BorrowedFd<'_>> {
        self.image.as_ref().and_then(LockImageAdapter::fd)
    }
    pub fn image_interest(&self) -> ReadyEvents {
        self.image
            .as_ref()
            .map_or(ReadyEvents::NONE, LockImageAdapter::poll_events)
    }
    pub fn image_begin(
        &mut self,
        metadata: ProducerMetadata,
        source: OwnedFd,
        now: u64,
    ) -> Result<ImageAdmission, ImageBeginError> {
        let check = (|| {
            self.ready().map_err(|_| ImageError::NotReady)?;
            if now < self.now {
                return Err(ProducerError::Argument.into());
            }
            self.submission.can_admit().map_err(|_| ImageError::Busy)?;
            if self
                .image
                .as_ref()
                .ok_or(ImageError::NotEnabled)?
                .metadata
                .iter()
                .all(Option::is_some)
            {
                return Err(ImageError::Busy);
            }
            Ok(())
        })();
        if let Err(error) = check {
            return Err(ImageBeginError {
                error,
                source: Some(source),
            });
        }
        let image = self.image.as_mut().unwrap();
        let (offer, body, assigned) =
            image
                .producer
                .begin(metadata, source, now)
                .map_err(|(e, source)| ImageBeginError {
                    error: e.into(),
                    source: Some(source),
                })?;
        let submission = match self.submission.admit_image(&body) {
            Ok(id) => id,
            Err(_) => {
                let _ = image.producer.begin_refused(offer, now);
                return Err(ImageBeginError {
                    error: ImageError::Protocol,
                    source: None,
                });
            }
        };
        self.now = now;
        *image.metadata.iter_mut().find(|m| m.is_none()).unwrap() = Some(assigned);
        image.begin_submission = Some((submission, offer));
        self.dirty = true;
        Ok(ImageAdmission { offer, submission })
    }
    pub fn image_cancel(&mut self, offer: u64, now: u64) -> Result<u64, ImageError> {
        self.ready().map_err(|_| ImageError::NotReady)?;
        if now < self.now {
            return Err(ProducerError::Argument.into());
        }
        self.submission.can_admit().map_err(|_| ImageError::Busy)?;
        let image = self.image.as_mut().ok_or(ImageError::NotEnabled)?;
        let metadata = image
            .metadata
            .iter()
            .flatten()
            .find(|m| m.offer_id == offer)
            .ok_or(ProducerError::Argument)?;
        let candidate = Candidate::ResourceCancel(LockResourceStep {
            transaction: offer,
            resource: LockResourceId {
                id: metadata.resource,
                generation: metadata.resource_generation,
            },
            total_bytes: None,
        });
        let prepared = self
            .submission
            .prepare(&candidate)
            .map_err(|_| ImageError::Protocol)?;
        image.producer.cancel(offer, now)?;
        let id = self.submission.admit_prepared(prepared);
        self.now = now;
        self.image_cancel_submission = Some(id);
        self.dirty = true;
        Ok(id)
    }
    pub fn image_forget(&mut self, offer: u64) -> Result<(), ImageError> {
        let image = self.image.as_mut().ok_or(ImageError::NotEnabled)?;
        image.producer.forget(offer)?;
        if let Some(slot) = image
            .metadata
            .iter_mut()
            .find(|m| m.is_some_and(|m| m.offer_id == offer))
        {
            *slot = None;
        }
        Ok(())
    }
    pub(super) fn drive_image(&mut self) -> Result<bool, SessionError> {
        let Some(image) = &mut self.image else {
            return Ok(false);
        };
        image
            .read_complete(self.now)
            .map_err(|_| SessionError::Protocol("invalid image discovery"))?;
        image
            .drive(self.now)
            .map_err(|_| SessionError::Protocol("image drive failed"))?;
        if self.image_tag.is_some() {
            return Ok(false);
        }
        let name = image.name();
        let Some(read) = &mut image.read else {
            return Ok(false);
        };
        let wire = self.wire.as_mut().ok_or(SessionError::Closed)?;
        let mut fid = Fid::NOFID;
        use crate::image_read::Operation as Op;
        let result = match read
            .operation(wire.msize())
            .map_err(|_| SessionError::Protocol("image read operation"))?
        {
            Some(Op::Walk) => Some(wire.walk(self.root, &[name]).map(|(tag, f)| {
                fid = f;
                tag
            })),
            Some(Op::Open(f)) => Some(wire.lopen(f, 0)),
            Some(Op::Read { fid, offset, count }) => Some(wire.read(fid, offset, count)),
            Some(Op::Clunk(f)) => Some(wire.clunk(f)),
            None => None,
        };
        match result {
            Some(Ok(tag)) => {
                read.issued(wire.msize(), fid)
                    .map_err(|_| SessionError::Protocol("image read issue"))?;
                self.image_tag = Some(tag);
                Ok(true)
            }
            Some(Err(e)) if busy(&e) => Ok(false),
            Some(Err(e)) => Err(e.into()),
            None => Ok(false),
        }
    }
}

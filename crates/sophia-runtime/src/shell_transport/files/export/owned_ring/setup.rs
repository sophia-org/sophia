//! Registration proposals advance only after budget and journal acceptance.
use super::*;

impl ShellFiles {
    pub(in crate::shell_transport) fn service_owned_ring_setup(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        instant: Instant,
    ) -> Result<Option<(Identity, OwnedRing)>, Errno> {
        if !self.owned_disclosed() {
            self.reset_owned_ring();
            return Ok(None);
        }
        let Some(mut setup) = self.owned.registration.take() else {
            return Ok(None);
        };
        let result = setup.advance(
            epochs,
            &mut self.journal,
            &mut self.owned.published,
            instant,
        );
        self.owned.registration = Some(setup);
        result
    }

    /// Called only after the existing image peer authentication check.
    pub(in crate::shell_transport::files::export) fn receive_owned_source(
        &mut self,
        mut packet: crate::image_channel::ImageDatagram,
    ) -> Result<(), Errno> {
        if !self.owned_disclosed() || packet.descriptor_count() != 1 {
            return Err(Errno::EPROTO);
        }
        let request = Registration::decode_source(packet.bytes()).ok_or(Errno::EPROTO)?;
        let setup = self.owned.registration.as_mut().ok_or(Errno::EPROTO)?;
        if setup.request != request || Instant::now() >= setup.deadline {
            return Err(Errno::EPROTO);
        }
        let Phase::AwaitingFd { fd, .. } = &mut setup.phase else {
            return Err(Errno::EPROTO);
        };
        if fd.is_some() {
            return Err(Errno::EPROTO);
        }
        let source = packet.take_descriptor(0).ok_or(Errno::EPROTO)?;
        // The first native adapter qualifies bounded shmem via Linux seals.
        // The internal pread experiment remains separately usable seal-free.
        #[cfg(target_os = "linux")]
        {
            use rustix::fs::{SealFlags, fcntl_get_seals};
            let seals = fcntl_get_seals(&source).map_err(|_| Errno::EPROTO)?;
            if !seals.contains(SealFlags::GROW | SealFlags::SHRINK) {
                return Err(Errno::EPROTO);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(Errno::EACCES);
        }
        *fd = Some(source);
        Ok(())
    }
}

impl Setup {
    fn advance(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        journal: &mut Journal,
        published: &mut u64,
        instant: Instant,
    ) -> Result<Option<(Identity, OwnedRing)>, Errno> {
        let phase = std::mem::replace(&mut self.phase, Phase::Refused);
        self.phase = if instant >= self.deadline {
            match phase {
                Phase::AwaitingBudget { admitted, terminal } => Phase::Refusing {
                    reason: ContentReason::Timeout as u16,
                    admitted: Some(admitted),
                    terminal,
                },
                Phase::AwaitingFd { terminal, .. } | Phase::PublishingReady { terminal, .. } => {
                    Phase::Refusing {
                        reason: ContentReason::Timeout as u16,
                        admitted: None,
                        terminal,
                    }
                }
                other => other,
            }
        } else {
            phase
        };
        // At most three local transitions: budget, FD handoff, terminal event.
        // Missing input and backpressure return to the existing owner turn.
        for _ in 0..3 {
            let phase = std::mem::replace(&mut self.phase, Phase::Refused);
            match phase {
                Phase::AwaitingBudget { admitted, terminal } => {
                    let reservation = match epochs.reserve_owned_ring_source(
                        self.request.identity.grant,
                        self.request.geometry().map_err(|_| Errno::EINVAL)?,
                    ) {
                        Ok(value) => value,
                        Err(crate::ContentStoreError::Budget) => {
                            self.phase = Phase::AwaitingBudget { admitted, terminal };
                            return Ok(None);
                        }
                        Err(error) => {
                            self.phase = Phase::Refusing {
                                reason: if error == crate::ContentStoreError::Revoked {
                                    ContentReason::Revoked
                                } else {
                                    ContentReason::Stale
                                } as u16,
                                admitted: Some(admitted),
                                terminal,
                            };
                            continue;
                        }
                    };
                    match journal.append_owned_event(
                        admitted,
                        Event::ring(EventKind::RingAdmitted, self.request.identity, 0),
                    ) {
                        Ok(sequence) => *published = sequence,
                        Err(error) => {
                            self.phase = Phase::AwaitingBudget { admitted, terminal };
                            return Err(error);
                        }
                    }
                    self.phase = Phase::AwaitingFd {
                        fd: None,
                        reservation,
                        terminal,
                    };
                    return Ok(None);
                }
                Phase::AwaitingFd {
                    fd: Some(fd),
                    reservation,
                    terminal,
                } => match OwnedRing::admit(std::fs::File::from(fd), reservation) {
                    Ok(ring) => self.phase = Phase::PublishingReady { ring, terminal },
                    Err(_) => {
                        self.phase = Phase::Refusing {
                            reason: ContentReason::Malformed as u16,
                            admitted: None,
                            terminal,
                        }
                    }
                },
                Phase::PublishingReady { ring, terminal } => {
                    match journal.append_owned_event(
                        terminal,
                        Event::ring(EventKind::RingReady, self.request.identity, 0),
                    ) {
                        Ok(sequence) => {
                            *published = sequence;
                            self.phase = Phase::Ready;
                            return Ok(Some((self.request.identity, ring)));
                        }
                        Err(error) => {
                            self.phase = Phase::PublishingReady { ring, terminal };
                            return Err(error);
                        }
                    }
                }
                Phase::Refusing {
                    reason,
                    admitted,
                    terminal,
                } => {
                    match journal.append_owned_event(
                        terminal,
                        Event::ring(EventKind::RingRefused, self.request.identity, reason),
                    ) {
                        Ok(sequence) => {
                            *published = sequence;
                            if let Some(hold) = admitted {
                                journal.release_reservation(hold);
                            }
                            self.phase = Phase::Refused;
                            return Ok(None);
                        }
                        Err(error) => {
                            self.phase = Phase::Refusing {
                                reason,
                                admitted,
                                terminal,
                            };
                            return Err(error);
                        }
                    }
                }
                other => {
                    self.phase = other;
                    return Ok(None);
                }
            }
        }
        Ok(None)
    }
}

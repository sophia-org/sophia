// The capabilities one connection's execution is driven by, bound once to the
// source that already owns its worker.
//
// Split by subject from the source itself: the source is storage, and this is
// the authority to start what goes in it, to stop what is running there, and
// to say that nothing more will start. Both belong to the same connection, and
// that is the point -- a registered handle destination reached with somebody
// else's stop and notice is not a registered control path.

/// The exact capabilities one connection's worker is driven by.
///
/// DERIVED FROM ITS OWN SERVING OWNER, under the one acquisition of the home
/// that establishes the owner is there and live. Nothing here is supplied by a
/// caller: a pair handed in alongside a registered slot would be a pair
/// nothing had checked belonged to that connection.
///
/// THE ORIGINAL `Arc`s, NOT COPIES OF WHAT THEY HELD. A stop copied by value
/// is a different flag, and setting it would tell nobody.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
struct PrivateControlCredentials {
    /// The home this connection's owner was found in.
    ///
    /// PINNED BY OCCUPANT. A place is returned and taken again, so a home
    /// reached by number could be a successor's; this is the one the identity
    /// named at the moment it was resolved.
    home: Arc<PrivateOrderedHome>,
    /// That owner's own stop flag.
    stop: Arc<AtomicBool>,
    /// That owner's own notice, so a stop can be published without going back
    /// to the home for permission to say so.
    notice: Arc<PrivateOrderedWake>,
}

/// What preparing a connection's control context found.
///
/// EVERY REFUSAL IS ITS OWN, because they are answers to different questions
/// and a caller told the wrong one looks in the wrong place. None of them
/// starts anything, permits anything, sets a stop, takes queued work or
/// changes what a home is holding.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateControlRefusal {
    /// The store this connection's name is in has gone.
    StoreGone,
    /// That place is not this connection's any more.
    StaleName,
    /// The home is live and nothing has been bound into it, so there is no
    /// owner to take credentials from.
    NothingBound,
    /// Its connection has ended. A retained home is not this borrower's to
    /// drive, and driving is not how retained work is finished.
    Retained,
    /// A holder panicked inside the home.
    ///
    /// NOT RECOVERED INTO ELIGIBILITY. The lock was acquired -- that is what
    /// poisoning means -- and what is unknown is what is in there, which is
    /// exactly the thing preparation would be asserting.
    Unreadable,
    /// The home holds something, and it is not a serving owner.
    NotServing,
    /// A serving owner with no stop of its own.
    ///
    /// ITS BINDING SAID SO, and nothing here mints a replacement: a stop this
    /// context invented would be one the worker never looks at, and the
    /// connection would read as stoppable while being unstoppable.
    Unstoppable,
}

/// One connection's bound control context.
///
/// BORROWED FROM THE CUSTODY, and from the owner that keeps it. It reaches the
/// worker source and the credentials and owns neither; it does not keep the
/// registration alive, and it is not a permit, a schedule or a licence to race
/// anything.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
struct PrivateControlContext<'c> {
    custody: &'c PrivateEvidenceCustody,
    credentials: &'c PrivateControlCredentials,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl<'c> PrivateControlContext<'c> {
    /// This connection's home, as its owner was found in.
    fn home(&self) -> &'c Arc<PrivateOrderedHome> {
        &self.credentials.home
    }

    /// This connection's authoritative stop.
    fn stop(&self) -> &'c Arc<AtomicBool> {
        &self.credentials.stop
    }

    /// The notice its worker waits on.
    fn notice(&self) -> &'c Arc<PrivateOrderedWake> {
        &self.credentials.notice
    }

    /// The sink its worker leaves a classification in.
    fn exit_sink(&self) -> &'c Arc<PrivateWorkerExit> {
        self.custody.exit_sink()
    }

    /// Start this connection's worker into this connection's slot.
    ///
    /// NO SLOT, STOP OR NOTICE IS PASSED IN, because there is nothing to pass:
    /// the destination is the one this connection's reservation made and the
    /// credentials are the ones its own owner published. What a caller still
    /// supplies is the spawn itself, which is the approved transaction's.
    fn start(
        &self,
        spawn: impl FnOnce() -> std::io::Result<std::thread::JoinHandle<()>>,
    ) -> PrivateStartupOutcome {
        start_connection_worker(
            self.custody.worker_slot(),
            self.credentials.stop(),
            &self.credentials.notice,
            spawn,
        )
    }

    /// Tell this connection's worker to stop, and wake it so it looks.
    ///
    /// NOTHING IS REACQUIRED. The stop and the notice were resolved when this
    /// context was prepared, so a worker blocked while borrowing its own home
    /// is exactly the case this serves rather than the case it waits behind.
    fn cancel(&self) {
        cancel_connection_worker(&self.credentials.stop, &self.credentials.notice);
    }

    /// Tell this connection nothing more will start, and say what was there.
    fn depart(&self) -> PrivateDeparture {
        depart_connection(
            self.custody.worker_slot(),
            &self.credentials.stop,
            &self.credentials.notice,
        )
    }
}

#[cfg(unix)]
impl PrivateControlCredentials {
    fn stop(&self) -> &Arc<AtomicBool> {
        &self.stop
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl PrivateEvidenceCustody {
    /// Bind this connection's control capabilities, once.
    ///
    /// THE HOME IS RESOLVED BY OCCUPANT AND THE STORE IS RELEASED BEFORE IT IS
    /// ENTERED. Reaching a home by number would reach whatever is at that
    /// number now, and holding the store while borrowing the home would put
    /// every other connection behind this one's reading.
    ///
    /// BOUND ONCE AND NOT REBOUND. A second preparation finds what the first
    /// published rather than replacing it: an association that could be
    /// replaced is one where two operation views can be driving a connection
    /// with different stops, and the one that is not authoritative would be
    /// setting a flag nobody reads.
    ///
    /// IT INSPECTS AND CHANGES NOTHING. No permit, no stop, no queued capsule
    /// taken, no standing altered, and a refusal leaves this unprepared.
    fn prepare_control(&self) -> Result<PrivateControlContext<'_>, PrivateControlRefusal> {
        if let Some(bound) = self.source.control.get() {
            return Ok(PrivateControlContext {
                custody: self,
                credentials: bound,
            });
        }
        let found = self.identity.with_home(|home| {
            // The store's aggregate is already released here; this is the
            // home's own lock and nothing else is held under it. The home
            // itself is pinned by the same resolution that found the occupant.
            let pinned = Arc::clone(home);
            let borrow = home.borrow_live(|continuation| match continuation {
                PrivateOrderedContinuation::Serving { owner, .. } => owner
                    .stop
                    .as_ref()
                    .map(|stop| (Arc::clone(&owner.wake), Arc::clone(stop)))
                    .ok_or(PrivateControlRefusal::Unstoppable),
                _ => Err(PrivateControlRefusal::NotServing),
            });
            (pinned, borrow)
        });
        let (home, notice, stop) = match found {
            PrivateMaintenanceReach::StoreGone => return Err(PrivateControlRefusal::StoreGone),
            PrivateMaintenanceReach::Stale => return Err(PrivateControlRefusal::StaleName),
            PrivateMaintenanceReach::Reached((home, borrow)) => match borrow {
                PrivateHomeBorrow::Acted(Ok((notice, stop))) => (home, notice, stop),
                PrivateHomeBorrow::Acted(Err(refusal)) => return Err(refusal),
                PrivateHomeBorrow::Retained => return Err(PrivateControlRefusal::Retained),
                PrivateHomeBorrow::Empty => return Err(PrivateControlRefusal::NothingBound),
                PrivateHomeBorrow::Unreadable => return Err(PrivateControlRefusal::Unreadable),
            },
        };
        // Published into the source's own storage. A racing preparation that
        // got here first keeps its association, and this one takes that rather
        // than overwriting it: whichever is published is the connection's.
        let _ = self.source.control.set(PrivateControlCredentials {
            home,
            stop,
            notice,
        });
        Ok(PrivateControlContext {
            custody: self,
            credentials: self
                .source
                .control
                .get()
                .expect("a control association is published before it is read"),
        })
    }
}

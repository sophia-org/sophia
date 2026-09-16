// Starting a worker for one connection, and owning its handle from the moment
// there is one.
//
// NOTHING HERE IS WIRED. No production path starts a worker; what this is is
// the transaction that would, written so that every way out of it leaves the
// handle owned and the connection's stop authoritative.

/// Where a connection's worker handle lives.
///
/// THE DESTINATION IS THE RECORD, and it is held before anything is spawned.
/// Reserving somewhere for a handle is not the same as holding it: between a
/// spawn returning and a store into an unheld place, the handle lives in a
/// local that an unwind would drop -- detaching a running thread that nobody
/// can then join.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
struct PrivateWorkerSlot {
    /// The worker whose handle this slot owns, if it still owns one.
    ///
    /// WITHIN A TRANSACTION, this is what says whether a spawn succeeded: a
    /// separate boolean can be stale in exactly the window that matters -- set
    /// late, or missed on an unwind -- and would then say no worker exists
    /// while one does.
    ///
    /// ACROSS A LIFETIME IT SAYS LESS. A handle handed to whoever joins it
    /// leaves this empty while the thread is still running, so emptiness here
    /// is not "nobody was ever started"; that is what the lifecycle beside it
    /// is for.
    handle: Option<std::thread::JoinHandle<()>>,
    /// Whether this connection has been told to depart.
    ///
    /// BESIDE THE HISTORY, NOT INSTEAD OF IT. A departing connection that
    /// never had a worker, one whose handle is still here, and one whose
    /// handle has gone to a joiner need completely different things done for
    /// them, and a single "departing" state that replaced the history would
    /// leave an empty slot that could be any of the three.
    departing: bool,
    /// What has become of this slot's worker, for as long as the slot lives.
    ///
    /// DURABLE, because the handle is not. Reading an empty handle as "never
    /// started" let a second worker be started for a connection whose first
    /// was alive and merely handed on to be joined.
    life: PrivateWorkerLife,
}

/// What has become of a connection's worker.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateWorkerLife {
    /// Nobody has ever been started here.
    NeverStarted,
    /// One exists and this slot holds its handle.
    Running,
    /// One exists and its handle has gone to whoever joins it.
    ///
    /// The state after that -- joined, and with what result -- belongs to
    /// whoever joins, which is not landed. It is not written here in advance
    /// of the thing that would establish it.
    HandedToJoiner,
}

/// What asking for a worker did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
#[derive(Debug, PartialEq, Eq)]
enum PrivateStartupOutcome {
    /// A worker exists, its handle is owned, and it has been permitted.
    Started,
    /// One is here now. The first stands.
    AlreadyStarted,
    /// Nothing could be spawned. Nothing exists to stop or join.
    SpawnRefused,
    /// A worker exists and could not be permitted.
    ///
    /// ITS OWN ANSWER, because the alternatives all lie: SpawnRefused would
    /// say nothing was attempted while a thread is running, and an unreadable
    /// destination would say the same. A worker exists, is cancelled, and is
    /// waiting to be joined.
    PermitRefused,
    /// A worker has existed here, and this slot does not start another.
    NoLongerStartable,
    /// The destination could not be read, so nothing was attempted.
    Unreadable,
}

/// Holds a startup transaction open, and closes it whatever happens.
///
/// The two ways out are not symmetrical, and the asymmetry is the point.
/// Before a spawn succeeds there is nothing to stop: the slot is empty, and
/// leaving it empty is the whole rollback. After one succeeds there may be a
/// thread running, and no unwind may pretend otherwise -- so the guard sets
/// the connection's own stop, wakes it, and LEAVES THE HANDLE WHERE IT IS for
/// whoever joins. It never restores a state that says no worker exists.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
struct PrivateStartupGuard<'a> {
    slot: &'a mut PrivateWorkerSlot,
    stop: &'a Arc<AtomicBool>,
    wake: &'a Arc<PrivateOrderedWake>,
    committed: bool,
}

#[cfg(unix)]
impl Drop for PrivateStartupGuard<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        // DERIVED FROM THE HANDLE, not from anything this guard was told.
        if self.slot.handle.is_none() {
            // Nothing was spawned. The slot is as empty as it was found, and
            // there is nothing running to cancel.
            return;
        }
        // A worker may be running. Cancel it through the connection's OWN stop
        // -- the one its serving already consults -- rather than a second flag
        // that only this arrangement knows about, and wake it so it looks.
        // The handle stays where it is: detaching it here would leave a thread
        // nobody can join.
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.wake.publish_recheck();
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
impl PrivateWorkerSlot {
    fn empty() -> Self {
        Self {
            handle: None,
            departing: false,
            life: PrivateWorkerLife::NeverStarted,
        }
    }

    /// Whether a worker's handle is owned here.
    ///
    /// A question about THIS MOMENT, used inside a transaction. Whether a
    /// worker has ever existed is `life`, and the two stop agreeing the moment
    /// a handle is handed on.
    fn running(&self) -> bool {
        self.handle.is_some()
    }
}

/// What handing a worker on produced.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing joins a worker yet.
struct PrivateWorkerHandoff {
    /// The worker, if this slot had one.
    handle: Option<std::thread::JoinHandle<()>>,
    /// Whether the slot it came from had been poisoned.
    ///
    /// REPORTED BESIDE THE HANDLE, NOT INSTEAD OF IT. A slot somebody panicked
    /// inside still owns whatever it owns, and answering "nothing here" would
    /// report accepted custody as absence -- leaving a running thread with no
    /// route to a join through this at all.
    source_poisoned: bool,
    /// The life this slot was in when the handle was asked for.
    ///
    /// READ BEFORE ANYTHING WAS CHANGED. Without it, "no handle" is one answer
    /// covering two different facts: a connection that never started one, and
    /// one whose handle has already gone to somebody else. A caller deciding
    /// what to do next needs them apart -- nothing was started is not a
    /// reason to stop expecting a join, and already handed on is.
    found: PrivateWorkerLife,
}

/// Give this connection's worker to whoever will join it.
///
/// THE SLOT REMEMBERS THAT IT HAD ONE. Handing the handle on empties it, and
/// an empty slot that had forgotten would start a second worker for a
/// connection whose first is still running.
///
/// IT DOES NOT REVOKE ANYTHING. Moving a handle is not a decision about
/// whether the worker should still be running: a transfer that cleared the
/// permit left a worker which had not yet reached its first look waiting for a
/// permission that had been taken back, with nothing telling it otherwise and
/// its handle already elsewhere. What bars a successor is the lifecycle, and
/// what stops a worker is cancellation, which is its own act.
///
/// NOTHING FOLLOWS THE TAKE. The lifecycle is written under the same lock the
/// handle came out of, and there is no later acquisition to cross -- so there
/// is no interval in which the handle is in a local and something else could
/// fail.
///
/// NO JOIN HERE, and no lock held across one. What comes back is the handle;
/// joining it is the caller's, after this returns.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing joins a worker yet.
fn hand_worker_to_joiner(slot: &Mutex<PrivateWorkerSlot>) -> PrivateWorkerHandoff {
    let (mut held, source_poisoned) = match slot.lock() {
        Ok(held) => (held, false),
        Err(poisoned) => (poisoned.into_inner(), true),
    };
    let found = held.life;
    let handle = held.handle.take();
    if handle.is_some() {
        held.life = PrivateWorkerLife::HandedToJoiner;
    }
    PrivateWorkerHandoff {
        handle,
        source_poisoned,
        found,
    }
}

/// What was here when a connection was told to depart.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing departs a connection yet.
#[derive(Debug, PartialEq, Eq)]
enum PrivateDeparture {
    /// Nobody was ever started. There is nothing to join.
    NothingStarted,
    /// A worker is here and its handle is in this slot.
    WorkerRunning,
    /// A worker is here and its handle has gone to whoever joins it.
    WorkerHandedOn,
    /// It was already departing. The first departure stands.
    AlreadyDeparting,
    /// The slot could not be read, so what was here is not established.
    ///
    /// THE STOP IS SET ANYWAY. Telling a worker to go does not depend on
    /// reading this, which is exactly why it happens first.
    Unreadable,
}

/// Tell this connection to go, and find out what is here to be dealt with.
///
/// THE STOP AND THE WAKE COME BEFORE THE WORKER-SLOT LOCK. The slot is held
/// across a spawn -- creating a thread is somebody else's latency -- so a
/// departure that wrote its no-more-starts decision first would wait behind
/// that spawn before telling the connection anything. Stopping a connection
/// must not queue behind starting one.
///
/// NOT LOCK-FREE, AND NOT BOUNDED. The stop itself is one atomic write, but
/// the wake that follows takes the connection's notice, so this is an order
/// between two locks rather than the absence of one, and nothing here says how
/// long either takes.
///
/// Then the decision, which is serialized against starting by the slot's own
/// lock: a start takes it and refuses a departing slot, a departure takes it
/// and marks one, and neither can interleave with the other.
///
/// WHAT WAS HERE IS REPORTED RATHER THAN ASSUMED, because the three histories
/// need different things: nothing to join, a handle to take, or a handle
/// already taken.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing departs a connection yet.
fn depart_connection(
    slot: &Mutex<PrivateWorkerSlot>,
    stop: &Arc<AtomicBool>,
    wake: &Arc<PrivateOrderedWake>,
) -> PrivateDeparture {
    cancel_connection_worker(stop, wake);
    let Ok(mut held) = slot.lock() else {
        return PrivateDeparture::Unreadable;
    };
    if held.departing {
        return PrivateDeparture::AlreadyDeparting;
    }
    held.departing = true;
    match held.life {
        PrivateWorkerLife::NeverStarted => PrivateDeparture::NothingStarted,
        PrivateWorkerLife::Running => PrivateDeparture::WorkerRunning,
        PrivateWorkerLife::HandedToJoiner => PrivateDeparture::WorkerHandedOn,
    }
}

/// Tell this connection's worker to stop, and wake it so it looks.
///
/// THE CONNECTION'S OWN STOP, the one its serving already consults, and a
/// recheck so a worker waiting on its notice goes and asks. Cancellation is
/// this, deliberately and by itself -- not a side effect of moving a handle,
/// and not the absence of a permit.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing cancels a worker yet.
fn cancel_connection_worker(stop: &Arc<AtomicBool>, wake: &Arc<PrivateOrderedWake>) {
    stop.store(true, std::sync::atomic::Ordering::Release);
    wake.publish_recheck();
}

/// Start one connection's worker, owning its handle from the moment it exists.
///
/// THE ORDER IS THE WHOLE CONTRACT:
///   the destination is held before anything is spawned;
///   the wake is NOT held across the spawn, so stopping and waking this
///     connection do not queue behind somebody else's thread creation;
///   the returned handle is stored into the already-held destination before
///     any further lock or anything that can fail;
///   only then is the permit published, under the wake.
///
/// A worker scheduled immediately cannot serve during any of that, because it
/// waits for a permit that is published last.
///
/// The spawner is supplied rather than named here so that a refusal can be
/// exercised: what production would pass is the ordinary thread builder.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
fn start_connection_worker<S>(
    slot: &Mutex<PrivateWorkerSlot>,
    stop: &Arc<AtomicBool>,
    wake: &Arc<PrivateOrderedWake>,
    spawn: S,
) -> PrivateStartupOutcome
where
    S: FnOnce() -> std::io::Result<std::thread::JoinHandle<()>>,
{
    let Ok(mut held) = slot.lock() else {
        return PrivateStartupOutcome::Unreadable;
    };
    // DEPARTING IS ASKED FIRST. A connection that has been told to go is not
    // one anything starts on, whatever its history.
    if held.departing {
        return PrivateStartupOutcome::NoLongerStartable;
    }
    // ASKED OF THE LIFECYCLE, not only of the handle. A slot whose worker has
    // been handed on to be joined is empty and is not free.
    match held.life {
        PrivateWorkerLife::NeverStarted => {}
        PrivateWorkerLife::Running => return PrivateStartupOutcome::AlreadyStarted,
        PrivateWorkerLife::HandedToJoiner => {
            return PrivateStartupOutcome::NoLongerStartable;
        }
    }
    if held.running() {
        return PrivateStartupOutcome::AlreadyStarted;
    }
    let mut guard = PrivateStartupGuard {
        slot: &mut held,
        stop,
        wake,
        committed: false,
    };
    // NOT UNDER THE WAKE. Creating a thread is somebody else's latency, and
    // holding this connection's notice across it would make stopping and
    // waking it wait for that -- and, because the permit below takes the same
    // notice, would deadlock this transaction against itself. A control that
    // holds it here does not fail: it hangs, which is the shape that mistake
    // has.
    let Ok(handle) = spawn() else {
        // Nothing exists. The guard's ordinary path leaves the slot empty.
        return PrivateStartupOutcome::SpawnRefused;
    };
    // STORED FIRST, into the destination already held, with nothing between
    // the spawn returning and this.
    guard.slot.handle = Some(handle);
    guard.slot.life = PrivateWorkerLife::Running;
    // And only now the permit, which is the last thing and a separate lock.
    //
    // IT CAN REFUSE. A permit taken from a notice somebody panicked inside is
    // not a permit: recovering that guard and writing into it would grant a
    // worker permission on the strength of a lock whose contents nobody stands
    // behind. The lock IS acquired -- that is what poisoning means -- and the
    // error carrying its guard is dropped here rather than recovered, so
    // nothing is still held when the cancellation below reacquires it.
    let permitted = match wake.state.lock() {
        Ok(mut state) => {
            state.started = true;
            true
        }
        Err(_) => false,
    };
    if !permitted {
        // The guard is not committed, so its own path runs: the connection's
        // stop is set, it is woken, and the handle stays here to be joined.
        return PrivateStartupOutcome::PermitRefused;
    }
    wake.ready.notify_all();
    guard.committed = true;
    PrivateStartupOutcome::Started
}

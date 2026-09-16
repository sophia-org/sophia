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
    /// The running worker, if there is one.
    ///
    /// POST-SPAWN STATUS IS READ FROM HERE and not from a flag beside it. A
    /// separate "spawned" boolean can be stale in exactly the window that
    /// matters -- set late, or missed on an unwind -- and would then say no
    /// worker exists while one does.
    handle: Option<std::thread::JoinHandle<()>>,
}

/// What asking for a worker did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts a worker yet.
#[derive(Debug, PartialEq, Eq)]
enum PrivateStartupOutcome {
    /// A worker exists, its handle is owned, and it has been permitted.
    Started,
    /// One is already here. The first stands.
    AlreadyStarted,
    /// Nothing could be spawned. Nothing exists to stop or join.
    SpawnRefused,
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
        Self { handle: None }
    }

    /// Whether a worker's handle is owned here.
    fn running(&self) -> bool {
        self.handle.is_some()
    }
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
    // And only now the permit, which is the last thing and a separate lock.
    {
        let mut state = wake
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.started = true;
    }
    wake.ready.notify_all();
    guard.committed = true;
    PrivateStartupOutcome::Started
}

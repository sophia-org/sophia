//! Final lock admission joins the actual file owner and live launch custody.
//! The transport worker's integration remains separate from this owner call.
use super::*;
use sophia_9p::export::{Access, AttachContext, Attachment, Export};
use sophia_runtime::lock_files::{LockFileExport, LockFileNode};

impl RoleAuthentication {
    /// The core has reserved root-fid and reply capacity. Check the lock view
    /// before pinning identity, then publish it without another fallible step.
    pub fn attach_lock(
        &mut self,
        view: &mut LockFileExport,
        context: &AttachContext<'_>,
        attempt: Attempt,
        now: Instant,
        book: &mut LaunchBook,
        supervisor: &mut ProcessSupervisor,
    ) -> Result<Attachment<LockFileNode>, Errno> {
        self.claims(
            context.connection,
            context.uname,
            context.aname,
            context.n_uname,
        )?;
        let prepared = view.prepare_attach(context)?;
        self.commit_attach(attempt, now, book, supervisor)?;
        Ok(prepared.publish())
    }

    /// The transport must call this for every retained file operation,
    /// including in-place upload fragments and retried waiting event reads.
    pub fn check_lock(
        &mut self,
        view: &mut LockFileExport,
        access: &Access<'_, LockFileNode>,
        now: Instant,
        book: &mut LaunchBook,
        supervisor: &mut ProcessSupervisor,
    ) -> Result<(), Errno> {
        self.check_admitted(now, book, supervisor)?;
        view.check(access)
    }
}

//! Metadata replies use the same fid and authority checks as data operations.
use super::*;

impl<E: Export> Connection<E> {
    pub(super) fn getattr(&mut self, export: &mut E, fid: Fid) -> Reply {
        let Some(state) = self.fids.get(&fid) else {
            return Reply::Lerror(Errno::EBADF);
        };
        if state.auth {
            return Reply::Lerror(Errno::EOPNOTSUPP);
        }
        if let Err(errno) = self.check(export, state.epoch, &state.node, Operation::Getattr) {
            return Reply::Lerror(errno);
        }
        let handle = state.open.as_ref().map(|opened| &opened.handle);
        let entry = export.describe(&state.node, handle);
        let file_type = match entry.kind {
            NodeKind::Directory => S_IFDIR,
            NodeKind::File => S_IFREG,
        };
        Reply::Getattr(Attr {
            valid: Attr::MODE | Attr::NLINK | Attr::INO | Attr::SIZE,
            qid: entry.qid(),
            mode: file_type | (entry.permissions & 0o7777),
            nlink: 1,
            size: entry.size,
        })
    }
}

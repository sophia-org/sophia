//! Shell exports execute walk and open in connection order. Queue both,
//! but withhold payload until both replies have succeeded, in either order.
use sophia_9p_client::pipeline::{Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, Tag};

use crate::ShellClientError;

#[cfg(test)]
#[path = "../../tests/support/writer_opening.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Opening {
    pub fid: Fid,
    pub walk: Option<Tag>,
    pub open: Option<Tag>,
    opened: bool,
    names: usize,
    flags: u32,
}

impl Opening {
    pub fn start(
        pipeline: &mut Pipeline,
        root: Fid,
        names: &[&[u8]],
        flags: u32,
    ) -> Result<Self, ShellClientError> {
        let (walk, fid) = pipeline.walk(root, names)?;
        let open = match pipeline.lopen(fid, flags) {
            Ok(tag) => Some(tag),
            // No request was queued. The walk reply releases its credit,
            // so this bounded pipeline can use the sequential path.
            Err(PipelineError::Limit(_)) => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            fid,
            walk: Some(walk),
            open,
            opened: false,
            names: names.len(),
            flags,
        })
    }

    /// None means another operation owns the tag; true means payload may start.
    pub fn receive(
        &mut self,
        pipeline: &mut Pipeline,
        tag: Tag,
        reply: &Reply,
    ) -> Result<Option<bool>, ShellClientError> {
        if self.walk == Some(tag) {
            if !matches!(reply, Reply::Walk(qids) if qids.len() == self.names) {
                return Err(ShellClientError::Protocol("writer walk refused"));
            }
            self.walk = None;
            if self.open.is_none() && !self.opened {
                self.open = Some(pipeline.lopen(self.fid, self.flags)?);
            }
        } else if self.open == Some(tag) {
            if !matches!(reply, Reply::Lopen { .. }) {
                return Err(ShellClientError::Protocol("writer open refused"));
            }
            self.open = None;
            self.opened = true;
        } else {
            return Ok(None);
        }
        Ok(Some(self.walk.is_none() && self.opened))
    }
}

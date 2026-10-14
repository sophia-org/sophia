//! Reply delivery preserves authority through partial writes and backpressure.
use super::{Access, Connection, Epoch, Errno, Export, Operation, Tag};

pub(super) struct QueuedReply<Node> {
    pub(super) tag: Tag,
    pub(super) remaining: usize,
    pub(super) delivery: Option<(Node, Epoch)>,
}

impl<E: Export> Connection<E> {
    /// Reply bytes not yet written, for inspection. Transport drivers must use
    /// [`Self::write_output`] so queued reads retain their delivery guards.
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Deliver an unguarded prefix or one guarded reply. Every retry, including
    /// a partial write, rechecks the original node and epoch rather than a fid
    /// that the client may since have clunked or reused. On error, close.
    pub fn write_output<T>(
        &self,
        export: &mut E,
        write: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, Errno> {
        if let Some(QueuedReply {
            remaining,
            delivery: Some((node, epoch)),
            ..
        }) = self.unwritten.front()
        {
            return export.with_delivery(
                &Access {
                    connection: self.id,
                    epoch: *epoch,
                    node,
                    operation: Operation::Read,
                },
                || write(&self.output[..*remaining]),
            );
        }
        let count = self
            .unwritten
            .iter()
            .take_while(|reply| reply.delivery.is_none())
            .map(|reply| reply.remaining)
            .sum::<usize>();
        Ok(write(&self.output[..count]))
    }

    /// The peer took `count` bytes of [`Self::output`]. Call
    /// [`Self::resume`] and [`Self::retry_waiting`] afterwards: requests held
    /// for want of room may now proceed.
    pub fn sent(&mut self, count: usize) {
        let mut count = count.min(self.output.len());
        self.output.drain(..count);
        while let Some(reply) = self.unwritten.front_mut() {
            if count < reply.remaining {
                reply.remaining -= count;
                break;
            }
            count -= reply.remaining;
            self.unwritten.pop_front();
        }
    }
}

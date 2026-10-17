use super::FileWire;
use crate::wire::Outbound;
use crate::{
    ContentBatchItem, MAX_QUEUED_BYTES, MAX_QUEUED_FRAMES, ShellBatchItem, ShellClientError,
};

impl FileWire {
    pub(crate) fn encode_content_batch(
        &mut self,
        items: &[ContentBatchItem<'_>],
    ) -> Result<Vec<Vec<u8>>, ShellClientError> {
        self.encode_batch_outbounds(items.len(), items.iter().map(|item| item.outbound()))
    }
    pub(crate) fn encode_shell_batch(
        &mut self,
        items: &[ShellBatchItem<'_>],
    ) -> Result<Vec<Vec<u8>>, ShellClientError> {
        self.encode_batch_outbounds(items.len(), items.iter().map(|item| item.outbound()))
    }
    fn encode_batch_outbounds(
        &mut self,
        count: usize,
        items: impl Iterator<Item = Outbound>,
    ) -> Result<Vec<Vec<u8>>, ShellClientError> {
        if count == 0 || count > MAX_QUEUED_FRAMES {
            return Err(ShellClientError::InvalidArgument("batch count"));
        }
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        self.cancel_encoded();
        let result = self.encode_batch_items(count, items);
        if result.is_err() {
            self.cancel_encoded();
        }
        result
    }

    fn encode_batch_items(
        &mut self,
        count: usize,
        items: impl Iterator<Item = Outbound>,
    ) -> Result<Vec<Vec<u8>>, ShellClientError> {
        let mut units = Vec::with_capacity(count);
        let mut bytes = 0usize;
        for item in items {
            let encoded = self.encode_unit(item)?;
            for unit in encoded {
                bytes = bytes
                    .checked_add(unit.len())
                    .ok_or(ShellClientError::QueueSaturated)?;
                if bytes > MAX_QUEUED_BYTES {
                    return Err(ShellClientError::QueueSaturated);
                }
                units.push(unit);
            }
        }
        Ok(units)
    }
}

use super::*;
use sophia_9p::journal::{Journal as Custody, JournalBounds, PreparedRecord};

#[path = "../../../../tests/support/policy_file_journal_reservation.rs"]
pub(super) mod reservation_tests;

const BOUNDS: JournalBounds = JournalBounds {
    records: WM_FILE_MAX_JOURNAL_RECORDS as usize,
    bytes: WM_FILE_MAX_BYTES,
};

/// The WM event journal: only event-class records, within one file's bytes.
pub(super) struct Journal(Custody);

/// Local reservation only; dropping it publishes nothing and spends no
/// identity.
pub(super) type PreparedEvent<'a> = PreparedRecord<'a>;

impl Journal {
    pub(super) fn new(epoch: u64) -> Self {
        Self(Custody::new(epoch))
    }

    fn header(&self, kind: WmFileKind) -> WmFileHeader {
        WmFileHeader {
            kind,
            connection_epoch: self.0.epoch(),
            submission_id: 0,
            sequence: self.0.next_sequence(),
        }
    }

    pub(super) fn append(&mut self, kind: WmFileKind, body: &[u8]) -> Result<u64, Errno> {
        if wm_file_class(kind) != WmFileClass::Event {
            return Err(Errno::EINVAL);
        }
        let size = WM_FILE_HEADER_BYTES
            .checked_add(body.len())
            .ok_or(Errno::EINVAL)?;
        if size > WM_FILE_MAX_BYTES {
            return Err(Errno::EINVAL);
        }
        if !self.0.fits(size, BOUNDS) {
            return Err(Errno::EAGAIN);
        }
        let bytes = encode_wm_file_record(self.header(kind), body).map_err(|_| Errno::EINVAL)?;
        Ok(self.0.prepare(bytes, BOUNDS)?.commit())
    }

    pub(super) fn prepare_encoded(
        &mut self,
        kind: WmFileKind,
        encode: impl FnOnce(WmFileHeader) -> Result<Vec<u8>, Errno>,
    ) -> Result<PreparedEvent<'_>, Errno> {
        if wm_file_class(kind) != WmFileClass::Event {
            return Err(Errno::EINVAL);
        }
        if self.0.position().records == BOUNDS.records {
            return Err(Errno::EAGAIN);
        }
        let header = self.header(kind);
        let bytes = encode(header)?;
        let record =
            decode_wm_file_record(&bytes, WmFileClass::Event).map_err(|_| Errno::EINVAL)?;
        if record.header != header || bytes.len() > WM_FILE_MAX_BYTES {
            return Err(Errno::EINVAL);
        }
        self.0.prepare(bytes, BOUNDS)
    }

    pub(super) fn read(&self, offset: u64, count: u32) -> Result<ReadOutcome, Errno> {
        self.0.read(offset, count)
    }

    pub(super) fn ack(&mut self, ack: WmFileAck) -> Result<(), Errno> {
        self.0.ack(ack.connection_epoch, ack.sequence).map(|_| ())
    }

    pub(super) fn size(&self) -> u64 {
        self.0.size()
    }
}

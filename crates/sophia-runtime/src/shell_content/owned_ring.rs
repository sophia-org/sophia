//! D4 measurement-only source custody. No source mapping, wire or authority.
use super::{IsbValidationFailure, OwnedRingSourceReservation};
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const HEADER: usize = 4096;
const MAX_SLOTS: usize = 4;
const MAX_PIXELS: usize = 4 << 20;

/// Fixed experimental layout: a little-endian u64 generation at each slot's
/// start, 4096 header bytes, then tightly packed pixels padded to 4096 bytes.
/// This is an internal harness layout, not an allocated public protocol.
#[derive(Clone, Copy, Debug)]
pub struct OwnedRingGeometry {
    slots: usize,
    pixels: usize,
    extent: usize,
}
impl OwnedRingGeometry {
    pub fn new(slots: usize, pixels: usize) -> io::Result<Self> {
        if !(1..=MAX_SLOTS).contains(&slots)
            || pixels == 0
            || pixels > MAX_PIXELS
            || !pixels.is_multiple_of(4)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owned ring geometry",
            ));
        }
        Ok(Self {
            slots,
            pixels,
            extent: HEADER + pixels.next_multiple_of(HEADER),
        })
    }
    pub fn source_bytes(self) -> usize {
        self.slots * self.extent
    }
    pub fn pixel_bytes(self) -> usize {
        self.pixels
    }
    pub fn slot_extent(self) -> usize {
        self.extent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedSlotIdentity {
    pub offer: u64,
    pub slot: usize,
    pub generation: u64,
}

struct PendingSlot {
    identity: OwnedSlotIdentity,
    finished: Arc<AtomicBool>,
}
#[derive(Default)]
struct Slot {
    generation: u64,
    pending: Option<PendingSlot>,
}

struct RingSource {
    // Close the fd before returning its charge (struct fields drop in order).
    file: File,
    reservation: OwnedRingSourceReservation,
    identity: Arc<()>,
}

/// In-process identity only. Retaining this token does not keep a source FD or
/// its byte charge alive, and cannot grant transport authority.
#[derive(Clone, Debug)]
pub(crate) struct OwnedRingBinding(Arc<()>);

/// One bounded source owner. Admission consumes a Session reservation for the
/// complete source extent, separate from every accepted destination.
/// All offers belong to this object; connection/ring identities are its future
/// export owner's obligation. Readable regular files can block: this experiment
/// supplies no syscall-duration bound or portable backing qualification.
pub struct OwnedRing {
    source: Arc<RingSource>,
    geometry: OwnedRingGeometry,
    slots: [Slot; MAX_SLOTS],
    greatest_offer: u64,
}

impl OwnedRing {
    pub fn admit(source: File, reservation: OwnedRingSourceReservation) -> io::Result<Self> {
        let geometry = reservation.geometry();
        // Establish field drop order before any fallible source inspection.
        let source = RingSource {
            file: source,
            reservation,
            identity: Arc::new(()),
        };
        let metadata = source.file.metadata()?;
        if !metadata.is_file() || metadata.len() != geometry.source_bytes() as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owned ring extent or reservation",
            ));
        }
        Ok(Self {
            source: Arc::new(source),
            geometry,
            slots: std::array::from_fn(|_| Slot::default()),
            greatest_offer: 0,
        })
    }

    pub fn grant(&self) -> sophia_protocol::ContentGrant {
        self.source.reservation.grant()
    }

    pub(crate) fn binding(&self) -> OwnedRingBinding {
        OwnedRingBinding(self.source.identity.clone())
    }

    pub(crate) fn geometry(&self) -> OwnedRingGeometry {
        self.geometry
    }

    pub(crate) fn is_charged_to(&self, used: &Arc<std::sync::atomic::AtomicU64>) -> bool {
        self.source.reservation.is_charged_to(used)
    }

    /// The caller must already hold destination, worker and terminal journal
    /// credits. A slot remains busy until both its read ended and its release
    /// was committed to the live journal. Failed offers do not consume identity.
    pub fn offer(&mut self, identity: OwnedSlotIdentity) -> io::Result<OwnedRingRead> {
        self.check_offer(identity)?;
        let slot = &mut self.slots[identity.slot];
        let finished = Arc::new(AtomicBool::new(false));
        slot.generation = identity.generation;
        slot.pending = Some(PendingSlot {
            identity,
            finished: finished.clone(),
        });
        self.greatest_offer = identity.offer;
        Ok(OwnedRingRead {
            source: self.source.clone(),
            geometry: self.geometry,
            identity,
            finished,
        })
    }

    pub(crate) fn check_offer(&self, identity: OwnedSlotIdentity) -> io::Result<()> {
        if identity.slot >= self.geometry.slots
            || identity.offer <= self.greatest_offer
            || identity.generation == 0
            || !identity.generation.is_multiple_of(2)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owned ring offer",
            ));
        }
        let slot = &self.slots[identity.slot];
        if slot.pending.is_some() || identity.generation <= slot.generation {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "slot busy or stale generation",
            ));
        }
        Ok(())
    }

    /// Repeated polling retains the same obligation under journal backpressure.
    /// Read completion is not pixel acceptance, presentation or ResourceReleased.
    pub fn pending_release(&self, slot: usize) -> Option<OwnedSlotIdentity> {
        let pending = self.slots.get(slot)?.pending.as_ref()?;
        pending
            .finished
            .load(Ordering::Acquire)
            .then_some(pending.identity)
    }

    /// Call only after recording this exact slot release. Never clear a pending
    /// source read because the resource was cancelled, expired or disconnected.
    pub fn confirm_release(&mut self, identity: OwnedSlotIdentity) -> bool {
        if self.pending_release(identity.slot) != Some(identity) {
            return false;
        }
        self.slots[identity.slot].pending = None;
        true
    }

    pub fn outstanding(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.pending.is_some())
            .count()
    }
}

/// Non-cloneable worker source. Dropping it proves no future reads through this
/// ticket, including refused submissions and cancellation before the first read.
/// An in-flight ticket independently retains the fd if its connection dies.
pub struct OwnedRingRead {
    source: Arc<RingSource>,
    geometry: OwnedRingGeometry,
    identity: OwnedSlotIdentity,
    finished: Arc<AtomicBool>,
}
impl Drop for OwnedRingRead {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
    }
}
impl OwnedRingRead {
    pub(crate) fn matches(&self, binding: &OwnedRingBinding, identity: OwnedSlotIdentity) -> bool {
        Arc::ptr_eq(&self.source.identity, &binding.0) && self.identity == identity
    }

    pub(crate) fn pixel_bytes(&self) -> usize {
        self.geometry.pixels
    }

    pub(crate) fn copy(
        self,
        check: impl Fn() -> Result<(), IsbValidationFailure>,
    ) -> Result<Box<[u8]>, IsbValidationFailure> {
        check()?;
        let offset = (self.identity.slot * self.geometry.extent) as u64;
        let generation = || {
            let mut bytes = [0; 8];
            if self.source.file.read_at(&mut bytes, offset).ok() != Some(bytes.len()) {
                return Err(IsbValidationFailure::SizeMismatch);
            }
            if u64::from_le_bytes(bytes) != self.identity.generation {
                return Err(IsbValidationFailure::GenerationChanged);
            }
            Ok(())
        };
        generation()?;
        check()?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.geometry.pixels)
            .map_err(|_| IsbValidationFailure::Malformed)?;
        bytes.resize(self.geometry.pixels, 0);
        // One payload read: short reads, EINTR and errors reject the offer. Never
        // stitch partial revisions or silently retry a possibly torn snapshot.
        check()?;
        if self
            .source
            .file
            .read_at(&mut bytes, offset + HEADER as u64)
            .ok()
            != Some(bytes.len())
        {
            return Err(IsbValidationFailure::SizeMismatch);
        }
        check()?;
        generation()?;
        check()?;
        // Equal producer-controlled words do not authenticate hostile pixels.
        // Only this owned result is subsequently scanned and given to consumers.
        Ok(bytes.into_boxed_slice())
    }
}

//! Local Linux mechanism probe, not a Session path or native acceptance gate.
//! Mutable mappings never expose Rust slices to consumers. Every copied result
//! owns a separate allocation; later producer writes cannot modify it.
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::FileExt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub const HEADER: usize = 4096;
pub const SLOTS: usize = 3;
pub const PANEL: usize = 1920 * 24 * 4;
pub const BULK: usize = 1024 * 1024 * 4;
const SIZE_SEALS: i32 = libc::F_SEAL_GROW | libc::F_SEAL_SHRINK;
const ALL_SEALS: i32 = SIZE_SEALS | libc::F_SEAL_WRITE | libc::F_SEAL_SEAL;

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}
pub fn memfd(bytes: usize) -> io::Result<File> {
    // SAFETY: the literal is NUL terminated and flags contain no borrowed state.
    let fd = unsafe {
        libc::memfd_create(
            c"d4-owned-ring-probe".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful memfd_create returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(bytes.try_into().map_err(|_| invalid("extent overflow"))?)?;
    // Match the existing fresh-source probe's page reservation.
    if unsafe { libc::fallocate(file.as_raw_fd(), 0, 0, bytes as i64) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}
pub fn seal(file: &File, flags: i32) -> io::Result<()> {
    // SAFETY: fcntl inspects a live file and takes an integer seal mask.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, flags) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn require_seals(file: &File, flags: i32) -> io::Result<()> {
    // SAFETY: the descriptor stays owned throughout fcntl.
    let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
    if seals < 0 {
        return Err(io::Error::last_os_error());
    }
    if seals & flags != flags {
        return Err(invalid("required seals absent"));
    }
    Ok(())
}
struct Mapping {
    address: NonNull<u8>,
    bytes: usize,
}
impl Mapping {
    // Callers establish extent stability before exposing any mapped access.
    fn new(file: &File, bytes: usize, writable: bool) -> io::Result<Self> {
        if bytes == 0 || bytes > 4 * (BULK + HEADER) {
            return Err(invalid("mapping extent"));
        }
        let protection = libc::PROT_READ | if writable { libc::PROT_WRITE } else { 0 };
        // SAFETY: mapping lifetime is owned here; no borrowed references escape.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                protection,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let Some(address) = NonNull::new(raw.cast()) else {
            unsafe {
                libc::munmap(raw, bytes);
            }
            return Err(invalid("null mapping"));
        };
        Ok(Self { address, bytes })
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: exact owned address/extent, no escaping references.
        unsafe {
            libc::munmap(self.address.as_ptr().cast(), self.bytes);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Mapped,
    Pread,
}
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub slots: usize,
    pub pixel_bytes: usize,
}
impl Geometry {
    pub fn slot_extent(self) -> io::Result<usize> {
        if !(1..=4).contains(&self.slots)
            || self.pixel_bytes == 0
            || self.pixel_bytes > BULK
            || !self.pixel_bytes.is_multiple_of(4)
        {
            return Err(invalid("ring geometry"));
        }
        Ok(HEADER + self.pixel_bytes.next_multiple_of(HEADER))
    }
    pub fn total(self) -> io::Result<usize> {
        Ok(self.slot_extent()? * self.slots)
    }
    fn offset(self, slot: usize) -> io::Result<usize> {
        if slot >= self.slots {
            return Err(invalid("slot index"));
        }
        Ok(self.slot_extent()? * slot)
    }
}
/// Test producer. Only this side ever has a writable mapping. For the unsealed
/// variant, tests must not touch that mapping after deliberately truncating it.
pub struct Producer {
    file: File,
    mapping: Mapping,
    geometry: Geometry,
}
impl Producer {
    pub fn new(geometry: Geometry, size_sealed: bool) -> io::Result<Self> {
        let bytes = geometry.total()?;
        let file = memfd(bytes)?;
        if size_sealed {
            seal(&file, SIZE_SEALS)?;
        }
        let mapping = Mapping::new(&file, bytes, true)?;
        Ok(Self {
            file,
            mapping,
            geometry,
        })
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn publish(&mut self, slot: usize, generation: u64, seed: u8) -> io::Result<()> {
        if generation == 0 || !generation.is_multiple_of(2) {
            return Err(invalid("generation must be nonzero and even"));
        }
        let offset = self.geometry.offset(slot)?;
        // SAFETY: slot bounds validated; page-aligned AtomicU64 header; only
        // atomic accesses reach the generation word. Probe calls are serialized.
        let word = unsafe { AtomicU64::from_ptr(self.mapping.address.as_ptr().add(offset).cast()) };
        word.store(generation - 1, Ordering::Release);
        // SAFETY: producer exclusively writes these pixels during its turn.
        // Consumer APIs never lend a Rust reference into this mutable source.
        let pixels = unsafe {
            std::slice::from_raw_parts_mut(
                self.mapping.address.as_ptr().add(offset + HEADER),
                self.geometry.pixel_bytes,
            )
        };
        fill(pixels, seed);
        word.store(generation, Ordering::Release);
        Ok(())
    }
}

/// Immutable geometry is copied at admission. The consumer never consults
/// producer-controlled dimensions, offsets or lengths after this point.
pub struct Source {
    file: File,
    mapped: Option<Mapping>,
    geometry: Geometry,
}
impl Source {
    pub fn admit(file: File, geometry: Geometry, method: Method) -> io::Result<Self> {
        let total = geometry.total()?;
        if method == Method::Mapped {
            require_seals(&file, SIZE_SEALS)?;
        }
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != total as u64 {
            return Err(invalid("ring file extent/type"));
        }
        let mapped = if method == Method::Mapped {
            Some(Mapping::new(&file, total, false)?)
        } else {
            None
        };
        Ok(Self {
            file,
            mapped,
            geometry,
        })
    }
    fn generation(&self, offset: usize) -> io::Result<u64> {
        if let Some(map) = &self.mapped {
            // SAFETY: size-sealed extent, checked page-aligned header; atomic
            // interior mutability, never an ordinary shared pixel slice.
            Ok(
                unsafe { AtomicU64::from_ptr(map.address.as_ptr().add(offset).cast()) }
                    .load(Ordering::Acquire),
            )
        } else {
            let mut bytes = [0; 8];
            read_once(&self.file, &mut bytes, offset as u64)?;
            Ok(u64::from_ne_bytes(bytes))
        }
    }
    /// Hook is only for deterministic fault injection in this standalone probe.
    /// Generation checks detect conforming publication races, not malicious
    /// rewrites which leave metadata unchanged. Copy bounds never trust pixels.
    pub fn capture(
        &self,
        slot: usize,
        generation: u64,
        after_copy: impl FnOnce(),
    ) -> io::Result<(OwnedResource, Stages)> {
        let offset = self.geometry.offset(slot)?;
        if generation == 0 || !generation.is_multiple_of(2) {
            return Err(invalid("invalid offer generation"));
        }
        let mut stages = Stages::default();
        let start = Instant::now();
        if self.generation(offset)? != generation {
            return Err(invalid("stale or in-progress generation"));
        }
        stages.generation_ns += elapsed(start);
        let start = Instant::now();
        let mut bytes = vec![0; self.geometry.pixel_bytes].into_boxed_slice();
        stages.allocate_ns = elapsed(start);
        let start = Instant::now();
        if let Some(map) = &self.mapped {
            // SAFETY: source extent cannot shrink; validated offset/length;
            // destination uniquely owns initialized storage and cannot alias
            // source. Raw copy creates no immutable borrow of mutable pixels.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    map.address.as_ptr().add(offset + HEADER),
                    bytes.as_mut_ptr(),
                    bytes.len(),
                );
            }
        } else {
            read_once(&self.file, &mut bytes, (offset + HEADER) as u64)?;
        }
        stages.copy_ns = elapsed(start);
        after_copy();
        let start = Instant::now();
        if self.generation(offset)? != generation {
            return Err(invalid("generation changed during copy"));
        }
        stages.generation_ns += elapsed(start);
        let start = Instant::now();
        if !valid_pixels(&bytes) {
            return Err(invalid("non-premultiplied owned pixels"));
        }
        stages.scan_ns = elapsed(start);
        Ok((OwnedResource(bytes), stages))
    }
}
fn read_once(file: &File, bytes: &mut [u8], offset: u64) -> io::Result<()> {
    // Deliberately no read_exact_at loop: a partial read cannot be stitched
    // together across producer revisions. EINTR/error is a refused offer too.
    if file.read_at(bytes, offset)? != bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short ring read",
        ));
    }
    Ok(())
}
pub struct OwnedResource(Box<[u8]>);
impl OwnedResource {
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}
#[derive(Default)]
pub struct Stages {
    pub allocate_ns: u64,
    pub generation_ns: u64,
    pub copy_ns: u64,
    pub scan_ns: u64,
}
pub fn elapsed(start: Instant) -> u64 {
    start.elapsed().as_nanos() as u64
}
pub fn fill(bytes: &mut [u8], seed: u8) {
    for (i, pixel) in bytes.chunks_exact_mut(4).enumerate() {
        let v = (i as u8).wrapping_add(seed);
        pixel.copy_from_slice(&[
            v & 127,
            v.wrapping_mul(3) & 127,
            v.wrapping_mul(5) & 127,
            255,
        ]);
    }
}
pub fn valid_pixels(bytes: &[u8]) -> bool {
    bytes.chunks(65536).all(|unit| {
        unit.chunks_exact(4)
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
    })
}

pub struct FreshResource {
    mapping: Mapping,
    _file: File,
}
impl FreshResource {
    pub fn produce(bytes: usize, seed: u8) -> io::Result<File> {
        let file = memfd(bytes)?;
        seal(&file, SIZE_SEALS)?;
        let map = Mapping::new(&file, bytes, true)?;
        // SAFETY: this fresh file has no other mapping or borrower.
        fill(
            unsafe { std::slice::from_raw_parts_mut(map.address.as_ptr(), bytes) },
            seed,
        );
        drop(map);
        seal(&file, libc::F_SEAL_WRITE | libc::F_SEAL_SEAL)?;
        Ok(file)
    }
    pub fn accept(file: File, bytes: usize) -> io::Result<Self> {
        require_seals(&file, ALL_SEALS)?;
        if file.metadata()?.len() != bytes as u64 {
            return Err(invalid("fresh extent"));
        }
        let mapping = Mapping::new(&file, bytes, false)?;
        let result = Self {
            mapping,
            _file: file,
        };
        if !valid_pixels(result.bytes()) {
            return Err(invalid("fresh pixels"));
        }
        Ok(result)
    }
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: all four seals proved before mapping; immutable exact extent.
        unsafe { std::slice::from_raw_parts(self.mapping.address.as_ptr(), self.mapping.bytes) }
    }
}

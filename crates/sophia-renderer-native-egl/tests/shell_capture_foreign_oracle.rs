//! Two-process FOREIGN control for the native shell capture path.
//!
//! The oracle (this test) receives a real Vulkan-exported modifier-0
//! DMA-BUF from a separate producer process over a seqpacket socket,
//! prepares and reserves through the real GBM pool, captures with
//! `begin_shell_capture` and no frame submission, then pixel-compares
//! the completed snapshot and proves it is independent of the source.
//! This establishes Vulkan FOREIGN release -> Sophia EGL import
//! only; Session presentation and performance remain open.
//!
//! Wire (fixture protocol v1, owned by the integration driver):
//!   FRAME, producer -> oracle, 52 bytes LE plus one SCM_RIGHTS fd:
//!     "SDMA", version 1, seq, width, height, offset, stride,
//!     format XR24, pattern (= seq), u64 modifier 0, u64 source_bytes.
//!   DONE, oracle -> producer, 16 bytes LE: "SACK", version 1, seq,
//!     status (0 complete and pixels match, 1 complete but mismatch,
//!     2 refused before use, 3 uncertain/timeout).
//!   FRAME 1, DONE 1, FRAME 2 (new allocation), DONE 2, exit. The
//!   producer keeps frame 1 alive until the end; after DONE 2 the
//!   oracle re-checks capture 1 still equals pattern 1. Each DONE
//!   wait is 10 s.
//!
//! Patterns are 256x64 opaque, split at x = 128: pattern 1 is left
//! red (255, 0, 0), right blue (0, 0, 255); pattern 2 is left green
//! (0, 255, 0), right white (255, 255, 255). XR24 memory is
//! B, G, R, X; X is undefined and ignored. Comparisons sample away
//! from the split edge.
#![cfg(all(
    feature = "gbm-platform",
    feature = "experimental-shell-capture",
    target_os = "linux"
))]

use sophia_renderer_native_egl::{
    NativeDmaBufPlane, NativeGbmRenderedScanoutContext, NativeMultiPlaneDmaBufFrame,
    NativePixmapImportProbe, NativeRendererImageId, NativeRendererSnapshotBudget,
    NativeRendererSnapshotEpoch, NativeShellCapturePoll, NativeShellCaptureRequest,
    NativeShellCaptureSource, NativeShellCaptureStart,
};
use std::{
    fs::{File, OpenOptions},
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const BY_PATH_RENDER: &str = "/dev/dri/by-path/pci-0000:03:00.0-render";

// Minimal seqpacket + SCM_RIGHTS plumbing (the crate allows unsafe in
// tests; std has no seqpacket or fd-passing API).
mod wire {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::{Path, PathBuf};

    #[repr(C)]
    struct Iovec {
        base: *mut core::ffi::c_void,
        len: usize,
    }
    #[repr(C)]
    struct Msghdr {
        name: *mut core::ffi::c_void,
        namelen: u32,
        iov: *mut Iovec,
        iovlen: usize,
        control: *mut core::ffi::c_void,
        controllen: usize,
        flags: i32,
    }
    #[repr(C)]
    struct Cmsghdr {
        len: usize,
        level: i32,
        ty: i32,
    }
    #[repr(C)]
    struct Timeval {
        sec: i64,
        usec: i64,
    }

    unsafe extern "C" {
        fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        fn bind(fd: i32, addr: *const SockaddrUn, len: u32) -> i32;
        fn listen(fd: i32, backlog: i32) -> i32;
        fn accept4(fd: i32, addr: *mut core::ffi::c_void, len: *mut u32, flags: i32) -> i32;
        fn sendmsg(fd: i32, msg: *const Msghdr, flags: i32) -> isize;
        fn recvmsg(fd: i32, msg: *mut Msghdr, flags: i32) -> isize;
        fn setsockopt(fd: i32, level: i32, name: i32, value: *const Timeval, len: u32) -> i32;
        fn unlink(path: *const u8) -> i32;
    }

    #[repr(C)]
    struct SockaddrUn {
        family: u16,
        path: [u8; 108],
    }

    const AF_UNIX: i32 = 1;
    const SOCK_SEQPACKET: i32 = 5;
    const SOCK_CLOEXEC: i32 = 0o2000000;
    const SOL_SOCKET: i32 = 1;
    const SCM_RIGHTS: i32 = 1;
    const SO_RCVTIMEO: i32 = 20;
    const SO_SNDTIMEO: i32 = 21;

    fn sockaddr(path: &Path) -> (SockaddrUn, u32) {
        let bytes = path.as_os_str().as_encoded_bytes();
        assert!(bytes.len() < 108);
        let mut addr = SockaddrUn {
            family: AF_UNIX as u16,
            path: [0; 108],
        };
        addr.path[..bytes.len()].copy_from_slice(bytes);
        (addr, (2 + bytes.len() + 1) as u32)
    }

    fn timed(fd: i32) {
        let tv = Timeval { sec: 10, usec: 0 };
        unsafe {
            setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, 16);
            setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, 16);
        }
    }

    pub struct Listener(OwnedFd, PathBuf);

    pub fn listen_at(path: &Path) -> std::io::Result<Listener> {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        unsafe { unlink(c.as_ptr().cast()) };
        let fd = unsafe { socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let listener = Listener(unsafe { OwnedFd::from_raw_fd(fd) }, path.to_path_buf());
        let (addr, len) = sockaddr(path);
        if unsafe { bind(fd, &addr, len) } != 0 || unsafe { listen(fd, 1) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        timed(fd);
        Ok(listener)
    }

    impl Listener {
        pub fn accept(&self) -> std::io::Result<Channel> {
            let fd = unsafe {
                accept4(
                    self.0.as_raw_fd(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    SOCK_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            timed(fd);
            Ok(Channel(unsafe { OwnedFd::from_raw_fd(fd) }))
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let c = std::ffi::CString::new(self.1.as_os_str().as_encoded_bytes()).unwrap();
            unsafe { unlink(c.as_ptr().cast()) };
        }
    }

    pub struct Channel(OwnedFd);

    impl Channel {
        pub fn send(&self, bytes: &[u8], fd: Option<&OwnedFd>) -> std::io::Result<()> {
            let mut iov = Iovec {
                base: bytes.as_ptr().cast_mut().cast(),
                len: bytes.len(),
            };
            let mut control = [0u8; 32];
            let (control_ptr, control_len) = if let Some(fd) = fd {
                let cmsg = Cmsghdr {
                    len: 16 + 4,
                    level: SOL_SOCKET,
                    ty: SCM_RIGHTS,
                };
                control[..16].copy_from_slice(unsafe {
                    core::slice::from_raw_parts((&cmsg as *const Cmsghdr).cast::<u8>(), 16)
                });
                control[16..20].copy_from_slice(&fd.as_raw_fd().to_ne_bytes());
                (control.as_mut_ptr().cast(), 20usize)
            } else {
                (core::ptr::null_mut(), 0)
            };
            let msg = Msghdr {
                name: core::ptr::null_mut(),
                namelen: 0,
                iov: &mut iov,
                iovlen: 1,
                control: control_ptr,
                controllen: control_len,
                flags: 0,
            };
            if unsafe { sendmsg(self.0.as_raw_fd(), &msg, 0) } != bytes.len() as isize {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn recv(&self, bytes: &mut [u8]) -> std::io::Result<(usize, Option<OwnedFd>)> {
            let mut iov = Iovec {
                base: bytes.as_mut_ptr().cast(),
                len: bytes.len(),
            };
            let mut control = [0u8; 32];
            let mut msg = Msghdr {
                name: core::ptr::null_mut(),
                namelen: 0,
                iov: &mut iov,
                iovlen: 1,
                control: control.as_mut_ptr().cast(),
                controllen: control.len(),
                flags: 0,
            };
            let got = unsafe { recvmsg(self.0.as_raw_fd(), &mut msg, 0) };
            if got < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut received = None;
            if msg.controllen >= 20 {
                let level = i32::from_ne_bytes(control[8..12].try_into().unwrap());
                let ty = i32::from_ne_bytes(control[12..16].try_into().unwrap());
                if level == SOL_SOCKET && ty == SCM_RIGHTS {
                    let raw = i32::from_ne_bytes(control[16..20].try_into().unwrap());
                    received = Some(unsafe { OwnedFd::from_raw_fd(raw) });
                }
            }
            Ok((got as usize, received))
        }
    }
}

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

const PATTERNS: [[(u8, u8, u8); 2]; 2] =
    [[(255, 0, 0), (0, 0, 255)], [(0, 255, 0), (255, 255, 255)]];

/// Every pixel except the two columns at the split edge must equal the
/// pattern half it belongs to; alpha must read back opaque.
fn matches_pattern(rgba: &[u8], width: u32, height: u32, seq: u32) -> bool {
    let [left, right] = PATTERNS[(seq - 1) as usize];
    if rgba.len() != (width * height * 4) as usize {
        return false;
    }
    for y in 0..height {
        for x in 0..width {
            if (127..=128).contains(&x) {
                continue;
            }
            let (r, g, b) = if x < 128 { left } else { right };
            let at = ((y * width + x) * 4) as usize;
            if rgba[at..at + 4] != [r, g, b, 255] {
                return false;
            }
        }
    }
    true
}

struct Frame {
    seq: u32,
    width: u32,
    height: u32,
    offset: u32,
    stride: u32,
    fd: OwnedFd,
}

fn recv_frame(channel: &wire::Channel) -> Frame {
    let mut header = [0u8; 52];
    let (got, fd) = channel.recv(&mut header).unwrap();
    assert_eq!(got, 52);
    assert_eq!(&header[0..4], b"SDMA");
    let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let u64_at = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
    assert_eq!(u32_at(4), 1, "fixture version");
    let frame = Frame {
        seq: u32_at(8),
        width: u32_at(12),
        height: u32_at(16),
        offset: u32_at(20),
        stride: u32_at(24),
        fd: fd.expect("exactly one source fd"),
    };
    assert_eq!(u32_at(28), 0x3432_5258, "XR24");
    assert_eq!(u32_at(32), frame.seq, "pattern equals seq");
    assert_eq!(u64_at(36), 0, "LINEAR modifier");
    let size = u64_at(44);
    assert!(frame.width > 0 && frame.height > 0 && frame.stride >= frame.width * 4);
    assert!(
        size >= u64::from(frame.stride) * u64::from(frame.height - 1)
            + u64::from(frame.offset)
            + u64::from(frame.width) * 4,
        "source_bytes covers the declared plane"
    );
    frame
}

fn send_done(channel: &wire::Channel, seq: u32, status: u32) {
    let mut done = [0u8; 16];
    done[0..4].copy_from_slice(b"SACK");
    done[4..8].copy_from_slice(&1u32.to_le_bytes());
    done[8..12].copy_from_slice(&seq.to_le_bytes());
    done[12..16].copy_from_slice(&status.to_le_bytes());
    channel.send(&done, None).unwrap();
}

/// Listen-only oracle: an external integration driver owns the
/// producer process entirely (launch, sandbox domain, lifetime).
/// This test knows only the fixture protocol and the capture path;
/// it names no client, producer path or build system.
#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE and SOPHIA_TEST_FOREIGN_SOCKET; externally driven"]
fn vulkan_foreign_frames_capture_exactly_and_stay_independent() {
    let path = PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE")
            .expect("SOPHIA_TEST_RENDER_NODE must name the granted render node"),
    );
    assert_eq!(
        std::fs::canonicalize(&path).unwrap(),
        std::fs::canonicalize(BY_PATH_RENDER).unwrap(),
        "render node must resolve to the pci-0000:03:00.0 display GPU"
    );
    let socket = PathBuf::from(
        std::env::var_os("SOPHIA_TEST_FOREIGN_SOCKET")
            .expect("SOPHIA_TEST_FOREIGN_SOCKET must name the bound socket path"),
    );
    let listener = wire::listen_at(&socket).unwrap();
    let channel = listener.accept().unwrap();

    let mut context = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(&path)))
        .context
        .expect("render context");
    context.set_snapshot_reuse_enabled(true);
    let facts = context.shell_capture_facts().expect("capture facts");
    let device_rdev = rustix::fs::fstat(open(&path).as_fd()).unwrap().st_rdev;
    assert_eq!(facts.device, device_rdev);
    let budget = NativeRendererSnapshotBudget::new(4, 1 << 24);
    let epoch = NativeRendererSnapshotEpoch::new(device_rdev, 1, 1);

    let capture = |context: &mut NativeGbmRenderedScanoutContext<File>,
                   frame: Frame,
                   id: u64|
     -> (
        u32,
        std::sync::Arc<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
    ) {
        let bound = context
            .prepare_shell_capture_destination(frame.width, frame.height)
            .unwrap()
            .expect("prepared destination bound");
        assert!(bound >= u64::from(frame.width) * u64::from(frame.height) * 4);
        let start = context.begin_shell_capture(NativeShellCaptureRequest {
            source: NativeShellCaptureSource {
                fd: frame.fd,
                width: frame.width,
                height: frame.height,
                modifier: 0,
                offset: frame.offset,
                stride: frame.stride,
            },
            image_id: NativeRendererImageId::from_raw(id),
            reservation: budget.reserve_capture(bound).unwrap(),
            budget: budget.clone(),
            epoch: epoch.clone(),
        });
        let mut pending = match start {
            NativeShellCaptureStart::Pending(pending) => pending,
            NativeShellCaptureStart::Refused { detail, .. } => {
                panic!("seq {} refused before use: {detail:?}", frame.seq)
            }
            NativeShellCaptureStart::Uncertain(detail) => {
                panic!("seq {} uncertain: {detail:?}", frame.seq)
            }
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let completion = loop {
            match context.poll_shell_capture(pending) {
                NativeShellCapturePoll::Pending(returned) => {
                    assert!(Instant::now() < deadline, "seq {} timed out", frame.seq);
                    pending = returned;
                    std::thread::sleep(Duration::from_millis(1));
                }
                NativeShellCapturePoll::Completed(completion) => break completion,
                NativeShellCapturePoll::Uncertain(detail) => {
                    panic!("seq {} uncertain: {detail:?}", frame.seq)
                }
            }
        };
        let (_source, result) = completion.into_parts();
        let captured = result.expect("snapshot retained");
        let sophia_renderer_native_egl::NativeCapturedImage::Exported(snapshot) = captured.as_ref()
        else {
            panic!("exported control returned managed storage");
        };
        (frame.seq, snapshot.clone())
    };

    let read = |snapshot: &sophia_renderer_native_egl::NativeRendererImageSnapshot| {
        let frame = snapshot.as_frame();
        NativePixmapImportProbe::new(
            open(&path),
            NativeMultiPlaneDmaBufFrame {
                width: frame.width,
                height: frame.height,
                format: frame.format,
                modifier: frame.modifier,
                plane_count: frame.plane_count,
                planes: std::array::from_fn(|i| {
                    frame.planes[i].as_ref().map(|plane| NativeDmaBufPlane {
                        fd: plane.fd.as_fd(),
                        offset: plane.offset,
                        stride: plane.stride,
                    })
                }),
            },
        )
        .unwrap()
        .read_rgba()
        .unwrap()
    };

    let (seq1, snapshot1) = capture(&mut context, recv_frame(&channel), 0x8000_0000_0001);
    assert_eq!(seq1, 1);
    let ok1 = matches_pattern(&read(&snapshot1), 256, 64, 1);
    send_done(&channel, 1, if ok1 { 0 } else { 1 });
    assert!(ok1, "capture 1 pixels do not match pattern 1");

    let (seq2, snapshot2) = capture(&mut context, recv_frame(&channel), 0x8000_0000_0002);
    assert_eq!(seq2, 2);
    let ok2 = matches_pattern(&read(&snapshot2), 256, 64, 2);
    send_done(&channel, 2, if ok2 { 0 } else { 1 });
    assert!(ok2, "capture 2 pixels do not match pattern 2");

    // Frame 2 was rendered after capture 1 completed; capture 1 must
    // still hold pattern 1 exactly.
    assert!(
        matches_pattern(&read(&snapshot1), 256, 64, 1),
        "capture 1 changed after the producer rendered frame 2"
    );

    // The external driver observes the producer exit; the oracle's
    // verdict is already carried by the two DONE statuses above.
}

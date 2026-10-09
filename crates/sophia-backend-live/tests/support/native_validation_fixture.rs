//! Raw KMS handles for crates that test complete validation heads without a
//! DRM dependency of their own.

use crate::prelude::*;

pub fn selection(connector: u32, crtc: u32, plane: u32) -> LibdrmNativePrimaryPlaneSelection {
    LibdrmNativePrimaryPlaneSelection::new(
        drm::control::from_u32(connector).expect("connector handle is nonzero"),
        drm::control::from_u32(crtc).expect("crtc handle is nonzero"),
        drm::control::from_u32(plane).expect("plane handle is nonzero"),
        Size {
            width: 1920,
            height: 1080,
        },
        None,
    )
}

pub fn primary_plane_properties() -> LibdrmNativePrimaryPlanePropertyHandles {
    let handle = |raw| drm::control::from_u32(raw).expect("property handle is nonzero");
    LibdrmNativePrimaryPlanePropertyHandles::new(
        handle(101),
        handle(102),
        handle(103),
        handle(104),
        handle(105),
        handle(106),
        handle(107),
        handle(108),
        handle(109),
        handle(110),
        handle(111),
        handle(112),
        handle(113),
    )
}

pub fn framebuffer(raw: u32) -> drm::control::framebuffer::Handle {
    drm::control::from_u32(raw).expect("framebuffer handle is nonzero")
}

/// Connector, CRTC, plane and framebuffer handles, mode blob and size of a head.
pub fn head_objects(head: &LibdrmNativeAtomicHead) -> (u32, u32, u32, u32, Option<u64>, Size) {
    let objects = head.objects;
    (
        objects.connector.into(),
        objects.crtc.into(),
        objects.plane.into(),
        objects.framebuffer.into(),
        objects.mode_blob,
        objects.size,
    )
}

/// A commit device with amdgpu's primary-plane rule and no previous scanout:
/// a request that activates a CRTC (property 103) without naming the primary
/// plane's framebuffer (property 104) is refused with `EINVAL`. A fixed
/// `errno` refuses every request instead. Requests are counted with their flags.
#[derive(Debug, Default)]
pub struct PlaneRequiredCommitDevice {
    pub errno: Option<i32>,
    pub submissions: std::cell::RefCell<Vec<(bool, bool)>>,
}

impl LibdrmNativeAtomicCommitDevice for PlaneRequiredCommitDevice {
    fn submit_atomic_commit(
        &self,
        flags: drm::control::AtomicCommitFlags,
        request: drm::control::atomic::AtomicModeReq,
    ) -> io::Result<()> {
        self.submissions.borrow_mut().push((
            flags.contains(drm::control::AtomicCommitFlags::TEST_ONLY),
            flags.contains(drm::control::AtomicCommitFlags::PAGE_FLIP_EVENT),
        ));
        if let Some(errno) = self.errno {
            return Err(io::Error::from_raw_os_error(errno));
        }
        let request = format!("{request:?}");
        if request.contains("property::Handle(103)") && !request.contains("property::Handle(104)") {
            return Err(io::Error::from_raw_os_error(22));
        }
        Ok(())
    }
}

// Complete topology validation (t306 bare metal). A replacement owner after a
// physical return found every plane-less TEST refused: amdgpu refuses an
// enabled CRTC whose primary plane is off (amdgpu_dm_crtc.c:688-691, EINVAL),
// and removing the previous owner's framebuffers had unbound that plane.

mod complete_topology_validation {
    use super::*;
    use sophia_backend_live::{
        LibdrmNativeValidationBufferDevice, LibdrmNativeValidationFramebuffer,
        NativeTopologyValidation, validate_native_complete_topology_on_device,
        validate_native_multi_head_topology_on_device,
    };
    use std::cell::RefCell;

    const EINVAL: i32 = 22;

    /// The amdgpu rule, with no previous scanout: a request that activates a
    /// CRTC without naming its primary plane's framebuffer is refused.
    #[derive(Default)]
    struct PlaneRequiredDevice {
        flags: RefCell<Vec<drm::control::AtomicCommitFlags>>,
        errno: Option<i32>,
    }

    impl LibdrmNativeAtomicCommitDevice for PlaneRequiredDevice {
        fn submit_atomic_commit(
            &self,
            flags: drm::control::AtomicCommitFlags,
            request: drm::control::atomic::AtomicModeReq,
        ) -> io::Result<()> {
            self.flags.borrow_mut().push(flags);
            if let Some(errno) = self.errno {
                return Err(io::Error::from_raw_os_error(errno));
            }
            let request = format!("{request:?}");
            let activates = request.contains("property::Handle(103)");
            let names_plane = request.contains("property::Handle(104)");
            if activates && !names_plane {
                return Err(io::Error::from_raw_os_error(EINVAL));
            }
            Ok(())
        }
    }

    fn plane_less_head() -> LibdrmNativeAtomicTopologyHead {
        LibdrmNativeAtomicTopologyHead::new(
            drm::control::from_u32(21).unwrap(),
            drm::control::from_u32(41).unwrap(),
            15,
            primary_plane_properties(),
        )
    }

    fn complete_head() -> LibdrmNativeAtomicHead {
        head(21, 41, 51, 61, scanout_size(1920, 1080))
    }

    #[test]
    fn a_plane_less_test_is_refused_but_a_complete_one_is_accepted_without_prior_scanout() {
        let device = PlaneRequiredDevice::default();
        assert_eq!(
            validate_native_multi_head_topology_on_device(&device, &[plane_less_head()]),
            NativeTopologySubmitOutcome::Rejected
        );
        assert_eq!(
            validate_native_complete_topology_on_device(&device, &[complete_head()]),
            NativeTopologyValidation {
                outcome: NativeTopologySubmitOutcome::Accepted,
                errno: 0,
            }
        );
        // The complete test never applies and never asks for an event.
        let flags = device.flags.borrow();
        let complete = flags[1];
        assert!(complete.contains(drm::control::AtomicCommitFlags::TEST_ONLY));
        assert!(complete.contains(drm::control::AtomicCommitFlags::ALLOW_MODESET));
        assert!(!complete.contains(drm::control::AtomicCommitFlags::PAGE_FLIP_EVENT));
    }

    #[test]
    fn the_kernel_errno_survives_and_eagain_and_ebusy_are_busy() {
        for (errno, outcome) in [
            (EINVAL, NativeTopologySubmitOutcome::Rejected),
            (1, NativeTopologySubmitOutcome::Rejected),
            (11, NativeTopologySubmitOutcome::Busy),
            (16, NativeTopologySubmitOutcome::Busy),
        ] {
            let device = PlaneRequiredDevice {
                errno: Some(errno),
                ..PlaneRequiredDevice::default()
            };
            assert_eq!(
                validate_native_complete_topology_on_device(&device, &[complete_head()]),
                NativeTopologyValidation { outcome, errno },
                "errno {errno}"
            );
        }
    }

    #[test]
    fn an_unbuildable_complete_request_never_reaches_the_device() {
        let device = PlaneRequiredDevice::default();
        let validation = validate_native_complete_topology_on_device(&device, &[]);
        assert!(matches!(
            validation.outcome,
            NativeTopologySubmitOutcome::Unbuildable(_)
        ));
        assert_eq!(validation.errno, 0);
        assert!(device.flags.borrow().is_empty());
    }

    /// Records every buffer and framebuffer call, failing where told.
    #[derive(Default)]
    struct BufferDevice {
        calls: RefCell<Vec<String>>,
        fail_add: bool,
        fail_destroy_framebuffer: bool,
    }

    impl LibdrmNativeValidationBufferDevice for BufferDevice {
        type Buffer = (u32, u32);

        fn create_validation_buffer(&self, width: u32, height: u32) -> io::Result<Self::Buffer> {
            self.calls.borrow_mut().push(format!("create {width}x{height}"));
            Ok((width, height))
        }

        fn add_validation_framebuffer(
            &self,
            buffer: &Self::Buffer,
        ) -> io::Result<drm::control::framebuffer::Handle> {
            self.calls.borrow_mut().push(format!("add {}x{}", buffer.0, buffer.1));
            if self.fail_add {
                return Err(io::Error::from_raw_os_error(EINVAL));
            }
            Ok(drm::control::from_u32(77).unwrap())
        }

        fn destroy_validation_framebuffer(
            &self,
            _framebuffer: drm::control::framebuffer::Handle,
        ) -> io::Result<()> {
            self.calls.borrow_mut().push("rmfb".into());
            if self.fail_destroy_framebuffer {
                return Err(io::Error::from_raw_os_error(2));
            }
            Ok(())
        }

        fn destroy_validation_buffer(&self, buffer: Self::Buffer) -> io::Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("destroy {}x{}", buffer.0, buffer.1));
            Ok(())
        }
    }

    #[test]
    fn a_framebuffer_that_cannot_be_added_leaves_no_buffer() {
        let card = BufferDevice {
            fail_add: true,
            ..BufferDevice::default()
        };
        let error = LibdrmNativeValidationFramebuffer::allocate(&card, 2560, 1440).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(EINVAL));
        assert_eq!(
            *card.calls.borrow(),
            ["create 2560x1440", "add 2560x1440", "destroy 2560x1440"]
        );
    }

    #[test]
    fn release_removes_the_framebuffer_then_the_buffer_and_reports_the_first_failure() {
        let card = BufferDevice {
            fail_destroy_framebuffer: true,
            ..BufferDevice::default()
        };
        let framebuffer = LibdrmNativeValidationFramebuffer::allocate(&card, 1920, 1080).unwrap();
        assert_eq!(framebuffer.size(), scanout_size(1920, 1080));
        let error = framebuffer.release(&card).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(2));
        assert_eq!(
            *card.calls.borrow(),
            ["create 1920x1080", "add 1920x1080", "rmfb", "destroy 1920x1080"]
        );
    }
}

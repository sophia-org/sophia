#![cfg(feature = "native-session")]
//! Complete validation heads (t306 bare metal). A physical return refused every
//! plane-less TEST on amdgpu, which will not enable a CRTC whose primary plane
//! is off. Each head now names a framebuffer allocated for the test, on the card
//! of that exact head, and every buffer and blob returns to that card.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::rc::Rc;

use sophia_backend_live::native_validation_fixture::{
    PlaneRequiredCommitDevice, framebuffer, head_objects, primary_plane_properties, selection,
};
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeValidationBufferDevice,
    LibdrmNativeValidationFramebuffer, LibdrmNativeVrrPropertyDiscoveryStatus,
};
use sophia_config::{ConfigDigest, ConfigGeneration, DesktopOutputCandidate};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{OutputId, Size};
use sophia_session::desktop_output_activation::run_native_output_activation;
use sophia_session::desktop_output_commit::NativeOutputTopologyValidationExecutor;
use sophia_session::desktop_output_heads::{
    NativeOutputComposedHead, NativeOutputHeadResolveError, NativeOutputHeadUnavailable,
    NativeOutputTopologyHardware, NativeOutputValidationHead, NativeOutputValidationResources,
    compose_native_output_validation_head, release_native_output_validation_head,
    resolve_native_output_topology_heads,
};
use sophia_session::desktop_output_topology::{
    NativeOutputActivationPlan, prepare_native_output_activation_plan,
    project_native_output_topology,
};

type Log = Rc<RefCell<Vec<String>>>;

/// One card's buffer calls, written to a log shared by every card.
struct FakeCard {
    name: &'static str,
    log: Log,
    fail_allocation: bool,
}

impl LibdrmNativeValidationBufferDevice for FakeCard {
    type Buffer = (&'static str, u32);

    fn create_validation_buffer(&self, width: u32, height: u32) -> io::Result<Self::Buffer> {
        self.log
            .borrow_mut()
            .push(format!("{} create {width}x{height}", self.name));
        if self.fail_allocation {
            return Err(io::Error::from_raw_os_error(12));
        }
        Ok((self.name, width))
    }

    fn add_validation_framebuffer(
        &self,
        buffer: &Self::Buffer,
    ) -> io::Result<sophia_backend_live::LibdrmNativeFramebufferHandle> {
        self.log.borrow_mut().push(format!("{} addfb", buffer.0));
        Ok(framebuffer(70))
    }

    fn destroy_validation_framebuffer(
        &self,
        _framebuffer: sophia_backend_live::LibdrmNativeFramebufferHandle,
    ) -> io::Result<()> {
        self.log.borrow_mut().push(format!("{} rmfb", self.name));
        Ok(())
    }

    fn destroy_validation_buffer(&self, buffer: Self::Buffer) -> io::Result<()> {
        assert_eq!(
            buffer.0, self.name,
            "a buffer returns to the card that made it"
        );
        self.log.borrow_mut().push(format!("{} destroy", self.name));
        Ok(())
    }
}

/// Connectors mapped to head indices, and head indices to cards.
struct FakeValidationHardware {
    log: Log,
    cards: Vec<FakeCard>,
    head_cards: Vec<usize>,
    connectors: BTreeMap<&'static str, usize>,
    next_blob: RefCell<u64>,
}

impl FakeValidationHardware {
    /// DP-1 and DP-3 on card A, DP-2 on card B; `failing` names a card whose
    /// allocations fail.
    fn new(failing: Option<&'static str>) -> Self {
        let log = Log::default();
        let cards = ["A", "B"]
            .map(|name| FakeCard {
                name,
                log: log.clone(),
                fail_allocation: failing == Some(name),
            })
            .into();
        Self {
            log,
            cards,
            head_cards: vec![0, 1, 0],
            connectors: BTreeMap::from([("DP-1", 0), ("DP-2", 1), ("DP-3", 2)]),
            next_blob: RefCell::new(500),
        }
    }

    fn card(&self, index: usize) -> &FakeCard {
        &self.cards[self.head_cards[index]]
    }

    fn entries(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

impl NativeOutputValidationResources for FakeValidationHardware {
    type Buffer = (&'static str, u32);

    fn allocate_validation_framebuffer(
        &self,
        index: usize,
        width: u32,
        height: u32,
    ) -> io::Result<LibdrmNativeValidationFramebuffer<Self::Buffer>> {
        LibdrmNativeValidationFramebuffer::allocate(self.card(index), width, height)
    }

    fn release_validation_framebuffer(
        &self,
        index: usize,
        framebuffer: LibdrmNativeValidationFramebuffer<Self::Buffer>,
    ) -> io::Result<()> {
        framebuffer.release(self.card(index))
    }

    fn release_validation_mode_blob(&self, index: usize, blob: u64) -> io::Result<()> {
        let card = self.card(index).name;
        self.log
            .borrow_mut()
            .push(format!("{card} blob {blob} released"));
        Ok(())
    }
}

impl NativeOutputTopologyHardware for FakeValidationHardware {
    type Head = NativeOutputValidationHead<(&'static str, u32)>;

    fn compose_head(
        &self,
        _output: OutputId,
        connector: &str,
        timing: LibdrmNativeOutputTiming,
    ) -> Result<NativeOutputComposedHead<Self::Head>, NativeOutputHeadUnavailable> {
        let index = *self
            .connectors
            .get(connector)
            .ok_or(NativeOutputHeadUnavailable::MissingSelection)?;
        let blob = {
            let mut next = self.next_blob.borrow_mut();
            *next += 1;
            *next
        };
        let card = self.card(index).name;
        self.log
            .borrow_mut()
            .push(format!("{card} blob {blob} created"));
        let raw = u32::try_from(index).unwrap() + 1;
        compose_native_output_validation_head(
            self,
            index,
            selection(20 + raw, 40 + raw, 60 + raw),
            primary_plane_properties(),
            blob,
            timing,
        )
    }

    fn release_mode_blob(&self, _output: OutputId, blob: u64) {
        panic!("validation blob {blob} released by output, not by its head");
    }

    fn release_head(&self, _output: OutputId, composed: NativeOutputComposedHead<Self::Head>) {
        release_native_output_validation_head(self, composed);
    }
}

fn timing() -> LibdrmNativeOutputTiming {
    LibdrmNativeOutputTiming::new(2560, 1440, 60_000)
}

fn plan(
    outputs: &[u64],
) -> (
    NativeOutputActivationPlan,
    Vec<LibdrmNativeOutputCapability>,
) {
    let capabilities = outputs
        .iter()
        .map(|output| {
            LibdrmNativeOutputCapability::new(
                OutputId::from_raw(*output),
                u32::try_from(*output).unwrap(),
                format!("DP-{output}"),
                [timing()],
                Some(timing()),
                timing(),
                LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let headless = outputs
        .iter()
        .map(|output| HeadlessOutput {
            id: OutputId::from_raw(*output),
            size: Size {
                width: 2560,
                height: 1440,
            },
            scale: 1,
        })
        .collect::<Vec<_>>();
    let topology = project_native_output_topology(&capabilities, &headless).unwrap();
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(7),
        digest: ConfigDigest::new([9; 32]),
        inherit_sophia: true,
        availability: sophia_config::DesktopOutputAvailability::Strict,
        fallback_policy_key: None,
        named: Vec::new(),
    };
    let reconciliation =
        sophia_config::reconcile_desktop_output_candidate(&candidate, &topology).unwrap();
    let plan =
        prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation).unwrap();
    (plan, capabilities)
}

#[test]
fn each_head_names_its_own_test_framebuffer_at_the_requested_mode() {
    let hardware = FakeValidationHardware::new(None);
    let (plan, capabilities) = plan(&[1, 2, 3]);
    let resolved = resolve_native_output_topology_heads(&plan, &capabilities, &hardware).unwrap();
    let size = Size {
        width: 2560,
        height: 1440,
    };
    let objects = resolved
        .heads()
        .iter()
        .map(|head| head_objects(head.as_ref()))
        .collect::<Vec<_>>();
    assert_eq!(
        objects,
        [
            (21, 41, 61, 70, Some(501), size),
            (22, 42, 62, 70, Some(502), size),
            (23, 43, 63, 70, Some(503), size),
        ]
    );
    assert_eq!(
        resolved
            .heads()
            .iter()
            .map(|head| head.index())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn a_dropped_resolution_releases_buffers_and_blobs_on_each_heads_own_card() {
    let hardware = FakeValidationHardware::new(None);
    let (plan, capabilities) = plan(&[1, 2, 3]);
    drop(resolve_native_output_topology_heads(&plan, &capabilities, &hardware).unwrap());
    let entries = hardware.entries();
    let after = &entries[entries
        .iter()
        .position(|entry| entry == "A blob 503 created")
        .unwrap()
        + 3..];
    assert_eq!(
        after,
        [
            "A rmfb",
            "A destroy",
            "A blob 501 released",
            "B rmfb",
            "B destroy",
            "B blob 502 released",
            "A rmfb",
            "A destroy",
            "A blob 503 released",
        ]
    );
}

#[test]
fn a_failed_allocation_releases_its_blob_and_every_earlier_head_on_their_cards() {
    // Card B cannot allocate: DP-2's blob returns to B, DP-1's buffer and blob to
    // A, and DP-3 is never composed.
    let hardware = FakeValidationHardware::new(Some("B"));
    let (plan, capabilities) = plan(&[1, 2, 3]);
    let Err(error) = resolve_native_output_topology_heads(&plan, &capabilities, &hardware) else {
        panic!("a failed allocation must fail the topology");
    };
    assert_eq!(
        error,
        NativeOutputHeadResolveError::Unavailable {
            output: 2,
            cause: NativeOutputHeadUnavailable::FramebufferUnavailable,
        }
    );
    assert_eq!(
        hardware.entries(),
        [
            "A blob 501 created",
            "A create 2560x1440",
            "A addfb",
            "B blob 502 created",
            "B create 2560x1440",
            "B blob 502 released",
            "A rmfb",
            "A destroy",
            "A blob 501 released",
        ]
    );
}

#[test]
fn mirrored_heads_behind_one_output_release_on_their_own_cards() {
    // One OutputId, two connectors on two cards: release follows the head, not
    // the output's primary head.
    let hardware = FakeValidationHardware::new(None);
    let output = OutputId::from_raw(1);
    let first = hardware.compose_head(output, "DP-1", timing()).unwrap();
    let second = hardware.compose_head(output, "DP-2", timing()).unwrap();
    hardware.release_head(output, second);
    hardware.release_head(output, first);
    assert_eq!(
        hardware.entries()[6..],
        [
            "B rmfb",
            "B destroy",
            "B blob 502 released",
            "A rmfb",
            "A destroy",
            "A blob 501 released",
        ]
    );
}

#[test]
fn complete_validation_is_accepted_with_no_previous_scanout_and_frees_after_the_test() {
    let hardware = FakeValidationHardware::new(None);
    let (plan, capabilities) = plan(&[1]);
    let resolved = resolve_native_output_topology_heads(&plan, &capabilities, &hardware).unwrap();
    let device = PlaneRequiredCommitDevice::default();
    let mut executor = NativeOutputTopologyValidationExecutor::new(&device, resolved.heads());
    run_native_output_activation(plan, &mut executor).unwrap();
    assert_eq!(executor.validation(), "accepted");
    assert_eq!(executor.validation_errno(), 0);
    // One TEST_ONLY request, no page-flip event, and the buffer is still owned
    // until the resolution is dropped.
    assert_eq!(*device.submissions.borrow(), [(true, false)]);
    assert!(
        !hardware
            .entries()
            .iter()
            .any(|entry| entry.ends_with("rmfb"))
    );
    drop(resolved);
    assert_eq!(
        hardware.entries()[3..],
        ["A rmfb", "A destroy", "A blob 501 released"]
    );
}

#[test]
fn a_kernel_refusal_keeps_its_errno_and_busy_is_not_rejected() {
    for (errno, word) in [(22, "rejected"), (16, "busy"), (11, "busy")] {
        let hardware = FakeValidationHardware::new(None);
        let (plan, capabilities) = plan(&[1]);
        let resolved =
            resolve_native_output_topology_heads(&plan, &capabilities, &hardware).unwrap();
        let device = PlaneRequiredCommitDevice {
            errno: Some(errno),
            ..PlaneRequiredCommitDevice::default()
        };
        let mut executor = NativeOutputTopologyValidationExecutor::new(&device, resolved.heads());
        run_native_output_activation(plan, &mut executor).unwrap();
        assert_eq!(executor.validation(), word, "errno {errno}");
        assert_eq!(executor.validation_errno(), errno);
    }
}

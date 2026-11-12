#![cfg(test)]
//! Stage 4c-4 (Codex review F2): the production combined-frame routing seam,
//! run with device-free exporters on a running renderer core. Cleanup must
//! follow the issuing facade through disable, mirror and remap, and a missing
//! issuer must get everything back and settle nothing.

use super::*;
use crate::LiveRendererWorkerOutputKey;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
type Validator = IsbValidationWorker<CombinedRenderPlan>;

struct NoDevice;

impl RenderDeviceDiscoveryBackend for NoDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> io::Result<std::fs::File> {
        Err(io::Error::other("no render device in this test"))
    }
}

fn exporter(core: &Arc<crate::NativeGbmRendererWorkerCore>, key: u64) -> Exporter<NoDevice> {
    let mut exporter = Exporter::new(NoDevice);
    exporter.set_output(LiveRendererWorkerOutputKey::from_raw(key));
    exporter.attach_shared_worker(core);
    exporter
}

fn head(id: u32, output: u64) -> LiveProductionNativeHead {
    super::super::native_head_fixture::head(
        id,
        output,
        crate::LibdrmNativePlaneFormatCapabilities::parse(201, 0, &[]),
    )
}

/// Two heads on outputs 1 and 2, each with its own facade on one core.
struct Fixture {
    native_owner: crate::NativeFrameOwner,
    heads: Vec<LiveProductionNativeHead>,
    exporters: Vec<Exporter<NoDevice>>,
    core: Arc<crate::NativeGbmRendererWorkerCore>,
}

impl Fixture {
    fn new(inode: u64) -> Self {
        let core = crate::device_free_core(inode);
        Self {
            native_owner: crate::NativeFrameOwner::new(),
            heads: vec![head(1, 1), head(2, 2)],
            exporters: vec![exporter(&core, 1), exporter(&core, 2)],
            core,
        }
    }

    fn routes(&mut self) -> CombinedRoutes<'_, NoDevice> {
        CombinedRoutes {
            native_owner: self.native_owner,
            heads: &self.heads,
            exporters: &mut self.exporters,
        }
    }
}

fn pixels() -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "combined-routing",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(&[0; 8]).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

/// Admits one frame for `output` through the routing seam and forwards it.
fn admit(
    fixture: &mut Fixture,
    validator: &mut Validator,
    output: u64,
) -> (CombinedHead, CombinedReservation) {
    let (head, inbox) = fixture
        .routes()
        .reserve(OutputId::from_raw(output))
        .expect("the serving facade reserves");
    let reservation = validator
        .reserve_combined(8, Instant::now() + WAIT)
        .unwrap();
    let _owner_plan = validator
        .submit_combined(
            &reservation,
            pixels(),
            crate::device_free_plan(output),
            inbox,
        )
        .ok()
        .expect("combined submission");
    (head, reservation)
}

/// Waits for the issuing facade's result, through the routing seam.
fn collect(fixture: &mut Fixture, head: &CombinedHead) -> CombinedRenderResult {
    let end = Instant::now() + WAIT;
    loop {
        if let Some(result) = fixture
            .routes()
            .take(head)
            .expect("the issuing facade answers")
        {
            return result;
        }
        assert!(
            Instant::now() < end,
            "the renderer did not return the frame"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// One admission per output, then a topology change, then every cleanup
/// call for output 1's admission, then output 2's own settlement.
fn cleanup_follows_the_issuer(inode: u64, change: impl FnOnce(&mut [LiveProductionNativeHead])) {
    let mut fixture = Fixture::new(inode);
    let mut validator = Validator::for_combined_plans(2, 8).unwrap();
    let (other, other_reservation) = admit(&mut fixture, &mut validator, 2);
    let (head, reservation) = admit(&mut fixture, &mut validator, 1);
    change(&mut fixture.heads);

    let result = collect(&mut fixture, &head);
    let job = reservation.job();
    let pixels = fixture
        .routes()
        .settle(&head, &mut validator, reservation, result)
        .ok()
        .expect("the issuing facade settles");
    assert_eq!(pixels.bytes(), &[0; 8], "the validated pixels come home");
    assert_eq!(
        validator.pending(),
        1,
        "only output 2's job is still pending"
    );
    assert_eq!(
        fixture.routes().take(&head).unwrap_err().kind(),
        io::ErrorKind::InvalidInput,
        "the issuer is found, and this admission is spent"
    );
    assert!(
        !fixture.routes().cancel(&head).expect("the issuer is found"),
        "a spent admission cancels nothing"
    );
    assert_eq!(
        fixture.routes().release_held(&head, job),
        CombinedRelease::Mismatch,
        "the issuer is found and holds nothing"
    );
    assert_eq!(
        fixture.routes().flush(&head, job),
        CombinedRelease::Mismatch
    );

    let result = collect(&mut fixture, &other);
    assert!(
        fixture
            .routes()
            .settle(&other, &mut validator, other_reservation, result)
            .is_ok(),
        "the unrelated facade's work was untouched"
    );
    assert_eq!(validator.pending(), 0);
}

#[test]
fn cleanup_follows_the_issuer_after_disable() {
    cleanup_follows_the_issuer(9_501, |heads| heads[0].enabled = false);
}

#[test]
fn cleanup_follows_the_issuer_after_mirror() {
    cleanup_follows_the_issuer(9_502, |heads| heads[1].output = heads[0].output);
}

#[test]
fn cleanup_follows_the_issuer_after_remap() {
    cleanup_follows_the_issuer(9_503, |heads| {
        heads[0].output.id = OutputId::from_raw(9);
        heads[1].output.id = OutputId::from_raw(1);
    });
}

/// Every call for `head` finds no issuer; settlement returns everything.
fn assert_not_found(
    fixture: &mut Fixture,
    head: &CombinedHead,
    validator: &mut Validator,
    reservation: CombinedReservation,
    result: CombinedRenderResult,
) -> (CombinedReservation, CombinedRenderResult) {
    let job = reservation.job();
    assert_eq!(
        fixture.routes().take(head).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        fixture.routes().cancel(head).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        fixture.routes().release_held(head, job),
        CombinedRelease::NotFound
    );
    assert_eq!(fixture.routes().flush(head, job), CombinedRelease::NotFound);
    let Err(CombinedSettleRefused::RendererNotFound(reservation, result)) = fixture
        .routes()
        .settle(head, validator, reservation, result)
    else {
        panic!("a missing issuer must not settle");
    };
    assert_eq!(result.pixels.bytes(), &[0; 8], "the pixels come back");
    assert_eq!(validator.pending(), 1, "nothing settled the credit");
    (reservation, *result)
}

#[test]
fn a_missing_issuer_gets_everything_back_and_settles_nothing() {
    let mut fixture = Fixture::new(9_504);
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let (head, reservation) = admit(&mut fixture, &mut validator, 1);
    let result = collect(&mut fixture, &head);

    // A rebuilt native owner.
    let issuing_owner = fixture.native_owner;
    fixture.native_owner = crate::NativeFrameOwner::new();
    let (reservation, result) =
        assert_not_found(&mut fixture, &head, &mut validator, reservation, result);
    fixture.native_owner = issuing_owner;

    // A replacement facade under the same output key, at the same head.
    let replacement = exporter(&fixture.core, 1);
    let original = std::mem::replace(&mut fixture.exporters[0], replacement);
    let (reservation, result) =
        assert_not_found(&mut fixture, &head, &mut validator, reservation, result);
    fixture.exporters[0] = original;

    assert!(
        fixture
            .routes()
            .settle(&head, &mut validator, reservation, result)
            .is_ok(),
        "the issuer, once back, settles what was returned"
    );
    assert_eq!(validator.pending(), 0);
}

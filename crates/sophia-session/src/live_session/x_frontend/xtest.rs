// XTEST admission for a live session: who may inject, and what is counted.
//
// The injector itself is the authority's `RoutedXTestInjector`; this file
// decides who is issued one and keeps the numbers the completion record
// reports. Admission is keyed on the namespace the client was admitted into,
// so a session serving several groups on several listeners (t142) admits one
// group and not the rest with no change here. Today the live frontend admits
// every client into one namespace, and the flag reaches all of them -- which
// is what XTEST has always meant on one display, and creates no reach across
// a boundary that does not yet exist.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sophia_protocol::{
    ClientAdmissionContext, DeviceId, InputEventKind, InputEventPacket, LayerSnapshot, NamespaceId,
    Point, SeatId, SurfaceId,
};
use sophia_x_authority::{
    PrivateRequestBarrier, RoutedXTestInjector, XAuthorityRoutedInputSender,
    XServerFrontendInjectionError, XServerFrontendInjectionPolicy, XTestAccepted,
    XTestInjectionRefusal, XTestInjector,
};

/// What XTEST did in this session, for the completion record.
#[derive(Debug, Default)]
pub(crate) struct LiveXTestEvidence {
    pub(crate) issued: AtomicU64,
    pub(crate) denied: AtomicU64,
    pub(crate) injected_keys: AtomicU64,
    pub(crate) injected_buttons: AtomicU64,
    pub(crate) injected_motions: AtomicU64,
    pub(crate) refused: AtomicU64,
}

impl LiveXTestEvidence {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn get(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}

/// The Engine's input layers as the owner loop last published them, so an
/// XTEST pointer event can be resolved the way a physical one is.
///
/// WHY THIS EXISTS. The authority's XTEST plan targets motion and buttons at
/// the focused window, and the registry and frontend trust that target, so a
/// synthetic pointer could only ever land in the focused window -- in a
/// session with no window manager, the first window and no other (t156). A
/// physical pointer is hit-tested against these same layers by the owner
/// loop. The injector runs on an X worker, which cannot see the owner's
/// runtime, so the owner publishes the layers here whenever their
/// presentation epoch changes and the injector hit-tests against the last
/// publication. It can therefore be at most one owner pass behind what the
/// pointer is over; a surface that has just appeared becomes reachable on
/// the next pass.
///
/// IMPLICIT GRAB. While any XTEST button is down, motion and buttons stay on
/// the surface the first press landed on, positioned against that surface's
/// origin at the press, until the last button is released -- the core
/// protocol's implicit pointer grab, which physical input gets from its route
/// leases. Without it a drag that left the pressed window delivered its
/// release to whatever lay under it, and xterm, which claims PRIMARY on the
/// release, never did (t124, under Hagia). The state is here rather than in
/// an injector because there is one XTEST device per session, whichever
/// client drives it.
#[derive(Debug, Default)]
pub(crate) struct LiveXTestPointerScene {
    published: Mutex<(u64, Arc<[LayerSnapshot]>)>,
    held: Mutex<Option<HeldPointer>>,
}

/// The surface an XTEST press landed on, where its origin was, and which
/// buttons are still down.
#[derive(Debug)]
struct HeldPointer {
    surface: SurfaceId,
    origin: Point,
    buttons: std::collections::BTreeSet<u32>,
}

impl HeldPointer {
    fn place(&self, global: Point) -> (SurfaceId, Point) {
        (
            self.surface,
            Point {
                x: global.x - self.origin.x,
                y: global.y - self.origin.y,
            },
        )
    }
}

impl LiveXTestPointerScene {
    /// Replace the layers when `epoch` has moved. The owner calls this every
    /// pass; an unchanged epoch costs a lock and a compare.
    pub(crate) fn publish(&self, epoch: u64, layers: &[LayerSnapshot]) {
        let Ok(mut published) = self.published.lock() else {
            return;
        };
        if published.0 != epoch || published.1.len() != layers.len() {
            *published = (epoch, Arc::from(layers));
        }
    }

    /// Where a synthetic motion goes: the held surface during a grab, the
    /// surface under it otherwise.
    pub(crate) fn route_motion(
        &self,
        seat: SeatId,
        device: DeviceId,
        planned: SurfaceId,
        global: Point,
        local: Point,
    ) -> (SurfaceId, Point) {
        if let Ok(held) = self.held.lock()
            && let Some(held) = held.as_ref()
        {
            return held.place(global);
        }
        resolve_pointer_target(&self.layers(), seat, device, planned, global, local)
    }

    /// Where a synthetic button goes, opening the grab on the first press and
    /// closing it on the last release.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn route_button(
        &self,
        seat: SeatId,
        device: DeviceId,
        planned: SurfaceId,
        button: u32,
        pressed: bool,
        global: Point,
        local: Point,
    ) -> (SurfaceId, Point) {
        let Ok(mut held) = self.held.lock() else {
            return resolve_pointer_target(&self.layers(), seat, device, planned, global, local);
        };
        if let Some(grab) = held.as_mut() {
            let placed = grab.place(global);
            if pressed {
                grab.buttons.insert(button);
            } else {
                grab.buttons.remove(&button);
                if grab.buttons.is_empty() {
                    *held = None;
                }
            }
            return placed;
        }
        let (surface, position) =
            resolve_pointer_target(&self.layers(), seat, device, planned, global, local);
        if pressed {
            *held = Some(HeldPointer {
                surface,
                origin: Point {
                    x: global.x - position.x,
                    y: global.y - position.y,
                },
                buttons: std::collections::BTreeSet::from([button]),
            });
        }
        (surface, position)
    }

    fn layers(&self) -> Arc<[LayerSnapshot]> {
        self.published.lock().map_or_else(
            |_| Arc::from(Vec::new()),
            |published| Arc::clone(&published.1),
        )
    }
}

/// The surface under `global`, and the position within it, by the Engine's
/// own hit-test -- or the planned target and position when nothing is hit
/// (the bare root, or a scene not yet published). Keys never come here: they
/// belong to the focus, and the plan's target is right for them.
pub(crate) fn resolve_pointer_target(
    layers: &[LayerSnapshot],
    seat: SeatId,
    device: DeviceId,
    planned: SurfaceId,
    global: Point,
    local: Point,
) -> (SurfaceId, Point) {
    let event = InputEventPacket {
        serial: 0,
        seat,
        device,
        time_msec: 0,
        kind: InputEventKind::PointerMotion,
        global_position: Some(global),
        target_surface: None,
        local_position: None,
    };
    let route = sophia_engine::hit_test_scene_surface_for_input(&event, layers);
    match (route.target_surface, route.local_position) {
        (Some(surface), Some(position)) => (surface, position),
        _ => (planned, local),
    }
}

/// Issues an injector to a client admitted into the nominated namespace, and
/// to nobody else.
pub(crate) struct LiveXTestInjectionPolicy {
    pub(crate) namespace: NamespaceId,
    pub(crate) seat: SeatId,
    /// A device of its own, so no synthetic event is ever attributed to the
    /// seat's keyboard or pointer.
    pub(crate) device: DeviceId,
    pub(crate) sender: XAuthorityRoutedInputSender,
    pub(crate) evidence: Arc<LiveXTestEvidence>,
    pub(crate) scene: Arc<LiveXTestPointerScene>,
}

impl XServerFrontendInjectionPolicy for LiveXTestInjectionPolicy {
    fn issue(
        &self,
        context: ClientAdmissionContext,
        _device: DeviceId,
    ) -> Result<Box<dyn XTestInjector>, XServerFrontendInjectionError> {
        if context.namespace.id != self.namespace {
            LiveXTestEvidence::bump(&self.evidence.denied);
            return Err(XServerFrontendInjectionError::Denied);
        }
        LiveXTestEvidence::bump(&self.evidence.issued);
        Ok(Box::new(LiveXTestInjector {
            inner: RoutedXTestInjector::new(self.sender.clone(), self.seat, self.device),
            evidence: Arc::clone(&self.evidence),
            scene: Arc::clone(&self.scene),
            seat: self.seat,
            device: self.device,
        }))
    }
}

/// The authority's injector, counted, with pointer events resolved against
/// the Engine's scene rather than the focus.
struct LiveXTestInjector {
    inner: RoutedXTestInjector,
    evidence: Arc<LiveXTestEvidence>,
    scene: Arc<LiveXTestPointerScene>,
    seat: SeatId,
    device: DeviceId,
}

impl LiveXTestInjector {
    fn counted(
        &self,
        counter: &AtomicU64,
        result: Result<XTestAccepted, XTestInjectionRefusal>,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        match &result {
            Ok(_) => LiveXTestEvidence::bump(counter),
            Err(_) => LiveXTestEvidence::bump(&self.evidence.refused),
        }
        result
    }
}

impl XTestInjector for LiveXTestInjector {
    fn report_completions_to(&self, barrier: PrivateRequestBarrier) -> bool {
        self.inner.report_completions_to(barrier)
    }

    fn submit_key(
        &self,
        target: SurfaceId,
        keycode: u32,
        pressed: bool,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        self.counted(
            &self.evidence.injected_keys,
            self.inner.submit_key(target, keycode, pressed),
        )
    }

    fn submit_button(
        &self,
        target: SurfaceId,
        button: u32,
        pressed: bool,
        global: Point,
        local: Point,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        let (target, local) = self.scene.route_button(
            self.seat,
            self.device,
            target,
            button,
            pressed,
            global,
            local,
        );
        self.counted(
            &self.evidence.injected_buttons,
            self.inner
                .submit_button(target, button, pressed, global, local),
        )
    }

    fn submit_motion(
        &self,
        target: SurfaceId,
        global: Point,
        local: Point,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        let (target, local) =
            self.scene
                .route_motion(self.seat, self.device, target, global, local);
        self.counted(
            &self.evidence.injected_motions,
            self.inner.submit_motion(target, global, local),
        )
    }
}

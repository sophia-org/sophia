// An XTEST injector that submits to the shared routed ingress.
//
// WHAT THIS IS FOR. The private instance issues injectors bound to its own
// runtime, order and custody. A live session has none of those: its routed
// input goes down one channel into one registry, physical and synthetic
// alike, and the only thing an XTEST client is owed beyond that is the
// ordering FakeInput promises its next request -- not until the work was
// accepted, but until the registry has taken the effect. This type keeps that
// promise with the barrier the connection installs and the completion slot
// the routed envelope carries, and nothing else. It decides no policy: who
// may hold one is the issuing policy's question, and the reserved chord is
// refused where the seat's modifiers are known, on the delivery path.

/// Submits synthetic input to the routed ingress and answers the barrier
/// after the registry has applied it.
pub struct RoutedXTestInjector {
    sender: XAuthorityRoutedInputSender,
    seat: sophia_protocol::SeatId,
    device: sophia_protocol::DeviceId,
    /// Ticks from the moment this injector was issued. A timestamp base of
    /// its own, since the shared ingress carries no clock a client could be
    /// handed; per-client ordering holds regardless, because the registry
    /// applies routes in the order the one channel delivers them.
    started: std::time::Instant,
    serial: std::sync::atomic::AtomicU64,
    /// Numbers the requests this injector arms on its barrier. The connection
    /// serialises FakeInput, so at most one is ever armed, but the barrier
    /// still asks for a number and a stale slot is disarmed by a new one.
    requests: std::sync::atomic::AtomicU64,
    /// Installed once by the connection that owns this injector, before its
    /// first request. `None` only between issue and that installation, when
    /// nothing can be submitted yet.
    barrier: std::sync::OnceLock<PrivateRequestBarrier>,
}

impl RoutedXTestInjector {
    pub fn new(
        sender: XAuthorityRoutedInputSender,
        seat: sophia_protocol::SeatId,
        device: sophia_protocol::DeviceId,
    ) -> Self {
        Self {
            sender,
            seat,
            device,
            started: std::time::Instant::now(),
            serial: std::sync::atomic::AtomicU64::new(1),
            requests: std::sync::atomic::AtomicU64::new(1),
            barrier: std::sync::OnceLock::new(),
        }
    }

    fn submit(
        &self,
        target: sophia_protocol::SurfaceId,
        global: sophia_protocol::Point,
        local: sophia_protocol::Point,
        kind: sophia_protocol::InputEventKind,
    ) -> Result<crate::XTestAccepted, crate::XTestInjectionRefusal> {
        let Some(barrier) = self.barrier.get() else {
            return Err(crate::XTestInjectionRefusal::Unavailable);
        };
        let serial = self
            .serial
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let route = XAuthorityRoutedInput {
            request: sophia_protocol::RoutedInputRequest {
                serial,
                seat: self.seat,
                device: self.device,
                time_msec: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                target_surface: target,
                global_position: global,
                local_position: local,
                kind,
            },
            route_lease: None,
            // NO DELIVERY TRACKING. A delivery id enters the recovery ledger,
            // which the session's own input phase already numbers from its
            // counter and settles from its receipts; a second numbering would
            // collide with it or answer receipts it never issued. With none,
            // routing begins and finishes as a no-op in that ledger, and what
            // says this work finished is the barrier, not a receipt.
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
            origin: XAuthorityRoutedInputOrigin::Synthetic,
        };
        let request = self
            .requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ticket = barrier.arm(request);
        // A refused send has already answered the ticket, so the connection
        // does not wait on it; the refusal names a dead receiver, which no
        // retry mends.
        self.sender
            .send_with_completion(route, ticket)
            .map(|()| crate::XTestAccepted { sequence: None })
            .map_err(|_| crate::XTestInjectionRefusal::Disconnected)
    }
}

impl crate::XTestInjector for RoutedXTestInjector {
    fn report_completions_to(&self, barrier: PrivateRequestBarrier) -> bool {
        self.barrier.set(barrier).is_ok()
    }

    fn submit_key(
        &self,
        target: sophia_protocol::SurfaceId,
        keycode: u32,
        pressed: bool,
    ) -> Result<crate::XTestAccepted, crate::XTestInjectionRefusal> {
        self.submit(
            target,
            sophia_protocol::Point::default(),
            sophia_protocol::Point::default(),
            sophia_protocol::InputEventKind::Key { keycode, pressed },
        )
    }

    fn submit_button(
        &self,
        target: sophia_protocol::SurfaceId,
        button: u32,
        pressed: bool,
        global: sophia_protocol::Point,
        local: sophia_protocol::Point,
    ) -> Result<crate::XTestAccepted, crate::XTestInjectionRefusal> {
        // The routed event is delivered at the position it carries, so a
        // button must carry the pointer's; the origin put every press there.
        self.submit(
            target,
            global,
            local,
            sophia_protocol::InputEventKind::PointerButton { button, pressed },
        )
    }

    fn submit_motion(
        &self,
        target: sophia_protocol::SurfaceId,
        global: sophia_protocol::Point,
        local: sophia_protocol::Point,
    ) -> Result<crate::XTestAccepted, crate::XTestInjectionRefusal> {
        self.submit(
            target,
            global,
            local,
            sophia_protocol::InputEventKind::PointerMotion,
        )
    }
}

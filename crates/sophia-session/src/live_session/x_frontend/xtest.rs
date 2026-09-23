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

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_protocol::{ClientAdmissionContext, DeviceId, NamespaceId, Point, SeatId, SurfaceId};
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
        }))
    }
}

/// The authority's injector, counted.
struct LiveXTestInjector {
    inner: RoutedXTestInjector,
    evidence: Arc<LiveXTestEvidence>,
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
        self.counted(
            &self.evidence.injected_motions,
            self.inner.submit_motion(target, global, local),
        )
    }
}

use super::*;

#[path = "session_present_unclocked.rs"]
mod unclocked;
use sophia_protocol::{NamespaceId, Rect, SurfaceConstraints, SurfaceId};
use sophia_x_authority::{
    XAuthorityRequestKind, XAuthorityRequestPacket, XAuthorityRuntime, XPresentMscTiming,
    XResourceId,
};
use std::cell::RefCell;

pub(super) const NS: NamespaceId = NamespaceId::from_raw(321);
pub(super) const WINDOW: XResourceId = XResourceId::new(0x400001, 1);
pub(super) const SURFACE: SurfaceId = SurfaceId::new(321, 1);

// Real admission/clock reducers with an injected transport and observations.
// No KMS device or wire dispatch is needed to exercise the Session seam.
pub(super) struct AdmissionFrontend {
    pub(super) runtime: RefCell<XAuthorityRuntime>,
    pub(super) bound: RefCell<Vec<(TransactionId, XPresentClockSample)>>,
}
impl AdmissionFrontend {
    pub(super) fn new(mapped: bool) -> Self {
        let mut runtime = XAuthorityRuntime::new();
        runtime.apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(1),
            namespace: NS,
            kind: XAuthorityRequestKind::CreateWindow {
                window: WINDOW,
                surface: SURFACE,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 20,
                },
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        });
        if mapped {
            runtime.apply(XAuthorityRequestPacket {
                transaction: TransactionId::from_raw(2),
                namespace: NS,
                kind: XAuthorityRequestKind::MapWindow {
                    window: WINDOW,
                    generation: 1,
                },
            });
        }
        Self {
            runtime: RefCell::new(runtime),
            bound: RefCell::new(Vec::new()),
        }
    }
    fn prepare(&self, id: u64, target: u64) {
        self.runtime
            .borrow_mut()
            .prepare_present_msc_notify(
                1,
                TransactionId::from_raw(id),
                NS,
                WINDOW,
                id as u32,
                XPresentMscTiming::notify(target, 0, 0).unwrap(),
            )
            .unwrap();
    }
}
impl FrontendClocks for AdmissionFrontend {
    fn admissions(&self) -> ClockResult<Vec<XPresentClockAdmission>> {
        Ok(self.runtime.borrow().present_clock_admissions())
    }
    fn bind_admission(
        &self,
        request: TransactionId,
        sample: XPresentClockSample,
    ) -> ClockResult<()> {
        assert!(self.runtime.borrow_mut().resolve_present_clock_admission(
            request,
            sample,
            None,
            || 50_100_000
        ));
        self.bound.borrow_mut().push((request, sample));
        Ok(())
    }
    fn query_demands(&self) -> ClockResult<Vec<(XPresentClockSource, u64)>> {
        Ok(Vec::new())
    }
    fn bound_sources(&self) -> ClockResult<Vec<XPresentClockSource>> {
        Ok(Vec::new())
    }
    fn lose_source(&self, _: XPresentClockSource) -> ClockResult<()> {
        Ok(())
    }
    fn observe_source(&self, _: XPresentClockSample) -> ClockResult<()> {
        Ok(())
    }
}

#[test]
fn one_bad_hardware_binding_does_not_abort_the_session_admission_batch() {
    let frontend = AdmissionFrontend::new(true);
    frontend.prepare(3, 20);
    let accepted = XPresentClockSample {
        source: XPresentClockSource::Hardware {
            domain: 1,
            incarnation: 1,
        },
        ust: 10_000,
        msc: 10,
    };
    frontend
        .bind_admission(TransactionId::from_raw(3), accepted)
        .unwrap();
    frontend.prepare(4, 0);
    frontend.prepare(5, 0);
    let mut bridge = SessionPresentClocks::default();
    bridge
        .admit_with(
            &frontend,
            50_100_000,
            |_| BTreeMap::from([(SURFACE, Some(OutputId::from_raw(1)))]),
            |_| {
                vec![(
                    RenderHeadId::from_raw(1),
                    LiveNativePresentClockObservation {
                        lost: None,
                        current: Some(sophia_backend_live::LiveNativePresentClockSample {
                            source: LiveNativePresentClockSource {
                                owner: 1,
                                incarnation: 1,
                            },
                            ust_usec: 10_000,
                            msc: 9,
                        }),
                        status: sophia_backend_live::LiveNativePresentClockStatus::Observed,
                    },
                )]
            },
        )
        .unwrap();
    assert_eq!(
        frontend
            .bound
            .borrow()
            .iter()
            .map(|(id, _)| id.raw())
            .collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    assert!(frontend.admissions().unwrap().is_empty());
    let stats = frontend.runtime.borrow().present_timing_statistics();
    assert_eq!(stats.admission_errors, 2);
    assert_eq!(stats.admission_fake_retries, 2);
    assert_eq!(stats.admission_settled, 0);
    assert_eq!(bridge.queries(), 1);
}

#[test]
fn session_binds_admissions_without_a_native_owner_or_visual_runtime() {
    for mapped in [false, true] {
        let frontend = AdmissionFrontend::new(mapped);
        frontend.prepare(3, 0);
        frontend.prepare(4, 0);
        let mut bridge = SessionPresentClocks::default();
        // This is the production service entry used before startup, after
        // owner release and outside the frame-service quarantine guard.
        bridge
            .service(&frontend, None, |_| BTreeMap::new(), Instant::now())
            .unwrap();
        assert!(frontend.admissions().unwrap().is_empty());
        assert_eq!(
            frontend
                .bound
                .borrow()
                .iter()
                .map(|(id, sample)| {
                    assert_eq!(sample.source, XPresentClockSource::Fake);
                    assert!(sample.ust > 0);
                    id.raw()
                })
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(
            frontend
                .runtime
                .borrow()
                .prepared_present_deadline_usec()
                .is_some()
        );
        bridge
            .service(&frontend, None, |_| BTreeMap::new(), Instant::now())
            .unwrap();
        assert_eq!(frontend.bound.borrow().len(), 2); // no rebind
        assert_eq!(bridge.queries(), 0);
    }
}

#[test]
fn missing_target_and_inactive_native_selection_bind_fake_in_request_order() {
    for mapped in [false, true] {
        let frontend = AdmissionFrontend::new(mapped);
        frontend.prepare(3, 11);
        frontend.prepare(4, 11);
        let output = OutputId::from_raw(1);
        let mut bridge = SessionPresentClocks::default();
        let mut visits = 0;
        bridge
            .admit_with(
                &frontend,
                10_100_000,
                |_| BTreeMap::from([(SURFACE, Some(output))]),
                |_| {
                    visits += 1;
                    Vec::new()
                },
            )
            .unwrap();
        assert_eq!(visits, usize::from(mapped));
        assert_eq!(frontend.bound.borrow().len(), 2);
        assert!(
            frontend
                .bound
                .borrow()
                .iter()
                .all(|(_, sample)| *sample == XPresentClockSample::background(10_100_000))
        );
        assert_eq!(
            frontend.runtime.borrow().prepared_present_deadline_usec(),
            Some(11_000_000)
        );
    }
}

#[test]
fn one_output_query_is_shared_by_new_requests_and_idle_has_no_selection_work() {
    let frontend = AdmissionFrontend::new(true);
    frontend.prepare(3, 30);
    frontend.prepare(4, 31);
    let output = OutputId::from_raw(1);
    let mut bridge = SessionPresentClocks::default();
    let mut visits = 0;
    bridge
        .admit_with(
            &frontend,
            10_100_000,
            |_| BTreeMap::from([(SURFACE, Some(output))]),
            |_| {
                visits += 1;
                vec![(
                    RenderHeadId::from_raw(1),
                    LiveNativePresentClockObservation {
                        lost: None,
                        current: Some(sophia_backend_live::LiveNativePresentClockSample {
                            source: LiveNativePresentClockSource {
                                owner: 1,
                                incarnation: 1,
                            },
                            ust_usec: 20_000,
                            msc: 20,
                        }),
                        status: sophia_backend_live::LiveNativePresentClockStatus::Observed,
                    },
                )]
            },
        )
        .unwrap();
    assert_eq!(visits, 1);
    assert_eq!(bridge.queries(), 1);
    assert!(
        frontend
            .bound
            .borrow()
            .iter()
            .all(|(_, sample)| sample.msc == 20)
    );
    bridge
        .admit_with(
            &frontend,
            10_200_000,
            |_| panic!("no idle visibility rebuild"),
            |_| panic!("no idle query"),
        )
        .unwrap();
}

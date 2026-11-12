//! G3-1 owner order through the real image owner: the hooks admit and send a
//! combined frame copy, and the result they collect is reconciled in the
//! same service call (SR1).
use super::*;
use crate::{
    CombinedFrameAdmitted, CombinedFrameHooks, CombinedInbox, CombinedReservation, CombinedResult,
    ForwardedValidation, ShellIsbImageOwner,
};
use sophia_protocol::shell_files::ShellFrameCommit;
use std::sync::mpsc;

type Worker = IsbValidationWorker<u32>;
const WAIT: Duration = Duration::from_secs(10);
const TICKET: u64 = 9;

#[derive(Default)]
struct Probe {
    events: Vec<&'static str>,
    refuse_plan: bool,
    inbox: Option<mpsc::Receiver<ForwardedValidation<u32>>>,
    arrived: Option<ForwardedValidation<u32>>,
    row: Option<(CombinedReservation, mpsc::Receiver<u32>)>,
    description: Option<ContentResourceBegin>,
}

impl CombinedFrameHooks<u32> for Probe {
    fn reserve(&mut self, _frame: &ShellFrameCommit) -> Option<(u64, CombinedInbox<u32>)> {
        self.events.push("reserve");
        let (sender, receiver) = mpsc::sync_channel(1);
        self.inbox = Some(receiver);
        Some((
            TICKET,
            CombinedInbox::new(sender, sophia_wake::WakeSlot::default()),
        ))
    }

    fn plan(&mut self, ticket: u64, admitted: &CombinedFrameAdmitted<'_>) -> Option<u32> {
        assert_eq!(ticket, TICKET);
        self.events.push("plan");
        self.description = Some(admitted.description.clone());
        (!self.refuse_plan).then_some(7)
    }

    fn cancel(&mut self, ticket: u64) {
        assert_eq!(ticket, TICKET);
        self.events.push("cancel");
    }

    fn submitted(
        &mut self,
        ticket: u64,
        reservation: CombinedReservation,
        returned: mpsc::Receiver<u32>,
    ) {
        assert_eq!(ticket, TICKET);
        self.events.push("submitted");
        self.row = Some((reservation, returned));
    }

    fn collect(
        &mut self,
        worker: &mut Worker,
        deliver: &mut dyn FnMut(u64, CombinedResult) -> Result<(), CombinedResult>,
    ) {
        self.events.push("collect");
        let Some(envelope) = self.arrived.take() else {
            return;
        };
        assert_eq!(envelope.description(), self.description.as_ref());
        let (reservation, returned) = self.row.take().expect("a submitted row");
        let job = reservation.job();
        let Ok((pixels, plan)) = worker.settle_combined(reservation, envelope) else {
            panic!("own token settles");
        };
        assert_eq!(plan, 7);
        assert!(
            returned.try_recv().is_err(),
            "a forwarded plan never returns"
        );
        assert!(deliver(job, Ok(pixels)).is_ok(), "the exact row takes it");
        self.events.push("delivered");
    }
}

struct Joined {
    f: Fixture,
    owner: ShellIsbImageOwner<u32>,
    probe: Probe,
}

impl Joined {
    fn new() -> Self {
        let mut joined = Self {
            f: Fixture::unserviced(),
            owner: ShellIsbImageOwner::for_combined_plans(1 << 20, 65_536).unwrap(),
            probe: Probe::default(),
        };
        joined.service();
        assert_eq!(joined.take_events(), ["collect"]);
        joined
    }

    fn service(&mut self) {
        self.owner
            .service_combined(
                &mut self.f.transport,
                &mut self.f.epochs,
                1,
                Instant::now(),
                &mut self.probe,
            )
            .unwrap();
    }

    fn take_events(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.probe.events)
    }

    fn ready(&self) -> bool {
        self.f
            .epochs
            .active_candidates(identity().grant)
            .unwrap()
            .ready_frame_commit()
            .is_some()
    }
}

#[test]
fn a_collected_result_is_reconciled_in_the_same_service_call() {
    let mut j = Joined::new();
    j.f.submit(1);
    j.service();
    assert_eq!(j.take_events(), ["collect", "reserve", "plan", "submitted"]);
    assert_eq!(j.owner.pending(), 1);
    assert!(!j.ready(), "the copy gate holds the candidate");
    let description = j.probe.description.as_ref().unwrap();
    assert_eq!(description.resource, offer(1, 0, 2).request.resource);
    let envelope = j
        .probe
        .inbox
        .as_ref()
        .unwrap()
        .recv_timeout(WAIT)
        .expect("validator forwards");
    assert!(!envelope.stopped());
    j.probe.arrived = Some(envelope);
    j.service();
    assert_eq!(j.take_events(), ["collect", "delivered"]);
    assert!(
        j.ready(),
        "intake precedes reconciliation: no extra owner turn"
    );
    let resource = offer(1, 0, 2).request.resource;
    assert!(
        j.f.epochs
            .resources(identity().grant)
            .unwrap()
            .lease(identity().grant, resource)
            .is_err(),
        "copy is private until Presented"
    );
}

#[test]
fn a_plan_build_failure_cancels_the_ticket_and_sends_nothing() {
    let mut j = Joined::new();
    j.probe.refuse_plan = true;
    j.f.submit(1);
    j.service();
    assert_eq!(j.take_events(), ["collect", "reserve", "plan", "cancel"]);
    assert!(j.probe.row.is_none());
    assert!(
        j.probe
            .inbox
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
        "no validator job owns the inbox"
    );
    assert!(!j.ready());
}

#[test]
fn a_refused_admission_cancels_the_ticket_before_any_plan() {
    let mut j = Joined::new();
    j.f.submit(2);
    j.service();
    assert_eq!(j.take_events(), ["collect", "reserve", "cancel"]);
    assert_eq!(j.owner.pending(), 0);
    assert!(j.probe.row.is_none());
}

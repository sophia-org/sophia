//! G3-1 owner order through the real image owner, journal and owned ring.
//! The hooks admit and send a combined frame copy; the result they collect
//! is reconciled in the same service call (SR1). Publication keeps today's
//! order: the host journals the terminal ResourceStatus, and a later call
//! publishes exactly one SlotReleased for the originating slot.
use super::*;
use crate::shell_transport::outbound::{Admitted, OutboundRecord};
use crate::shell_transport::outbox::tests::files::Peer;
use crate::{
    CombinedFrameAdmitted, CombinedFrameHooks, CombinedInbox, CombinedReservation, CombinedResult,
    ForwardedValidation, IsbValidationFailure, ShellIsbImageOwner,
};
use sophia_protocol::shell_files::{ShellFileAck, ShellFrameCommit};
use std::sync::mpsc;

type Worker = IsbValidationWorker<u32>;
const WAIT: Duration = Duration::from_secs(10);
const TICKET: u64 = 9;

#[derive(Default)]
struct Probe {
    events: Vec<&'static str>,
    refuse_plan: bool,
    /// Deliver the renderer's description mismatch instead of the pixels.
    malformed: bool,
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
        let result = if self.malformed {
            drop(pixels);
            Err(IsbValidationFailure::Malformed)
        } else {
            Ok(pixels)
        };
        assert!(deliver(job, result).is_ok(), "the exact row takes it");
        self.events.push("delivered");
    }
}

struct Joined {
    f: Fixture,
    owner: ShellIsbImageOwner<u32>,
    probe: Probe,
    /// Where journal reads start: the end the test last acknowledged.
    floor: u64,
}

impl Joined {
    fn new() -> Self {
        let mut joined = Self {
            f: Fixture::unserviced(),
            owner: ShellIsbImageOwner::for_combined_plans(1 << 20, 65_536).unwrap(),
            probe: Probe::default(),
            floor: 0,
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

    /// The host's part of the same owner turn: the content-resource service
    /// moves store events to the outbox, and the file turn drains it. False
    /// when a full journal holds records back.
    fn host(&mut self) -> bool {
        self.f
            .transport
            .flush_content_resource_events(&mut self.f.epochs)
            .unwrap();
        Peer::drain(&mut self.f.transport)
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

    /// The copy's journalled statuses, as (status, reason).
    fn statuses(&mut self) -> Vec<(u16, u16)> {
        let resource = offer(1, 0, 2).request.resource;
        let files = self.f.transport.files_mut().unwrap().export();
        statuses_in(&journal_bytes_from(files, self.floor))
            .into_iter()
            .filter(|status| status.resource == resource)
            .map(|status| (status.status, status.reason))
            .collect()
    }

    /// Journalled SlotReleased events, as (slot, reason).
    fn releases(&mut self) -> Vec<(crate::OwnedSlotIdentity, u16)> {
        let files = self.f.transport.files_mut().unwrap().export();
        events_in(&journal_bytes_from(files, self.floor))
            .into_iter()
            .filter(|event| event.kind == EventKind::SlotReleased)
            .map(|event| (event.slot, event.reason))
            .collect()
    }

    /// The export still owes the offer's status or slot release.
    fn owes(&mut self) -> bool {
        let files = self.f.transport.files_mut().unwrap().export();
        files.owned.live.iter().any(Option::is_some)
    }

    /// Submit the frame, send it, and park its forwarded result for collect.
    fn forwarded(&mut self) {
        self.f.submit(1);
        self.service();
        assert_eq!(
            self.take_events(),
            ["collect", "reserve", "plan", "submitted"]
        );
        let envelope = self
            .probe
            .inbox
            .as_ref()
            .unwrap()
            .recv_timeout(WAIT)
            .expect("validator forwards");
        assert!(!envelope.stopped());
        self.probe.arrived = Some(envelope);
    }

    /// Status first, then exactly one SlotReleased, then nothing more.
    fn publishes(&mut self, status: (u16, u16), release_reason: u16) {
        assert!(self.releases().is_empty(), "no release before its status");
        assert!(self.host());
        assert_eq!(self.statuses(), [status]);
        assert!(
            self.releases().is_empty(),
            "status journalled, release next"
        );
        assert!(self.owes(), "the Releasing entry survives until published");
        self.service();
        let slot = offer(1, 0, 2).request.slot;
        assert_eq!(self.releases(), [(slot, release_reason)]);
        assert_eq!(self.owner.pending(), 0);
        assert!(!self.owes());
        self.service();
        assert!(self.host());
        assert_eq!(self.statuses(), [status], "status published once");
        assert_eq!(self.releases().len(), 1, "release published once");
    }
}

#[test]
fn a_collected_result_is_reconciled_in_the_same_service_call_and_released_after_its_status() {
    let mut j = Joined::new();
    j.forwarded();
    assert_eq!(j.owner.pending(), 1);
    assert!(!j.ready(), "the copy gate holds the candidate");
    let description = j.probe.description.as_ref().unwrap();
    assert_eq!(description.resource, offer(1, 0, 2).request.resource);
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
    j.publishes((2, 0), 0);
}

#[test]
fn a_delivered_mismatch_is_published_malformed_once_through_the_row() {
    let mut j = Joined::new();
    j.probe.malformed = true;
    j.forwarded();
    j.service();
    assert_eq!(j.take_events(), ["collect", "delivered"]);
    let malformed = ContentReason::Malformed as u16;
    j.publishes((3, malformed), malformed);
}

#[test]
fn a_plan_build_failure_cancels_the_ticket_and_releases_the_slot_once() {
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
    let cancelled = ContentReason::Cancelled as u16;
    j.publishes((4, cancelled), cancelled);
}

#[test]
fn a_full_journal_keeps_the_releasing_entry_until_retry() {
    let mut j = Joined::new();
    j.forwarded();
    // An ordinary record ahead of the status in a full journal: reserved
    // terminals queue behind it in FIFO order.
    let filler = OutboundRecord::Content(
        TransactionId::from_raw(90),
        ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: identity().grant,
            resource: ContentResourceId {
                id: 7,
                generation: 1,
            },
            status: 3,
            reason: ContentReason::Malformed as u16,
            next_ordinal: 0,
            admitted_bytes: 0,
        }),
    );
    assert!(Peer::fill_journal(&mut j.f.transport, &filler) > 0);
    j.f.transport.output.push(Admitted {
        resource_settlement: false,
        record: filler,
        control: false,
        charge: 0,
    });
    j.service();
    assert_eq!(j.take_events(), ["collect", "delivered"]);
    assert!(j.ready());
    for _ in 0..2 {
        assert!(!j.host(), "the full journal refuses the queue head");
        j.service();
        assert!(j.statuses().is_empty());
        assert!(j.releases().is_empty());
        assert!(j.owes(), "custody and the Releasing entry survive");
        assert_eq!(j.owner.pending(), 1);
    }
    let files = j.f.transport.files_mut().unwrap().export_mut();
    let sequence = files.journal.records() as u64;
    // Acknowledging every record moves the read floor to the current end.
    let floor = files.journal.size();
    files
        .journal
        .ack(
            ShellFileAck {
                connection_epoch: 1,
                sequence,
            },
            Instant::now(),
        )
        .unwrap();
    assert!(
        matches!(
            files.journal.read(floor - 1, 1),
            Err(sophia_9p::Errno::ESTALE)
        ),
        "the floor is the acknowledged end"
    );
    j.floor = floor;
    j.publishes((2, 0), 0);
}

#[test]
fn a_begin_isb_refusal_after_the_combined_credit_cancels_the_ticket() {
    let mut j = Joined::new();
    // Fill the store's open transfers with replacement generations of a
    // lower id, so the copy's own resource stays fresh and only begin_isb,
    // after the combined credit, refuses it.
    let resources = j.f.epochs.resources_mut(identity().grant).unwrap();
    for generation in 1..=4 {
        resources
            .begin_isb(
                TransactionId::from_raw(200 + generation),
                ContentResourceBegin {
                    grant: identity().grant,
                    resource: ContentResourceId { id: 1, generation },
                    width_px: 1,
                    height_px: 1,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 4,
                },
                1,
            )
            .unwrap();
    }
    let usage = j.f.epochs.resources(identity().grant).unwrap().usage();
    j.f.submit(1);
    j.service();
    assert_eq!(j.take_events(), ["collect", "reserve", "cancel"]);
    assert!(j.probe.row.is_none());
    assert_eq!(j.owner.pending(), 0, "no job or work entry survives");
    assert_eq!(
        j.f.epochs.resources(identity().grant).unwrap().usage(),
        usage,
        "the refused copy holds no charge"
    );
    assert!(
        matches!(j.f.transport.output.front().unwrap().record, OutboundRecord::Frame(ShellFrameRecord::Refused { reason, .. }) if reason == ContentReason::Budget as u16)
    );
}

//! Generic peer for Sophia's supplied-admission WM export test. No desktop
//! policy, protected launch, Engine settlement or presentation claim.
use sophia_9p_client::pipeline::{IoInterest, PipelineLimits};
use sophia_wm_client::{
    journal::Event,
    protocol::{wm_files::*, *},
    submission::Custody,
    *,
};
use std::{
    os::{fd::BorrowedFd, unix::net::UnixStream},
    time::{Duration, Instant},
};

struct Clock(Instant);
impl Clock {
    fn now(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
    fn wait(&self, fd: BorrowedFd<'_>, interest: IoInterest, wake: Option<u64>) {
        let now = self.now();
        assert!(now < 20_000, "fixture deadline");
        if interest.immediate {
            return;
        }
        let millis = wake
            .unwrap_or(20_000)
            .min(20_000)
            .saturating_sub(now)
            .min(10);
        let mut flags = rustix::event::PollFlags::empty();
        if interest.readable {
            flags |= rustix::event::PollFlags::IN;
        }
        if interest.writable {
            flags |= rustix::event::PollFlags::OUT;
        }
        let mut fds = [rustix::event::PollFd::new(&fd, flags)];
        let timeout = rustix::event::Timespec::try_from(Duration::from_millis(millis)).unwrap();
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(_) => (),
            Err(rustix::io::Errno::INTR) => (),
            Err(e) => panic!("poll: {e}"),
        }
    }
    fn step(&self, s: &mut Session) {
        self.wait(s.fd().unwrap(), s.io_interest().unwrap(), s.wake_deadline());
        s.dispatch(self.now(), 65536).unwrap();
    }
    fn event(&self, s: &mut Session) -> Event {
        while s.event().is_none() {
            self.step(s);
        }
        let (header, event) = s.event().unwrap();
        assert_eq!(header.connection_epoch, 9);
        event.clone()
    }
    fn submit(&self, s: &mut Session, candidate: Candidate) {
        let ticket = loop {
            match s.submit(&candidate, 20_000) {
                Ok(id) => break id,
                Err(SessionError::Submission(submission::Error::Busy)) => self.step(s),
                Err(e) => panic!("submit: {e:?}"),
            }
        };
        loop {
            match s.custody(ticket) {
                Some(Custody::Submitted) => break,
                Some(Custody::Admitted | Custody::Issued) => self.step(s),
                other => panic!("custody: {other:?}"),
            }
        }
    }
}
fn main() {
    let clock = Clock(Instant::now());
    let path = std::env::args_os().nth(1).expect("socket path");
    let stream = UnixStream::connect(path).unwrap();
    let mut connecting = Connecting::direct(
        stream,
        PipelineLimits {
            msize: 4096,
            ..PipelineLimits::default()
        },
        WmFileNegotiationOffer {
            required_capabilities: (1 << 23) - 1,
            optional_capabilities: 0,
        },
        clock.now(),
        20_000,
    )
    .unwrap();
    while !connecting.ready() {
        clock.wait(
            connecting.fd().unwrap(),
            connecting.io_interest().unwrap(),
            connecting.wake_deadline(),
        );
        connecting.dispatch(clock.now(), 65536).unwrap();
    }
    let mut s = connecting.take_session().unwrap();
    for (transaction, prepare) in [(40, true), (41, false)] {
        let command = match clock.event(&mut s) {
            Event::ProfilePrepare(c) if prepare => c,
            Event::ProfileActivate(c) if !prepare => c,
            event => panic!("profile: {event:?}"),
        };
        assert_eq!(command.transaction.raw(), transaction);
        assert_eq!(command.identity.profile_generation, 3);
        assert_eq!(command.identity.profile_digest, [7; 32]);
        let completion = PolicyProfileCompletion {
            transaction: command.transaction,
            identity: command.identity,
            outcome: PolicyProfileOutcome::Accepted,
        };
        clock.submit(
            &mut s,
            if prepare {
                Candidate::ProfilePrepared(completion)
            } else {
                Candidate::ProfileActive(completion)
            },
        );
        s.consume().unwrap();
    }
    let mut chrome = WmChromePolicy::default();
    chrome.focus_ring.enabled = false;
    chrome.focus_ring.width = 0;
    chrome.frame.enabled = false;
    clock.submit(
        &mut s,
        Candidate::Configuration(WmFileConfiguration {
            transaction: TransactionId::from_raw(10),
            configuration: PolicyConfiguration {
                connection_epoch: 9,
                generation: 3,
                actions: vec![],
                chrome,
                action_lifecycles: vec![],
            },
        }),
    );
    assert!(
        matches!(clock.event(&mut s),Event::ConfigurationOutcome(v) if v.transaction.raw()==10 && v.generation==3 && v.outcome==PolicyProjectionOutcome::Committed)
    );
    s.consume().unwrap();
    clock.submit(
        &mut s,
        Candidate::Dirty(PolicyDirtyRequest {
            connection_epoch: 9,
            policy_generation: 3,
            affected_outputs: vec![OutputId::from_raw(1)],
        }),
    );
    let Event::Cycle(cycle) = clock.event(&mut s) else {
        panic!("Cycle missing")
    };
    assert_eq!(cycle.snapshot_transaction.raw(), 100);
    assert_eq!(cycle.request_transaction.raw(), 101);
    assert_eq!(cycle.request.request_id, 55);
    assert_eq!(cycle.request.scene_generation, 7);
    assert_eq!(cycle.request.policy_generation, 3);
    assert_eq!(cycle.request.cause, PolicyRequestCause::SceneChanged);
    assert_eq!(cycle.request.affected_outputs, [OutputId::from_raw(1)]);
    s.request_snapshot(20_000).unwrap();
    s.consume().unwrap();
    while s.snapshot_result().is_none() {
        clock.step(&mut s);
    }
    let snapshot = s.snapshot_result().unwrap();
    assert_eq!(snapshot.transaction.raw(), 100);
    let scene = &snapshot.snapshot.scene;
    assert_eq!(scene.generation, 7);
    assert_eq!(scene.active_output.raw(), 1);
    assert_eq!(scene.outputs.len(), 1);
    assert_eq!(scene.outputs[0].generation, 3);
    assert_eq!(scene.outputs[0].bounds.width, 100);
    assert_eq!(scene.outputs[0].work_area.height, 100);
    assert_eq!(scene.surfaces.len(), 64);
    for (i, surface) in scene.surfaces.iter().enumerate() {
        assert_eq!(surface.surface, SurfaceId::new(3 + i as u32, 1));
        assert_eq!(surface.generation, 8);
        assert_eq!(surface.current_output, Some(OutputId::from_raw(1)));
        assert_eq!(
            (surface.geometry.width, surface.geometry.height),
            (100, 100)
        );
    }
    s.release_snapshot().unwrap();
    clock.submit(
        &mut s,
        Candidate::Projection(Box::new(PolicyProjectionProposal {
            transaction: TransactionId::from_raw(11),
            connection_epoch: 9,
            request_id: 55,
            base_generation: 7,
            active_output: OutputId::from_raw(1),
            outputs: vec![PolicyOutputProjection {
                output: OutputId::from_raw(1),
                focus: Some(SurfaceId::new(3, 1)),
                placements: vec![PolicySurfacePlacement {
                    surface: SurfaceId::new(3, 1),
                    surface_generation: 8,
                    geometry: Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                    requested_size: None,
                    crop: None,
                    transform: PolicyTransform::Identity,
                    presentation: PolicyPresentationState::default(),
                }],
            }],
            indicators: vec![],
            output_statuses: vec![],
            tab_groups: vec![],
            translation_groups: vec![],
            launch_contexts: vec![],
            output_launch_contexts: vec![],
            presentation: None,
        })),
    );
    assert!(
        matches!(clock.event(&mut s),Event::ProjectionOutcome(v) if v.transaction.raw()==11 && v.request_id==55 && v.scene_generation==7 && v.outcome==PolicyProjectionOutcome::Committed && v.expect_session_operation)
    );
    s.consume().unwrap();
    clock.submit(
        &mut s,
        Candidate::SessionOperation(WmFileSessionOperation {
            transaction: TransactionId::from_raw(12),
            request: PolicySessionOperationRequest {
                connection_epoch: 9,
                request_id: 55,
                operation: 11,
                target: None,
            },
        }),
    );
    assert!(
        matches!(clock.event(&mut s),Event::SessionOperationOutcome(v) if v.transaction.raw()==12 && v.outcome.request_id==55 && v.outcome.outcome==PolicyProjectionOutcome::Committed)
    );
    s.consume().unwrap();
    let Event::PresentationReceipt(value) = clock.event(&mut s) else {
        panic!("receipt missing")
    };
    assert_eq!(value.transaction.raw(), 102);
    let receipt = value.receipt;
    assert_eq!(receipt.publication_generation, 1);
    assert_eq!(receipt.output.raw(), 1);
    assert_eq!(receipt.output_generation, 3);
    assert_eq!(receipt.presentation_epoch, 1);
    assert_eq!(receipt.outcome, PolicyPresentationOutcome::Presented);
    s.consume().unwrap();
    while s.obligations().acked != s.obligations().consumed || s.obligations().deadline.is_some() {
        clock.step(&mut s);
    }
    s.close();
    println!(
        "sophia_rust_wm_sdk status=pass profiles=2 snapshots=1 surfaces=64 projections=1 operations=1 receipts=1"
    );
}

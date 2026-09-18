//! Real r8 handshake, action FIFO and launch queue; supplied presentation and
//! protection evidence. No supervised child, process execution or native display.
use super::super::*;
use crate::application_catalog::*;
use crate::session_actions::{CatalogLaunchCause, SessionLaunchQueue};
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::Write;
#[path = "catalog_socket.rs"]
mod socket;
use socket::{GRANT, Peer, tx};

const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
    | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;

fn hello() -> ShellV1ClientHello {
    ShellV1ClientHello {
        minimum_revision: 8,
        maximum_revision: 8,
        required_capabilities: CAPS,
    }
}
fn publication() -> PublishedApplicationCatalog {
    let catalog = build_application_catalog(
        &sophia_config::ApplicationCatalogConfig {
            name: "dock".into(),
            sources: vec![],
            applications: vec!["terminal".into()],
            terminal: None,
            terminal_arguments: vec![],
        },
        &[RegisteredCatalogApplication {
            name: "terminal".into(),
            command: ApplicationLaunchCommand {
                executable: std::env::current_exe().unwrap(),
                arguments: vec![],
                working_directory: None,
            },
        }],
        &ApplicationCatalogEnvironment {
            search_path: vec![],
            locale: "C".into(),
            current_desktop: vec![],
        },
    )
    .unwrap();
    PublishedApplicationCatalog::new(GRANT.connection_epoch, 8, catalog).unwrap()
}
fn presented(target: PresentedContentTarget) -> sophia_engine::PresentedContentBinding {
    sophia_engine::PresentedContentBinding {
        grant: target.grant,
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        transform: sophia_engine::PresentedContentTransform {
            viewport: Rect {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
            layout_generation: 1,
        },
        authority_current: true,
        targets: vec![target],
        allocations: vec![],
    }
}
struct Harness {
    peer: Peer,
    epochs: ContentEpochRegistry,
    ledger: ContentActionLedger,
    publication: PublishedApplicationCatalog,
    presented: Vec<sophia_engine::PresentedContentBinding>,
    queue: SessionLaunchQueue,
    activation: CatalogActivation,
}
impl Harness {
    fn new() -> Self {
        let mut epochs = socket::empty();
        let mut peer = Peer::new(&mut epochs, ContentStoreProfile::PersistentCatalog);
        let welcome = peer
            .negotiate(&mut epochs, hello(), socket::granted())
            .unwrap();
        assert_eq!(welcome.selected_revision, 8);
        assert_eq!(welcome.capabilities, CAPS);
        assert_eq!(
            decode_shell_v1_server_welcome_frame(&peer.read()).unwrap(),
            welcome
        );
        assert!(matches!(
            decode_shell_content_frame(&peer.read()).unwrap().1,
            ShellContentRecord::Limits(_)
        ));
        assert!(
            peer.transport
                .connection(&mut epochs)
                .supports_persistent_catalog()
        );
        assert!(!peer.transport.supports_native_launcher());
        assert!(!peer.transport.supports_indicators());
        let mut target = super::super::tests::target();
        target.grant = GRANT;
        target.action_id = 1;
        let publication = publication();
        let mut ledger = ContentActionLedger::default();
        let event = ledger
            .issue_bound(
                target.clone(),
                0,
                &socket::limits(),
                tx(1),
                &mut peer.transport.connection(&mut epochs),
                ActionAuthority::Catalog(8),
            )
            .unwrap()
            .unwrap();
        peer.transport.poll_io(&mut epochs).unwrap();
        let (transaction, ShellContentRecord::Action(action)) =
            decode_shell_content_frame(&peer.read()).unwrap()
        else {
            panic!("issued action")
        };
        assert_eq!(transaction, tx(1));
        assert_eq!(action.event_id, event);
        Self {
            peer,
            epochs,
            ledger,
            publication,
            presented: vec![presented(target)],
            queue: SessionLaunchQueue::default(),
            activation: CatalogActivation {
                action,
                catalog_generation: 8,
            },
        }
    }
    fn activate(
        &mut self,
        activation: CatalogActivation,
        active: usize,
    ) -> CatalogActivationOutcome {
        self.peer
            .client
            .write_all(
                &encode_shell_catalog_action_frame(
                    tx(9),
                    &ShellCatalogActionRecord::Activate(activation.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        assert!(
            self.ledger
                .service_catalog_request(
                    &mut self.peer.transport.connection(&mut self.epochs),
                    &self.publication,
                    &self.presented,
                    &mut self.queue,
                    SessionApplicationId::from_raw(2),
                    active,
                    1
                )
                .unwrap()
        );
        self.peer.transport.poll_io(&mut self.epochs).unwrap();
        let (transaction, ShellCatalogActionRecord::ActivationOutcome(outcome)) =
            decode_shell_catalog_action_frame(&self.peer.read()).unwrap()
        else {
            panic!("outcome")
        };
        assert_eq!(transaction, tx(9));
        assert_eq!(outcome.activation, activation);
        outcome
    }
    fn ack(&mut self) {
        let a = &self.activation.action;
        self.peer
            .send_content(ShellContentRecord::ActionAck(ContentActionAck {
                grant: a.grant,
                output: a.output,
                candidate_generation: a.candidate_generation,
                presentation_epoch: a.presentation_epoch,
                interaction_generation: a.interaction_generation,
                allocation: a.allocation,
                target_id: a.target_id,
                target_generation: a.target_generation,
                action_id: a.action_id,
                event_id: a.event_id,
                disposition: 1,
            }));
        assert_eq!(
            self.ledger
                .service_acks(&mut self.peer.transport.connection(&mut self.epochs), 1, 32)
                .unwrap(),
            1
        );
    }
}

#[test]
fn catalog_click_admits_exact_queue_origin_once_in_both_ack_orders() {
    for ack_first in [false, true] {
        let mut h = Harness::new();
        if ack_first {
            h.ack();
        }
        assert_eq!(h.activate(h.activation.clone(), 0).status, 1);
        if !ack_first {
            h.ack();
        }
        assert_eq!(h.activate(h.activation.clone(), 0).status, 2);
        let intent = h.queue.begin_next(true).unwrap();
        assert!(h.queue.dispatch_catalog(intent.transaction));
        let launch = h.queue.take_native_catalog_dispatch().unwrap();
        assert_eq!(
            launch.cause,
            CatalogLaunchCause::Persistent(h.activation.clone())
        );
        assert_eq!(launch.entry.identity, "registered:terminal");
        assert!(h.queue.take_native_catalog_dispatch().is_none());
        h.queue.cancel_native_catalog(&launch);
        assert!(h.queue.begin_next(true).is_none());
        // Queue disposition cannot revive the already consumed pointer event.
        assert_eq!(h.activate(h.activation.clone(), 0).status, 2);
    }
}

#[test]
fn catalog_refuses_changed_echo_or_current_target_without_launch() {
    for mode in 0..11 {
        let mut h = Harness::new();
        let mut request = h.activation.clone();
        match mode {
            0 => request.catalog_generation += 1,
            1 => request.action.target_generation += 1,
            2 => request.action.candidate_generation += 1,
            3 => request.action.presentation_epoch += 1,
            4 => request.action.allocation.generation += 1,
            5 => h.presented[0].authority_current = false,
            6 => {
                h.presented[0].targets[0].continuity =
                    sophia_engine::ContentTargetContinuity::mint()
            }
            7 => h.presented[0].targets[0].bounds_px.width += 1,
            8 => h.ledger.live[0].authority = ActionAuthority::Indicator,
            9 => h.ledger.live[0].cancel_sent = true,
            _ => h.ledger.live[0].deadline_msec = 1,
        }
        assert_eq!(h.activate(request, 0).status, 2, "mode={mode}");
        assert!(h.queue.begin_next(true).is_none());
        assert_eq!(h.ledger.live[0].activation, ActivationState::Awaiting);
    }
}

#[test]
fn unchanged_presented_target_survives_new_raster_but_not_catalog_revision() {
    let mut h = Harness::new();
    h.presented[0].candidate_generation += 1;
    h.presented[0].presentation_epoch += 1;
    h.presented[0].targets[0].candidate_generation += 1;
    h.presented[0].targets[0].presentation_epoch += 1;
    assert_eq!(h.activate(h.activation.clone(), 0).status, 1);
    let h = Harness::new();
    let mut current = h.publication.wire().clone();
    current.generation += 1;
    assert_eq!(
        h.ledger
            .catalog_eligible(&h.activation, &current, &h.presented, 1),
        None
    );
}

#[test]
fn catalog_negotiation_requires_exact_role_and_explicit_input_policy() {
    for mode in 0..8 {
        let mut epochs = socket::empty();
        let profile = if mode == 7 {
            ContentStoreProfile::Legacy
        } else {
            ContentStoreProfile::PersistentCatalog
        };
        let mut peer = Peer::new(&mut epochs, profile);
        let mut request = hello();
        let mut policy = socket::granted();
        match mode {
            0 => request.required_capabilities |= SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER,
            1 => request.required_capabilities |= SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
            2 => request.required_capabilities &= !SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION,
            3 => {
                request.minimum_revision = 7;
                request.maximum_revision = 7;
            }
            4 => policy = ShellContentAdmissionPolicy::Denied,
            5 => {
                policy = ShellContentAdmissionPolicy::Granted {
                    discrete_input: false,
                }
            }
            6 => policy = ShellContentAdmissionPolicy::Unavailable,
            _ => {}
        }
        assert!(
            peer.negotiate(&mut epochs, request, policy).is_err(),
            "mode={mode}"
        );
        assert!(
            !peer
                .transport
                .connection(&mut epochs)
                .supports_persistent_catalog()
        );
    }
}

#[test]
fn capacity_refusal_consumes_only_the_exact_event_and_does_not_retry_launch() {
    let mut h = Harness::new();
    assert_eq!(
        h.activate(
            h.activation.clone(),
            crate::session_actions::SESSION_ACTION_APPLICATION_CAPACITY
        )
        .status,
        5
    );
    assert_eq!(h.ledger.live[0].activation, ActivationState::Rejected);
    assert_eq!(h.activate(h.activation.clone(), 0).status, 2);
    assert!(h.queue.begin_next(true).is_none());
}

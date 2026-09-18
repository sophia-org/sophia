use super::*;
use sophia_runtime::*;
use sophia_session::application_catalog::*;
#[allow(dead_code)] // Same real socket/store fixture, not a second implementation.
#[path = "../../../../sophia-runtime/tests/support/native_launcher_socket.rs"]
mod socket;
pub(super) use socket::*;

pub(super) struct Harness {
    pub epochs: ContentEpochRegistry,
    pub peer: Peer,
    pub service: NativeLauncherActionService,
    pub queue: SessionLaunchQueue,
    pub catalog: PublishedApplicationCatalog,
    pub focus: NativeLauncherBinding,
    pub target: PresentedContentTarget,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_command(ApplicationLaunchCommand {
            executable: std::env::current_exe().unwrap(),
            arguments: vec![],
            working_directory: None,
        })
    }
    pub fn with_command(command: ApplicationLaunchCommand) -> Self {
        let mut epochs = empty();
        let mut peer = Peer::connected(&mut epochs);
        let allocations = peer.allocation(&mut epochs);
        peer.upload(&mut epochs);
        // Inspect this executable as catalog input, never spawn it as an app.
        let registered = ["app1", "app2"].map(|name| RegisteredCatalogApplication {
            name: name.into(),
            command: command.clone(),
        });
        let catalog = build_application_catalog(
            &sophia_config::ApplicationCatalogConfig {
                name: "native-fixture".into(),
                sources: vec![],
                applications: vec!["app1".into(), "app2".into()],
                terminal: None,
                terminal_arguments: vec![],
            },
            &registered,
            &ApplicationCatalogEnvironment {
                search_path: vec![],
                locale: "C".into(),
                current_desktop: vec![],
            },
        )
        .unwrap();
        let catalog = PublishedApplicationCatalog::new(GRANT.connection_epoch, 8, catalog).unwrap();
        assert_eq!(catalog.wire(), &socket::catalog());
        peer.transport
            .grant_content_permit(&mut epochs, tx(3), OUTPUT, 1, 1, 0)
            .unwrap();
        peer.transport.poll_io(&mut epochs).unwrap();
        peer.read();
        peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
        peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
        peer.send_content(ShellContentRecord::CandidateEnd(end()));
        let current = native(catalog.wire());
        assert_eq!(
            peer.transport
                .service_native_launcher_content(&mut epochs, context(&allocations), current, 0)
                .unwrap(),
            3
        );
        let _bundle = peer
            .transport
            .begin_native_launcher_submission(&mut epochs, 1, context(&allocations), current, 0)
            .unwrap();
        peer.transport
            .content_prepared(&mut epochs, GRANT, OUTPUT, 1, 1, 1, 0)
            .unwrap();
        peer.transport
            .content_presented(&mut epochs, GRANT, OUTPUT, 1, 11, 1, 1)
            .unwrap();
        let focus = peer
            .transport
            .install_native_launcher_focus(&mut epochs, tx(4))
            .unwrap();
        peer.transport.poll_io(&mut epochs).unwrap();
        for kind in [1, 2] {
            assert!(
                matches!(decode_shell_content_frame(&peer.read()).unwrap().1,
                ShellContentRecord::CandidateOutcome(v) if v.kind == kind)
            );
        }
        assert_eq!(
            decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
            ShellNativeLauncherRecord::Focus(focus)
        );
        let wire_target = chunk().targets.remove(1); // slot 1, not selected slot 2
        let target = PresentedContentTarget {
            continuity: None,
            scale_generation: 4,
            grant: GRANT,
            output: OUTPUT,
            candidate_generation: focus.candidate_generation,
            presentation_epoch: focus.presentation_epoch,
            interaction_generation: focus.interaction_generation,
            allocation: focus.allocation,
            allocation_logical: allocations[0].logical,
            allocation_pixel: allocations[0].pixel,
            target_id: wire_target.target_id,
            target_generation: wire_target.target_generation,
            action_id: wire_target.action_id,
            bounds_px: wire_target.bounds_px,
        };
        Self {
            epochs,
            peer,
            service: NativeLauncherActionService::default(),
            queue: SessionLaunchQueue::default(),
            catalog,
            focus,
            target,
        }
    }
    pub fn accept(&mut self) -> NativeLauncherActivation {
        let event = self
            .peer
            .transport
            .issue_native_launcher_input(
                &self.epochs,
                self.focus,
                tx(10),
                NativeLauncherInputKind::Accept,
                "",
                1000,
            )
            .unwrap()
            .unwrap();
        self.peer.transport.poll_io(&mut self.epochs).unwrap();
        assert!(
            matches!(decode_shell_native_launcher_frame(&self.peer.read()).unwrap().1,
            ShellNativeLauncherRecord::Input(v) if v.event == event)
        );
        NativeLauncherActivation {
            event,
            cause: 1,
            slot: 2,
        }
    }
    pub fn acknowledge(&mut self, activation: NativeLauncherActivation, disposition: u16) -> bool {
        self.peer.send(ShellNativeLauncherRecord::InputAck(
            NativeLauncherInputAck {
                event: activation.event,
                disposition,
            },
        ));
        self.peer
            .transport
            .poll_native_launcher_input_ack(&mut self.epochs)
            .unwrap()
            .unwrap()
            .2
    }
    pub fn service(&mut self, active: usize) -> bool {
        self.service
            .service_request(
                &mut self.peer.transport.connection(&mut self.epochs),
                &self.catalog,
                &mut self.queue,
                SessionApplicationId::from_raw(2),
                active,
                1001,
                1,
            )
            .unwrap()
    }
    pub fn outcome(&mut self) -> NativeLauncherActivationOutcome {
        self.peer.transport.poll_io(&mut self.epochs).unwrap();
        let (_, ShellNativeLauncherRecord::ActivationOutcome(outcome)) =
            decode_shell_native_launcher_frame(&self.peer.read()).unwrap()
        else {
            panic!("outcome");
        };
        outcome
    }
    pub fn activate(
        &mut self,
        activation: NativeLauncherActivation,
        active: usize,
    ) -> NativeLauncherActivationOutcome {
        self.peer
            .send(ShellNativeLauncherRecord::Activate(activation));
        assert!(self.service(active));
        let outcome = self.outcome();
        assert_eq!(outcome.activation, activation);
        outcome
    }
    pub fn dispatch(
        &mut self,
    ) -> std::sync::Arc<sophia_session::session_actions::NativeCatalogLaunch> {
        let intent = self.queue.begin_next(true).unwrap();
        assert!(self.queue.dispatch_catalog(intent.transaction));
        assert!(self.queue.take_catalog_dispatch().is_none());
        let payload = self.queue.take_native_catalog_dispatch().unwrap();
        assert_eq!(payload.transaction, intent.transaction);
        assert!(self.queue.take_native_catalog_dispatch().is_none());
        payload
    }
}

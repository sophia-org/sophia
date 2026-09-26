use sophia_protocol::inspection::{
    InspectionEvent, InspectionOutput, InspectionRect, InspectionSnapshot, InspectionState,
    InspectionSurface, InspectionSurfaceId, InspectionWire,
};
use sophia_runtime::inspection::{
    InspectionError, InspectionPublisher, InspectionService, PublishOutcome,
};

struct LivePolicyInspection {
    service: InspectionService,
    publisher: InspectionPublisher,
    session_generation: u64,
    epoch: u64,
    excluded_pid: Option<u32>,
    capabilities: Option<u64>,
    last: Option<InspectionSignature>,
    refused: Option<InspectionSignature>,
    pending_event: Option<InspectionEvent>,
    retry: bool,
    refusals: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct InspectionSignature {
    scene_generation: u64,
    commit_serial: u64,
    selected_capabilities: Option<u64>,
    configured: bool,
    unavailable: bool,
}

impl LiveWmSession {
    fn start_inspection(
        &mut self,
        config: &mut PersistentXtermSessionConfig,
        session_generation: u64,
    ) {
        config.inspection_socket = None;
        if config.inspection_access != sophia_config::DesktopInspectionAccess::HostAdmin {
            return;
        }
        let Some(public) = self.public.as_mut() else {
            return;
        };
        let result = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| std::io::Error::other("XDG_RUNTIME_DIR is required for inspection"))
            .and_then(|path| InspectionService::bind(std::path::Path::new(&path)));
        let mut service = match result {
            Ok(service) => service,
            Err(error) => {
                tracing::warn!(target: "sophia_inspection", status = "disabled", %error);
                return;
            }
        };
        if let Err(error) = service.fence(public.connection_epoch, self.supervisor.peer_id()) {
            tracing::warn!(target: "sophia_inspection", status = "disabled", %error);
            return;
        }
        config.inspection_socket = Some(service.socket_path().to_path_buf());
        public.inspection = Some(LivePolicyInspection {
            publisher: service.publisher(),
            service,
            session_generation,
            epoch: public.connection_epoch,
            excluded_pid: self.supervisor.peer_id(),
            capabilities: public
                .transport_ready
                .then_some(public.selected_capabilities),
            last: None,
            refused: None,
            pending_event: Some(InspectionEvent::ConnectionChanged),
            retry: true,
            refusals: 0,
        });
        public.publish_inspection();
        tracing::info!(target: "sophia_inspection", status = "enabled", access = "host-admin");
    }

    fn service_inspection(&mut self, stopping: bool) {
        let Some(public) = self.public.as_mut() else {
            return;
        };
        if stopping
            || public
                .inspection
                .as_ref()
                .is_some_and(|inspection| !inspection.service.is_running())
        {
            // Dropping the separate service revokes its fids and pending reads.
            public.inspection.take();
            return;
        }
        if self.degraded || self.control_restart.is_some() || public.transport_unavailable {
            public.fence_inspection(0, self.supervisor.peer_id());
        }
        // Retry a lost publication on the next owner turn without requesting
        // new policy work or retaining an unbounded observation queue.
        public.publish_inspection();
    }
}

impl LivePublicPolicyState {
    fn fence_inspection(&mut self, epoch: u64, excluded_pid: Option<u32>) {
        let Some(mut inspection) = self.inspection.take() else {
            return;
        };
        // Keep the last protected peer excluded throughout disconnection too.
        let excluded_pid = excluded_pid.or(inspection.excluded_pid);
        if let Err(error) = inspection.service.fence(epoch, excluded_pid) {
            tracing::warn!(target: "sophia_inspection", status = "disabled", %error);
            return;
        }
        let changed = inspection.epoch != epoch || inspection.excluded_pid != excluded_pid;
        inspection.epoch = epoch;
        inspection.excluded_pid = excluded_pid;
        if changed {
            inspection.capabilities = None;
            inspection.last = None;
            inspection.refused = None;
            inspection.pending_event = Some(InspectionEvent::ConnectionChanged);
            inspection.retry = true;
        }
        self.inspection = Some(inspection);
        if changed {
            self.publish_inspection();
        }
    }

    fn note_inspection_event(&mut self, event: Option<InspectionEvent>) {
        let (Some(inspection), Some(event)) = (self.inspection.as_mut(), event) else {
            return;
        };
        // Notifications summarize an owner turn, not a ledger of commands.
        // Mixed transitions ask the reader to consult the newest snapshot.
        inspection.pending_event = Some(match inspection.pending_event {
            None => event,
            Some(previous) if previous == event => event,
            Some(_) => InspectionEvent::SnapshotChanged,
        });
    }

    fn publish_inspection(&mut self) {
        let Some(inspection) = self.inspection.as_ref() else {
            return;
        };
        let signature = InspectionSignature {
            scene_generation: self.reducer.scene().generation,
            commit_serial: self.reducer.commit_serial(),
            selected_capabilities: inspection.capabilities,
            configured: self.configured,
            unavailable: self.transport_unavailable,
        };
        if inspection.refused == Some(signature) {
            self.inspection.as_mut().unwrap().pending_event = None;
            return;
        }
        if inspection.pending_event.is_none()
            && !inspection.retry
            && inspection.last == Some(signature)
        {
            return;
        }
        let snapshot = inspection_snapshot(self, inspection.session_generation, inspection.epoch);
        let inspection = self.inspection.as_mut().expect("inspection owner retained");
        let event = inspection.pending_event.take();
        inspection.refused = None;
        let result = inspection.publisher.publish(snapshot, event);
        if inspection.record_publication(signature, event, result) {
            self.inspection.take();
        }
    }
}

impl LivePolicyInspection {
    /// Returns true when the separate service must be retired.
    fn record_publication(
        &mut self,
        signature: InspectionSignature,
        event: Option<InspectionEvent>,
        result: Result<PublishOutcome, InspectionError>,
    ) -> bool {
        match result {
            Ok(PublishOutcome::Published { .. } | PublishOutcome::Unchanged) => {
                self.last = Some(signature);
                self.retry = false;
                false
            }
            Ok(PublishOutcome::Busy { .. }) => {
                self.pending_event = event;
                self.retry = true;
                false
            }
            Err(error) => {
                self.retry = matches!(error, InspectionError::Fenced);
                if self.retry {
                    self.pending_event = event;
                }
                if matches!(error, InspectionError::Record(_)) {
                    // The runtime already exposed this refusal as loss. Wait
                    // for changed owner facts instead of rebuilding it each turn.
                    self.refused = Some(signature);
                }
                self.refusals = self.refusals.saturating_add(1);
                if self.refusals.is_power_of_two() {
                    tracing::warn!(target: "sophia_inspection", status = "publication_refused",
                        refusals = self.refusals, %error);
                }
                if matches!(
                    error,
                    InspectionError::Stopped
                        | InspectionError::Exhausted
                        | InspectionError::Poisoned
                ) {
                    tracing::warn!(target: "sophia_inspection", status = "disabled", %error);
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// The last complete Session scene supplied to spatial policy, not a physical
/// presentation claim. Explicit construction is the live disclosure boundary:
/// session operation tokens, policy keys, metadata and catalogs are omitted.
fn inspection_snapshot(
    public: &LivePublicPolicyState,
    session_generation: u64,
    epoch: u64,
) -> InspectionSnapshot {
    let scene = public.reducer.scene();
    let available = epoch != 0;
    let capabilities = public
        .inspection
        .as_ref()
        .and_then(|inspection| inspection.capabilities);
    InspectionSnapshot {
        session_generation,
        wm_epoch: epoch,
        scene_generation: if available { scene.generation } else { 0 },
        selected_capabilities: if available {
            capabilities.unwrap_or(0)
        } else {
            0
        },
        wire: match public.wm_transport {
            WmTransportSelection::CurrentIpc => InspectionWire::CurrentIpc,
            WmTransportSelection::NineP2000L => InspectionWire::Files,
        },
        state: if !available || public.transport_unavailable {
            InspectionState::Unavailable
        } else if public.configured && capabilities.is_some() {
            InspectionState::Ready
        } else {
            InspectionState::Starting
        },
        outputs: if available {
            scene
                .outputs
                .iter()
                .map(|output| InspectionOutput {
                    id: output.output.raw(),
                    generation: output.generation,
                    geometry: inspection_rect(output.bounds),
                    work_area: inspection_rect(output.work_area),
                    focus: output.focus.map(inspection_surface_id),
                })
                .collect()
        } else {
            Vec::new()
        },
        surfaces: if available {
            scene
                .surfaces
                .iter()
                .map(|surface| InspectionSurface {
                    id: inspection_surface_id(surface.surface),
                    state_generation: surface.generation,
                    output: surface.current_output.map(sophia_protocol::OutputId::raw),
                    geometry: inspection_rect(surface.geometry),
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

fn inspection_rect(rect: Rect) -> InspectionRect {
    // PolicyProjectionReducer validates strictly positive scene extents,
    // including output work areas, before this view can observe them.
    InspectionRect {
        x: rect.x,
        y: rect.y,
        width: rect.width as u32,
        height: rect.height as u32,
    }
}

fn inspection_surface_id(surface: SurfaceId) -> InspectionSurfaceId {
    InspectionSurfaceId {
        index: surface.index(),
        generation: surface.generation(),
    }
}

// These are owner reports, not proof that the peer consumed a command or that
// an application executed an accepted intent. No action identity crosses here.
fn inspection_command_event(command: &PolicyTransportCommand) -> Option<InspectionEvent> {
    use PolicyTransportCommand as C;
    use sophia_protocol::PolicyProjectionOutcome as O;
    match command {
        C::ConfigurationOutcome { outcome, .. } => {
            (*outcome != O::Committed).then_some(InspectionEvent::ConfigurationRejected)
        }
        C::ProjectionOutcome { outcome, .. } => Some(match outcome {
            O::Committed => InspectionEvent::ProjectionCommitted,
            O::TimedOut => InspectionEvent::ProjectionTimedOut,
            _ => InspectionEvent::ProjectionRejected,
        }),
        C::SessionOperationOutcome { outcome, .. } => Some(if *outcome == O::Committed {
            InspectionEvent::SessionOperationAccepted
        } else {
            InspectionEvent::SessionOperationRejected
        }),
        C::PresentationReceipt { .. } => Some(InspectionEvent::PresentationChanged),
        C::Cycle { .. } | C::Stop => None,
    }
}

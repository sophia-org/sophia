/// Why a frontend could not become an execution runner. The frontend is
/// returned intact; no producer is exposed by a failed preparation.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRunnerRefusal {
    ProducerAlreadyExposed,
    Keyboard(PrivateKeyboardsRefusal),
    StateUnavailable,
}

/// One executing thread owns the frontend and its continuing keyboard history.
/// Preparation completes before this value can expose a reserving ingress.
/// Dropping it drops the frontend first, handing obligations to its durable
/// owner, before the thread-local keyboard state is destroyed.
///
/// This value cannot be sent to a different executing thread:
/// ```compile_fail
/// fn move_runner(runner: sophia_x_authority::PrivatePreparedRunner) {
///     std::thread::spawn(move || drop(runner));
/// }
/// ```
#[cfg(unix)]
pub struct PrivatePreparedRunner {
    frontend: Option<PrivateXServerFrontend>,
    keyboards: PrivateKeyboards,
    namespace: NamespaceId,
    seat: SeatId,
}

/// Processing, enqueue and settlement are counted separately. An enqueued
/// event is not evidence that a writer flushed it.
#[cfg(unix)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PrivateRunnerProgress {
    pub taken: usize,
    pub refused: usize,
    pub enqueued: usize,
    pub observed: usize,
    pub settled: usize,
    pub blocked: Option<crate::ReadySequence>,
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Prepare on the thread that will execute turns, before granting an
    /// ingress. The instance's issuer supplies the seat; callers cannot choose
    /// a different seat for the same authority.
    #[allow(clippy::result_large_err)] // Refusal returns the caller's owned frontend without boxing.
    pub fn prepare_runner(
        self,
        namespace: NamespaceId,
    ) -> Result<PrivatePreparedRunner, (PrivateRunnerRefusal, Self)> {
        if self.ordered_runner {
            return Err((PrivateRunnerRefusal::ProducerAlreadyExposed, self));
        }
        // Installation takes common itself and binds the actual connection
        // projections before any producer can reserve against this runner.
        if self
            .broker
            .registry
            .install_private_applied(&self.controller, namespace)
            .is_err()
        {
            return Err((PrivateRunnerRefusal::StateUnavailable, self));
        }
        let seat = self.submit.binding().seat();
        // These allocations initialize the supported namespace before any
        // execution can consume its state. No keymap work happens under common.
        let prepared = self.controller.under_common(|_| {
            let mut pointers = self.broker.registry.pointer_state.lock().map_err(|_| ())?;
            let mut grabs = self
                .broker
                .registry
                .input_authority
                .lock()
                .map_err(|_| ())?;
            pointers
                .entry((namespace, seat))
                .or_insert_with(crate::XCorePointerMapper::new);
            grabs.prepare_ordered_namespace(namespace);
            Ok::<(), ()>(())
        });
        if !matches!(prepared, Ok(Ok(()))) {
            return Err((PrivateRunnerRefusal::StateUnavailable, self));
        }
        let mut keyboards = match self.keyboards() {
            Ok(keyboards) => keyboards,
            Err(refusal) => return Err((PrivateRunnerRefusal::Keyboard(refusal), self)),
        };
        if !keyboards.prepare(seat) {
            // The history never escaped and never executed an input.
            self.keyboards_issued.store(false, Ordering::Release);
            return Err((
                PrivateRunnerRefusal::Keyboard(PrivateKeyboardsRefusal::KeymapUnavailable),
                self,
            ));
        }
        Ok(PrivatePreparedRunner {
            frontend: Some(self),
            keyboards,
            namespace,
            seat,
        })
    }
}

#[cfg(unix)]
impl PrivatePreparedRunner {
    pub fn seat(&self) -> SeatId {
        self.seat
    }
    pub fn namespace(&self) -> NamespaceId {
        self.namespace
    }

    pub fn admission_participant(&self) -> &PrivateAdmissionParticipant {
        &self.frontend.as_ref().expect("live runner").participant
    }

    pub fn control_producer(&self) -> PrivateControlProducer {
        self.frontend
            .as_ref()
            .expect("live runner")
            .control_producer()
    }

    pub fn ingress_for(
        &mut self,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateIngress, PrivateAdmissionRefusal> {
        self.frontend
            .as_mut()
            .expect("live runner")
            .ingress_for(client, device)
    }

    /// Consume the actual shared order using this runner's continuing state.
    /// Writer settlement is supplied by the terminal owner, not inferred from
    /// a turn returning or from a successful queue handoff.
    pub fn service_turn(&mut self) -> Result<PrivateRunnerProgress, XServerFrontendRouteError> {
        let frontend = self.frontend.as_mut().expect("live runner");
        let items = frontend.route_pending_ordered(&mut self.keyboards)?;
        let mut progress = PrivateRunnerProgress {
            taken: items.len(),
            refused: items
                .iter()
                .filter(|item| matches!(item, PrivateOrderedItem::Refused { .. }))
                .count(),
            ..PrivateRunnerProgress::default()
        };
        for delivered in frontend.deliver_turn(items) {
            progress.enqueued += usize::from(delivered.enqueued);
            progress.observed += usize::from(delivered.completion.is_some());
            progress.settled += usize::from(delivered.debt_settled);
        }
        progress.blocked = frontend.blocked();
        Ok(progress)
    }

    pub fn shutdown(mut self) -> PrivateSettlement {
        self.frontend.take().expect("live runner").shutdown()
    }
}

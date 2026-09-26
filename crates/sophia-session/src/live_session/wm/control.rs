// Linux parent-death signaling follows the spawning thread. A protected WM
// must retain its launch thread even after its resources return to the owner.
struct ControlProcessLifetime {
    stop: Option<std::sync::mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ControlProcessLifetime {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct ControlRestartJob {
    completion: std::sync::mpsc::Receiver<ControlRestartResult>,
    lifetime: ControlProcessLifetime,
}

struct ControlRestartResult {
    supervisor: ProcessSupervisor,
    output_service: Option<sophia_runtime::OutputTransportService>,
    result: Result<(PolicyTransportWorker, sophia_runtime::SupervisorEvent), String>,
    epoch: u64,
    abandoned_output: bool,
}

impl LiveWmSession {
    fn begin_control_restart(
        &mut self,
        _output: sophia_engine::HeadlessOutput,
    ) -> Result<u64, String> {
        let public = self.public.as_mut().ok_or("policy unavailable")?;
        if self.control_restart.is_some() || self.desktop_reload.is_some()
            || self.force_transport_restart || self.pending_policy_launch_spec.is_some()
        {
            return Err("restart or profile replacement already active".into());
        }
        let epoch = public.next_connection_epoch;
        public.next_connection_epoch = epoch.checked_add(1).ok_or("WM epoch exhausted")?;
        let placeholder = ProcessSupervisor::new(
            self.supervisor.process(),
            self.supervisor.launch_spec().clone(),
        );
        let mut supervisor = std::mem::replace(&mut self.supervisor, placeholder);
        let old_worker = public.worker.take();
        let old_lifetime = self.control_lifetime.take();
        let output_service = public.output_service.take();
        let endpoint = public.directory.endpoint_path();
        let profile_key = public.profile_key;
        let wm_transport = public.wm_transport;
        let wm_filesystem_qids = public.wm_filesystem_qids.clone();
        let native_presentation_capable = public.native_presentation_capable;
        let (state, command) = update_supervisor(
            self.supervisor_state.clone(),
            SupervisorEvent::ProcessExited,
            self.restart_policy,
        );
        self.supervisor_state = state;
        public.transport_unavailable = true;
        public.configured = false;
        self.pending_policy_configuration = None;
        self.force_transport_restart = false;
        self.restarts = self.restarts.saturating_add(1);
        let _ = public.reducer.disconnect(public.connection_epoch);
        let (completion, results) = std::sync::mpsc::sync_channel(1);
        let (stop, stopped) = std::sync::mpsc::sync_channel::<()>(1);
        let job = std::thread::Builder::new().name("sophia-wm-restart".into()).spawn(move || {
            let mut abandoned_output = false;
            let result = (|| -> Result<_, String> {
                supervisor.terminate().map_err(|e| e.to_string())?;
                drop(old_worker);
                drop(old_lifetime);
                if let Some(service) = &output_service { abandoned_output = !service.pause_acceptance(Duration::from_secs(1))?.is_empty(); }
                let mut transport = bind_public_policy_endpoint(&endpoint, profile_key, wm_transport).map_err(|e| e.to_string())?;
                let started = supervisor.apply(command).map_err(|e| e.to_string())?.ok_or("supervisor declined restart")?;
                let pid = supervisor.peer_id().ok_or("replacement has no peer")?;
                transport.authorize(&supervisor).map_err(|e| e.to_string())?;
                if let Some(service) = &output_service {
                    service.command(sophia_runtime::OutputTransportServiceCommand::ReplaceSupervisedPid { pid }).map_err(|_| "output service unavailable")?;
                }
                let worker = start_public_policy_worker(transport, epoch, profile_key, native_presentation_capable, &supervisor, wm_filesystem_qids).map_err(|e| e.to_string())?;
                Ok((worker, started))
            })();
            if result.is_err() { let _ = supervisor.terminate(); }
            let keep_alive = result.is_ok();
            let _ = completion.send(ControlRestartResult { supervisor, output_service, result, epoch, abandoned_output });
            if keep_alive { let _ = stopped.recv(); }
        });
        match job {
            Ok(job) => {
                self.control_restart = Some(ControlRestartJob {
                    completion: results,
                    lifetime: ControlProcessLifetime {
                        stop: Some(stop),
                        thread: Some(job),
                    },
                });
                Ok(epoch)
            }
            Err(error) => {
                self.degraded = true;
                Err(error.to_string())
            }
        }
    }

    fn poll_control_restart(
        &mut self,
        layout: &mut PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) {
        let Some(job) = self.control_restart.as_ref() else {
            return;
        };
        let result = match job.completion.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.control_restart.take();
                self.degraded = true;
                return;
            }
        };
        let job = self.control_restart.take().unwrap();
        self.control_lifetime = Some(job.lifetime);
        self.supervisor = result.supervisor;
        let public = self
            .public
            .as_mut()
            .expect("restarting policy retains its owner");
        public.output_service = result.output_service;
        if result.abandoned_output && public.abandon_output_candidate().is_err() {
            self.degraded = true;
            return;
        }
        match result.result {
            Ok((worker, started)) => {
                let (state, _) =
                    update_supervisor(self.supervisor_state.clone(), started, self.restart_policy);
                self.supervisor_state = state;
                if public.reducer.connect(result.epoch).is_err() {
                    self.degraded = true;
                    return;
                }
                public.worker = Some(worker);
                public.connection_epoch = result.epoch;
                public.configured = false;
                public.negotiated = false;
                public.selected_capabilities = 0;
                if let Ok(mut origins) = public.launch_origins.lock() { origins.set_epoch(0); }
                public.cycle_submitted = false;
                public.transport_ready = false;
                public.in_flight_request = None;
                public.in_flight_source = None;
                public.staged = None;
                public.prepared = None;
                public.pending_operation = None;
                public.expected_operation_slot = None;
                public.deferred_command = None;
                public.transport_unavailable = false;
                public.actions.clear();
                public.queue.clear();
                public.pending_dirty_outputs.clear();
                layout.rearm_manage_settlements();
                public.queue.push_back(LivePublicPolicyCause {
                    source: LiveWmProposalSource::Relayout,
                    cause: sophia_protocol::PolicyRequestCause::SceneChanged,
                    affected_outputs: public.all_outputs(output.id),
                });
                crate::session_println!(
                    "sophia_control schema=1 status=restart_admitted epoch={} completion=pending",
                    result.epoch
                );
            }
            Err(error) => {
                self.degraded = true;
                crate::session_eprintln!(
                    "sophia_control schema=1 status=restart_failed preserved_layout=true reason={error}"
                );
            }
        }
    }
}

struct LiveControlState {
    service: Option<sophia_runtime::ControlService>,
    catalog: std::sync::Arc<sophia_protocol::ControlCatalog>,
    signature: Option<(u64, u64, bool)>,
    next: Option<sophia_runtime::ControlTicket>,
    restarting: Option<(sophia_runtime::ControlTicket, u64, usize)>,
    published: bool,
}

impl LiveControlState {
    fn start(config: &mut PersistentXtermSessionConfig) -> Self {
        let service = if config.control_access == sophia_config::DesktopControlAccess::HostAdmin {
            let result = std::env::var_os("XDG_RUNTIME_DIR")
                .ok_or_else(|| {
                    std::io::Error::other("XDG_RUNTIME_DIR is required for host control")
                })
                .and_then(|path| sophia_runtime::ControlService::bind(std::path::Path::new(&path)));
            match result {
                Ok(service) => {
                    config.control_socket = Some(service.socket_path().to_path_buf());
                    crate::session_println!(
                        "sophia_control schema=1 status=enabled access=host-admin socket={}",
                        service.socket_path().display()
                    );
                    Some(service)
                }
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_control schema=1 status=disabled reason={error}"
                    );
                    None
                }
            }
        } else {
            None
        };
        Self {
            service,
            catalog: std::sync::Arc::new(sophia_protocol::ControlCatalog {
                generation: 1,
                commands: Vec::new(),
            }),
            signature: None,
            next: None,
            restarting: None,
            published: false,
        }
    }

    fn service(
        &mut self,
        wm: Option<&mut LiveWmSession>,
        layout: &PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
        stopping: bool,
    ) {
        use sophia_protocol::{ControlCommand, ControlOutcome as O, ControlOwner};
        let Some(service) = self.service.as_ref() else {
            return;
        };
        if stopping || !service.is_running() {
            if let Some(ticket) = self.next.take() {
                ticket.finish(O::Unavailable);
            }
            if let Some((ticket, _, _)) = self.restarting.take() {
                ticket.finish(O::Indeterminate);
            }
            // Service shutdown has no peer-dependent wait; revoke all queued tickets.
            self.service.take();
            return;
        }
        let Some(wm) = wm else {
            return;
        };
        let Some(public) = wm.public.as_mut() else {
            return;
        };
        let ready = public.configured
            && !wm.degraded
            && wm.control_restart.is_none()
            && !public.transport_unavailable;
        let signature = (
            public.connection_epoch,
            public.control_catalog_serial,
            ready,
        );
        if self.signature.as_ref() != Some(&signature) {
            let Some(generation) = self.catalog.generation.checked_add(1) else {
                self.service.take();
                return;
            };
            let mut commands = if ready {
                public
                    .actions
                    .iter()
                    .filter(|action| action.session_operation_slot.is_none())
                    .map(|action| ControlCommand {
                        owner: ControlOwner::Policy,
                        name: action.name.clone(),
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if ready {
                commands.push(ControlCommand {
                    owner: ControlOwner::Session,
                    name: "restart-wm".to_owned(),
                });
            }
            commands.sort();
            self.catalog = std::sync::Arc::new(sophia_protocol::ControlCatalog {
                generation,
                commands,
            });
            public.control_generation = generation;
            self.signature = Some(signature);
            self.published = false;
        }
        if !self.published {
            self.published = service.publish(
                self.catalog.clone(),
                &wm.supervisor.peer_id().into_iter().collect::<Vec<_>>(),
            );
        }
        if let Some((ticket, epoch, committed)) = self.restarting.take() {
            if wm.degraded || (wm.control_restart.is_none() && public.connection_epoch != epoch) {
                ticket.finish(O::Indeterminate);
            } else if ready && public.connection_epoch == epoch && wm.committed > committed {
                ticket.finish(O::Completed);
            } else {
                self.restarting = Some((ticket, epoch, committed));
            }
            return;
        }
        if self.next.is_none() {
            self.next = service.try_request();
        }
        let Some(ticket) = self.next.take() else {
            return;
        };
        if ticket.cancelled() {
            return;
        }
        if ticket.generation != self.catalog.generation {
            ticket.finish(O::Stale);
            return;
        }
        if !ready {
            ticket.finish(O::Unavailable);
            return;
        }
        match ticket.command.owner {
            ControlOwner::Policy => {
                let action = public
                    .actions
                    .iter()
                    .find(|a| a.name == ticket.command.name && a.session_operation_slot.is_none())
                    .map(|a| a.action);
                let Some(action) = action else {
                    ticket.finish(O::Rejected);
                    return;
                };
                if public.queue.len() >= WM_OWNER_REQUEST_CAPACITY {
                    ticket.finish(O::Overloaded);
                    return;
                }
                let Ok(serial) = public.mint_transaction() else {
                    ticket.finish(O::Unavailable);
                    return;
                };
                let cause = LivePublicPolicyCause {
                    source: LiveWmProposalSource::Action(action),
                    cause: sophia_protocol::PolicyRequestCause::Action {
                        activation_serial: serial.raw(),
                        action,
                    },
                    affected_outputs: public.all_outputs(public.active_output),
                };
                if public.queue_cause(cause) == LiveWmRequestAdmission::Admitted {
                    public.control_tickets.insert(serial.raw(), ticket);
                } else {
                    ticket.finish(O::Overloaded);
                }
            }
            ControlOwner::Session => {
                if ticket.command.name != "restart-wm" {
                    ticket.finish(O::Rejected);
                    return;
                }
                // A lifecycle barrier follows already queued policy work.
                if layout.pending.is_some()
                    || public.in_flight_request.is_some()
                    || !public.queue.is_empty()
                    || public.output_effect_dispatched
                    || public.output_topology_effect_pending()
                    || !public.ordinary_policy_settlement_idle()
                    || !public.control_tickets.is_empty()
                {
                    self.next = Some(ticket);
                    return;
                }
                if !ticket.claim() {
                    if !ticket.cancelled() {
                        self.next = Some(ticket);
                    }
                    return;
                }
                let committed = wm.committed;
                match wm.begin_control_restart(output) {
                    Ok(epoch) => self.restarting = Some((ticket, epoch, committed)),
                    Err(error) => {
                        ticket.finish(O::Indeterminate);
                        crate::session_eprintln!(
                            "sophia_control schema=1 status=restart_failed reason={error}"
                        );
                    }
                }
            }
        }
    }
}

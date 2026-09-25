// Dispatch policy intents in their original physical-input order.
{
macro_rules! dispatch_physical_policy_inputs {
    ($report:expr) => {{
        let report = $report;
            physical_policy_inputs.synchronize(wm_session.as_ref().and_then(|wm| wm.public.as_ref().map(|p| p.connection_epoch)));
            if report.presentation_capacity_exceeded
                && let Some(public) = wm_session.as_mut().and_then(|wm| wm.public.as_mut()) {
                public.revoke_live_presentation();
            }
            let hover_enabled = wm_session.as_ref().is_some_and(LiveWmSession::pointer_focus_enabled);
            for input in report.policy_inputs.iter().copied() {
                if !physical_policy_inputs.push(input, hover_enabled) {
                    if matches!(input, PhysicalPolicyInput::PresentedAction(_))
                        && let Some(public) = wm_session.as_mut().and_then(|wm| wm.public.as_mut()) {
                        public.revoke_live_presentation();
                    }
                    crate::session_eprintln!("sophia_live_wm schema=4 status=input_rejected reason=capacity");
                }
            }
            while let Some(input) = physical_policy_inputs.next(wm_session.as_ref().is_some_and(LiveWmSession::pointer_focus_pending)) {
                let action = match input {
                    PhysicalPolicyInput::PresentedAction(action) => {
                        if let Some(wm) = wm_session.as_mut() { wm.enqueue_presented_action(action)?; }
                        continue;
                    }
                    PhysicalPolicyInput::Hover(observation) => {
                        if let Some(wm) = wm_session.as_mut() {
                            wm.enqueue_pointer_focus(observation);
                        }
                        continue;
                    }
                    PhysicalPolicyInput::ClickFocus(surface) => {
                        // A SESSION WITHOUT A WM HAS NO FOCUS TO CHANGE. The
                        // hover arm above already reads the absence that way
                        // and continues; this arm made it fatal, which killed
                        // every session configured for a pointer proof with no
                        // window manager. That is the QEMU scenario exactly --
                        // it passes `--expect-physical-pointer` and starts no
                        // WM -- so the session announced `pointer status=ready
                        // action=select`, the harness sent the click it had
                        // just asked for, and the session died on the answer.
                        // Dropping the request costs the proof nothing: focus
                        // policy is not what a pointer proof measures, and the
                        // button still routes to the client through the
                        // authority, which is what moves the pixels it reads.
                        let Some(wm) = wm_session.as_mut() else {
                            crate::session_eprintln!(
                                "sophia_live_wm schema=3 status=request_rejected source=pointer_focus reason=no_wm_session surface={}",
                                surface.index(),
                            );
                            continue;
                        };
                        match wm.enqueue_focus(surface, &layout, output)? {
                            LiveWmRequestAdmission::Admitted => {
                                crate::session_println!(
                                    "sophia_live_wm schema=3 status=focus_requested source=pointer surface={}",
                                    surface.index(),
                                );
                            }
                            LiveWmRequestAdmission::Duplicate => {}
                            LiveWmRequestAdmission::RejectedCapacity => {
                                crate::session_eprintln!(
                                    "sophia_live_wm schema=3 status=request_rejected source=pointer_focus reason=capacity surface={}",
                                    surface.index(),
                                );
                            }
                        }
                        continue;
                    }
                    PhysicalPolicyInput::Action(action) => action,
                };

                if is_reserved_session_action(action)
                    && action != SHELL_HELP_SHORTCUT_ACTION
                    && !is_shell_switcher_shortcut(action)
                {
                    if let Some(wm) = wm_session.as_mut() {
                        wm.enqueue_command_shortcut(action, session_launches, secondary_children.len())?;
                    }
                    continue;
                }
                if action==SHELL_HELP_SHORTCUT_ACTION || is_shell_switcher_shortcut(action){
                    if let Some(shell)=metadata_shell.as_mut() && shell.launcher_busy(){
                        shell.cancel_launcher()?;launcher_capture.present(None,0,&[],true);
                        if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
                    }
                }
                if action==SHELL_HELP_SHORTCUT_ACTION {
                    if let Some(shell)=metadata_shell.as_mut(){shell.queue_reference(sophia_protocol::ShellReferenceOperation::Toggle,wm_session.as_ref().and_then(LiveWmSession::reference_output).unwrap_or(output.id));}
                    continue;
                }
                if is_shell_switcher_shortcut(action) {
                    let broker = metadata_broker
                        .as_ref()
                        .ok_or("shell shortcut has no live metadata broker")?;
                    let shell = metadata_shell
                        .as_mut()
                        .ok_or("shell shortcut has no live metadata shell")?;
                    if shell.reference_busy() {
                        shell.cancel_reference()?;
                        reference_capture.present(None);
                        if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
                    }
                    if shell.interaction_presented() {
                        crate::session_println!(
                            "sophia_live_metadata_shell schema=1 status=shortcut_consumed outcome=already_open"
                        );
                        continue;
                    }
                    let output_bounds = wm_output_bounds(&outputs);
                    let bounds = output_bounds
                        .iter()
                        .find(|(candidate, _)| *candidate == output.id)
                        .map(|(_, bounds)| *bounds)
                        .ok_or("shell shortcut has no output bounds")?;
                    let root = wm_root_bounds(&output_bounds)
                        .ok_or("shell shortcut has no root bounds")?;
                    let activation_surfaces = live_shell_activation_surfaces(
                        &layout.layers,
                        &layout.presentation_roles,
                    );
                    match shell.request_candidate(
                        broker,
                        output,
                        bounds,
                        root,
                        &output_bounds,
                        &activation_surfaces,
                    ) {
                        Ok(()) => (),
                        Err(error) => {
                            crate::session_eprintln!(
                                "sophia_live_metadata_shell schema=1 status=transport_failed stage=candidate reason={error}"
                            );
                            shell.recover_transport("candidate_failure")?;
                            shell.revoke_interaction();
                            descriptor_captures.cancel_all();
                            runtime
                                .as_mut()
                                .ok_or("shell shortcut has no visual runtime")?
                                .revoke_descriptor_overlay_interaction();
                            continue;
                        }
                    };
                    crate::session_println!(
                        "sophia_live_metadata_shell schema=1 status=shortcut_admitted action=descriptor_switcher"
                    );
                    continue;
                }
                let wm = wm_session
                    .as_mut()
                    .ok_or("WM shortcut activated without a live WM session")?;
                if wm.public.as_ref().is_none_or(|public| !public.configured) {
                    crate::session_println!("sophia_live_wm schema=2 status=physical_action_withheld reason=policy_replacement");
                    continue;
                }
                match wm.enqueue_action(action, &layout, output)? {
                    LiveOrderedWmActionAdmission::Admitted => {
                        crate::session_println!(
                            "sophia_live_wm schema=1 status=physical_action_admitted action={}",
                            action.raw(),
                        );
                    }
                    LiveOrderedWmActionAdmission::RejectedCapacity { report } => {
                        if report {
                            crate::session_eprintln!(
                                "sophia_live_wm schema=2 status=request_rejected source=action reason=capacity action={}",
                                action.raw(),
                            );
                        }
                    }
                }
            }
    }};
}
include!("physical_input_phase.rs")
}

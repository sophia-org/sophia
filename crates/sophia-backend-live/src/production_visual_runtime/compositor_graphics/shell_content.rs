use super::*;

/// What the setter's checks decided before any write.
pub(in crate::production_visual_runtime) enum ShellContentPreflight {
    /// The same frame is already installed: the setter answers Ok(false).
    Unchanged,
    /// Admissible now, under this presented transform.
    Admit(sophia_engine::PresentedContentTransform),
}

impl LiveProductionVisualRuntime {
    /// Installs one complete shell content candidate and queues a retained
    /// repaint. The prior candidate remains installed if native queueing fails.
    pub fn set_shell_content(
        &mut self,
        frame: LiveShellContentFrame,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.set_shell_content_on_target(frame, scene, native_scanout)
    }

    /// Installs content in a Session-assigned component layer. The layer is not
    /// supplied by the peer; exact grant ownership is validated by the intake.
    pub fn set_shell_component_content(
        &mut self,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.set_shell_component_content_on_target(frame, layer, scene, native_scanout)
    }

    pub(in crate::production_visual_runtime) fn set_shell_content_on_target<
        T: NativeCompositionTarget,
    >(
        &mut self,
        frame: LiveShellContentFrame,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut T>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.set_shell_component_content_on_target(
            frame,
            LiveShellContentLayer::Shell,
            scene,
            native_scanout,
        )
    }

    pub(in crate::production_visual_runtime) fn set_shell_component_content_on_target<
        T: NativeCompositionTarget,
    >(
        &mut self,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut T>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let transform = match self.preflight_shell_component_content(
            &frame,
            layer,
            native_scanout
                .as_deref()
                .map(NativeCompositionTarget::frame_service_available),
        )? {
            ShellContentPreflight::Unchanged => return Ok(false),
            ShellContentPreflight::Admit(transform) => transform,
        };
        // The preflight admits only with native presentation; same refusal.
        let native_scanout = native_scanout
            .ok_or("shell content candidate cannot be accepted without native presentation")?;
        self.install_shell_component_content(frame, layer, transform, scene, native_scanout)
    }

    /// Every check the setter makes before it writes, in its order, with its
    /// messages, writing nothing (G3-3). Its answer is for the same owner
    /// turn only: it is not current authority once the turn moves on.
    /// `frame_service` is the native target's answer, `None` without native
    /// presentation (G3-4: the combined accept supplies its own target's).
    pub(in crate::production_visual_runtime) fn preflight_shell_component_content(
        &self,
        frame: &LiveShellContentFrame,
        layer: LiveShellContentLayer,
        frame_service: Option<bool>,
    ) -> Result<ShellContentPreflight, Box<dyn std::error::Error>> {
        let key = (frame.output, layer);
        if !frame.output.is_valid()
            || frame.candidate_generation == 0
            || frame.images.is_empty()
            || frame.images.iter().any(|image| {
                !matches!(
                    image.node,
                    CompositorNodeId::ShellContent {
                        grant,
                        output,
                        candidate,
                        ..
                    } if grant == frame.grant && output == frame.output && candidate == frame.candidate_generation
                        && image.resource.shell().is_some_and(|lease| lease.description().grant == grant)
                )
            })
        {
            return Err("shell content frame is malformed".into());
        }
        if self
            .shell_content
            .iter()
            .any(|((output, other_layer), owned)| {
                *output == frame.output && *other_layer != layer && owned.frame.grant == frame.grant
            })
        {
            return Err("one content grant cannot occupy multiple component layers".into());
        }
        if self
            .shell_content
            .get(&key)
            .is_some_and(|owned| owned.interaction_revoked && owned.frame.grant == frame.grant)
        {
            return Err(
                "closing component must be removed before another candidate is admitted".into(),
            );
        }
        if self.shell_content.get(&key).map(|owned| &owned.frame) == Some(frame) {
            return Ok(ShellContentPreflight::Unchanged);
        }
        let frame_service_available = frame_service
            .ok_or("shell content candidate cannot be accepted without native presentation")?;
        if self.native_suspended || !frame_service_available {
            return Err(
                "shell content candidate cannot be accepted while native presentation is quiesced"
                    .into(),
            );
        }
        let output = frame.output;
        let grant = frame.grant;
        if self
            .shell_retirement_claims(key)
            .any(|owner| owner != grant)
        {
            return Err(
                "shell content candidate found an unreconciled retirement from another grant"
                    .into(),
            );
        }
        Ok(ShellContentPreflight::Admit(
            sophia_engine::PresentedContentTransform {
                viewport: self
                    .outputs
                    .logical_viewport(output)
                    .ok_or("shell content output has no committed viewport")?,
                layout_generation: self.content_layout_generation,
            },
        ))
    }

    /// The setter's write: installs a preflighted frame and queues its
    /// retained repaint, restoring the prior content if queueing fails.
    fn install_shell_component_content<T: NativeCompositionTarget>(
        &mut self,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        transform: sophia_engine::PresentedContentTransform,
        scene: &LiveProductionCpuScene,
        native_scanout: &mut T,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let key = (frame.output, layer);
        let grant = frame.grant;
        let previous = self.shell_content.insert(
            key,
            AdmittedShellContent {
                frame,
                retained_content_epoch: None,
                transform,
                interaction_revoked: false,
            },
        );
        let previous_retirement = self.retained_projection_retirements.insert(key, grant);
        if let Err(error) = self.queue_retained_projection(scene, native_scanout) {
            match previous_retirement {
                Some(previous) => {
                    self.retained_projection_retirements.insert(key, previous);
                }
                None => {
                    self.retained_projection_retirements.remove(&key);
                }
            }
            match previous {
                Some(previous) => {
                    self.shell_content.insert(key, previous);
                }
                None => {
                    self.shell_content.remove(&key);
                }
            }
            return Err(error);
        }
        Ok(true)
    }

    pub(in crate::production_visual_runtime) fn shell_retirement_claims(
        &self,
        key: ShellContentKey,
    ) -> impl Iterator<Item = sophia_protocol::ContentGrant> + '_ {
        self.retained_projection_retirements
            .get(&key)
            .copied()
            .into_iter()
            .chain(
                self.queued_shell_retirements
                    .values()
                    .filter_map(move |claims| claims.get(&key).copied()),
            )
    }

    /// Retires the physical-presentation claims owned by a revoked shell
    /// connection. The caller must revoke that connection first: clearing a
    /// live claim without closing its protocol obligation would strand an
    /// accepted candidate without either Presented or Rejected.
    pub fn revoke_shell_content_retirement_claims(
        &mut self,
        grant: sophia_protocol::ContentGrant,
    ) -> usize {
        let before = self.retained_projection_retirements.len();
        self.retained_projection_retirements
            .retain(|_, owner| *owner != grant);
        let mut removed = before.saturating_sub(self.retained_projection_retirements.len());
        self.queued_shell_retirements.retain(|_, claims| {
            let before = claims.len();
            claims.retain(|_, owner| *owner != grant);
            removed += before - claims.len();
            !claims.is_empty()
        });
        removed
    }

    /// Drops content whose output disappeared or changed pixel size after a
    /// quiescent topology replacement. An output number alone cannot make an
    /// old allocation valid. Its presentation claims must first be revoked.
    pub(in crate::production_visual_runtime) fn retain_shell_content_outputs(
        &mut self,
        outputs: &[HeadlessOutput],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let compatible = |key: &ShellContentKey| {
            outputs
                .iter()
                .find(|output| output.id == key.0)
                .is_some_and(|output| {
                    self.shell_content.get(key).is_none_or(|owned| {
                        owned
                            .frame
                            .images
                            .iter()
                            .all(|image| image.output_size_px == output.size)
                    })
                })
        };
        if self
            .retained_projection_retirements
            .keys()
            .any(|key| !compatible(key))
            || self
                .queued_shell_retirements
                .iter()
                .any(|((output, _), claims)| {
                    !outputs.iter().any(|candidate| candidate.id == *output)
                        || claims.keys().any(|key| !compatible(key))
                })
        {
            return Err(
                "native topology replacement would orphan a shell content retirement claim".into(),
            );
        }
        let retained: BTreeSet<_> = self
            .shell_content
            .keys()
            .filter(|key| compatible(key))
            .copied()
            .collect();
        let before = self.shell_content.len();
        self.shell_content.retain(|key, _| retained.contains(key));
        Ok(before.saturating_sub(self.shell_content.len()))
    }

    pub fn shell_content_presentation_epoch(
        &self,
        output: OutputId,
        grant: sophia_protocol::ContentGrant,
        candidate_generation: u64,
    ) -> Option<u64> {
        let frame = self
            .shell_content
            .iter()
            .find(|((id, _), owned)| *id == output && owned.frame.grant == grant)?
            .1;
        let frame = &frame.frame;
        if frame.grant != grant || frame.candidate_generation != candidate_generation {
            return None;
        }
        let projection = self
            .input_projections
            .iter()
            .find(|projection| projection.output == output)?;
        let presented = self.tab_frames.get(&output).is_some_and(|display_list| {
            crate::production_visual_runtime::projection::presented_content_list_matches(
                display_list,
                frame,
            )
        });
        presented
            .then(|| {
                projection
                    .content
                    .iter()
                    .find(|binding| {
                        binding.grant == grant
                            && binding.candidate_generation == candidate_generation
                    })
                    .map(|binding| binding.presentation_epoch)
            })
            .flatten()
    }
}

#[path = "shell_content/removal.rs"]
mod removal;

/// Why a combined accept did not happen. Nothing changed in any case: the
/// frame comes back, and the facade still holds its result for disposal.
#[cfg(feature = "experimental-combined-frame-job")]
// First production caller is G3-5's owner row; until then the accept controls.
#[cfg_attr(not(test), allow(dead_code))]
pub enum CombinedAcceptError {
    /// The setter's own preflight refused the frame, with its message.
    Runtime(Box<dyn std::error::Error>),
    /// The frame is already installed: there is nothing to accept.
    Unchanged,
    /// The frame is for another output than the admitted plan.
    Output,
    /// A retirement claim for this key is still unbound, or the native
    /// frame already carries claims.
    RetirementClaim,
    Native(crate::CombinedAcceptRefusal),
}

#[cfg(feature = "experimental-combined-frame-job")]
// First production caller is G3-5's owner row; until then the accept controls.
#[cfg_attr(not(test), allow(dead_code))]
pub struct CombinedAcceptFailed {
    pub error: CombinedAcceptError,
    pub frame: LiveShellContentFrame,
}

#[cfg(feature = "experimental-combined-frame-job")]
impl LiveProductionVisualRuntime {
    /// The two-phase accept of a held combined result (AUTHORITY-STEP3 F2).
    /// The setter's preflight, then the native preflight, both reading only;
    /// then the commit: this runtime installs the content and binds its
    /// retirement claim to the plan's native frame without queueing a
    /// render (step 1), and the native target writes steps 2 to 4.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn accept_combined_shell_content<N: crate::CombinedAcceptNative>(
        &mut self,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        native: &mut N,
        head: &N::Head,
        accept: crate::CombinedAccept,
    ) -> Result<(), Box<CombinedAcceptFailed>> {
        let refuse = |error, frame| Err(Box::new(CombinedAcceptFailed { error, frame }));
        let transform = match self.preflight_shell_component_content(
            &frame,
            layer,
            Some(native.frame_service_available()),
        ) {
            Ok(ShellContentPreflight::Admit(transform)) => transform,
            Ok(ShellContentPreflight::Unchanged) => {
                return refuse(CombinedAcceptError::Unchanged, frame);
            }
            Err(error) => return refuse(CombinedAcceptError::Runtime(error), frame),
        };
        if frame.output != accept.snapshot.output.id {
            return refuse(CombinedAcceptError::Output, frame);
        }
        let key = (frame.output, layer);
        let native_frame = accept.frame;
        if self.retained_projection_retirements.contains_key(&key)
            || self
                .queued_shell_retirements
                .contains_key(&(frame.output, native_frame))
        {
            return refuse(CombinedAcceptError::RetirementClaim, frame);
        }
        let plan = match native.preflight_combined_accept(head, accept) {
            Ok(plan) => plan,
            Err(refusal) => return refuse(CombinedAcceptError::Native(refusal), frame),
        };
        // Step 1: content and its retirement claim, bound to the plan's
        // native frame as `bind_shell_retirement_claims` binds a queued one.
        let grant = frame.grant;
        self.shell_content.insert(
            key,
            AdmittedShellContent {
                frame,
                retained_content_epoch: None,
                transform,
                interaction_revoked: false,
            },
        );
        self.queued_shell_retirements
            .entry((key.0, native_frame))
            .or_default()
            .insert(key, grant);
        native.commit_combined_accept(head, plan);
        Ok(())
    }
}

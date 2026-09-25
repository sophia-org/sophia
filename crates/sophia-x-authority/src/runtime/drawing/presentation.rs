// Carrying a finished draw to the screen: composing it into the toplevel's
// presentation with what is stacked over it, journaling it for density
// replay, and answering density demand from that journal. Included by
// runtime.rs beside the other drawing paths, so it shares their imports.

impl XAuthorityRuntime {
    /// A border change moves existing pixels without changing their drawable
    /// coordinates. Recompose the whole top-level so the old position is erased.
    pub(crate) fn republish_border_geometry(
        &mut self,
        transaction: TransactionId,
        namespace: NamespaceId,
        window: crate::XResourceId,
    ) -> Option<XAuthorityResponsePacket> {
        let surface = self.windows.get(window)?.authority_surface();
        let (top, _, _) = self.windows.presentation_root_and_offset(window).ok()?;
        let record = self.windows.get(top)?;
        let generation = record.generation;
        let whole = Rect { x: 0, y: 0, width: record.geometry.width, height: record.geometry.height };
        let Some(handle) = self.software_buffers.buffer_handle(top) else {
            let mut response = XAuthorityResponsePacket::accepted(transaction);
            response.surfaces.push(surface);
            return Some(response);
        };
        self.pending_raster_command = Some(XAuthorityRasterCommand::Unsupported(
            XRasterUnsupportedKind::RenderOperation,
        ));
        let mut response = self.finish_drawing_update(XDrawingUpdate::core_draw(
            transaction, namespace, top, handle, Region::single(whole), generation, 250,
        ));
        response.surfaces.push(surface);
        Some(response)
    }

    /// What covers `source` inside the toplevel `presentation`.
    ///
    /// Every mapped window painted after the source is either its inferior
    /// or stacked over it, so those are the layers -- except the source's
    /// own inferiors while a draw goes through them, since the source's
    /// buffer then already holds them. A source the walk never reaches is
    /// unmapped or clipped away and contributes no pixels to the presentation.
    fn present_stacking(
        &self,
        presentation: crate::XResourceId,
        source: crate::XResourceId,
    ) -> crate::XPresentStacking {
        let Some(root) = self.windows.get(presentation) else {
            return crate::XPresentStacking::default();
        };
        let whole = Rect {
            x: 0,
            y: 0,
            width: root.geometry.width,
            height: root.geometry.height,
        };
        let painted = self.painted_subtree(root.namespace, presentation, whole);
        if source == presentation {
            let above = if self.drawing_through == Some(source) {
                Vec::new()
            } else {
                painted.into_iter().map(|window| window.layer).collect()
            };
            return crate::XPresentStacking {
                source_clip: None,
                above,
            };
        }
        let Some(index) = painted.iter().position(|window| window.layer.window == source) else {
            return crate::XPresentStacking {
                source_clip: Some(Rect::default()),
                above: Vec::new(),
            };
        };
        let depth = painted[index].depth;
        let after = &painted[index + 1..];
        // A window's inferiors follow it directly in painting order, deeper.
        let inferiors = if self.drawing_through == Some(source) {
            after.iter().take_while(|window| window.depth > depth).count()
        } else {
            0
        };
        crate::XPresentStacking {
            source_clip: Some(painted[index].layer.clip),
            above: after[inferiors..].iter().map(|window| window.layer).collect(),
        }
    }

    fn finish_drawing_update(&mut self, mut update: XDrawingUpdate) -> XAuthorityResponsePacket {
        let transaction_id = update.transaction;
        let source_window = update.target_window;
        let semantic_command = self
            .pending_raster_command
            .take()
            .map(XAuthorityRasterCommand::unless_clip_masked);
        let mut cpu_buffer_updates = Vec::new();
        if matches!(
            update.buffer,
            sophia_protocol::BufferSource::CpuBuffer { .. }
        ) && update.kind != crate::XDrawingUpdateKind::PresentPixmap
        {
            let (presentation_window, offset_x, offset_y) =
                match self.windows.presentation_root_and_offset(source_window) {
                    Ok(presentation) => presentation,
                    Err(error) => {
                        return XAuthorityResponsePacket::rejected(transaction_id, error.into());
                    }
                };
            let Some(presentation_record) = self.windows.get(presentation_window) else {
                return XAuthorityResponsePacket::rejected(
                    transaction_id,
                    XAuthorityRuntimeError::UnknownResource,
                );
            };
            let presentation_size = Size {
                width: presentation_record.geometry.width,
                height: presentation_record.geometry.height,
            };
            update.previous_committed_generation = presentation_record.generation;
            // A bounding shape on the toplevel is what clips the composed
            // result; an unset one leaves the buffer opaque as before.
            let shape =
                match self.effective_shape(presentation_window, crate::X_SHAPE_KIND_BOUNDING) {
                    (true, rects) => Some(rects),
                    (false, _) => None,
                };
            let stacking = self.present_stacking(presentation_window, source_window);
            let Some(presentation_update) = self.software_buffers.present_window_damage(
                presentation_window,
                presentation_size,
                source_window,
                offset_x,
                offset_y,
                &update.damage.rects,
                shape.as_deref(),
                &stacking,
            ) else {
                return XAuthorityResponsePacket::rejected(
                    transaction_id,
                    XAuthorityRuntimeError::InvalidResource,
                );
            };
            update.target_window = presentation_window;
            update.buffer = sophia_protocol::BufferSource::CpuBuffer {
                handle: presentation_update.handle(),
            };
            // The authority composed this raster itself, at the presentation
            // buffer's size, so it spans exactly what it fills.
            update.presentation_extent = Some(presentation_update.size());
            update.raster_extent = Some(presentation_update.size());
            update.damage = Region {
                rects: update
                    .damage
                    .rects
                    .iter()
                    .map(|rect| Rect {
                        x: rect.x.saturating_add(offset_x),
                        y: rect.y.saturating_add(offset_y),
                        width: rect.width,
                        height: rect.height,
                    })
                    .collect(),
            };
            cpu_buffer_updates.push(presentation_update.clone());
            if let Some(command) = semantic_command {
                let mut command = command.translated(offset_x, offset_y);
                // Replay draws over the whole toplevel, so a command is held
                // to what its window shows there (t200).
                if stacking.source_clip.is_some() || !stacking.above.is_empty() {
                    let size = presentation_update.size();
                    let shown = stacking.source_clip.unwrap_or(Rect {
                        x: 0,
                        y: 0,
                        width: size.width,
                        height: size.height,
                    });
                    let above = stacking
                        .above
                        .iter()
                        .map(|layer| layer.clip)
                        .collect::<Vec<_>>();
                    command = command.clipped_to(
                        &sophia_protocol::geometry::region_algebra::subtract(&[shown], &above),
                    );
                }
                cpu_buffer_updates.extend(self.raster_store.record(
                    presentation_window,
                    presentation_update.size(),
                    command,
                ));
            } else {
                self.raster_store.invalidate_unjournaled_presentation(
                    presentation_window,
                    presentation_update.size(),
                );
            }
        }
        let window = update.target_window;
        let previous_generation = update.previous_committed_generation;
        let mut transaction = match surface_transaction_from_drawing_update(&self.windows, update) {
            Ok(transaction) => transaction,
            Err(error) => {
                return XAuthorityResponsePacket::rejected(transaction_id, error.into());
            }
        };
        // A window that shaped its input answers the pointer only inside
        // that shape; one that has not is interactive everywhere.
        transaction.input_region = match self.effective_shape(window, crate::X_SHAPE_KIND_INPUT) {
            (true, rects) => Some(Region { rects }),
            (false, _) => None,
        };
        if let Err(error) = self.windows.advance_generation(window, previous_generation) {
            return XAuthorityResponsePacket::rejected(transaction_id, error.into());
        }
        // A retained CPU background is not the source of a later DRI3 Present.
        // Expand density variants only for the exact backing this draw chose;
        // changing the source here also changes Present's routing and fences.
        if let Some(canonical) = self.software_buffers.presentation_snapshot(window)
            && transaction.target_buffer()
                == (sophia_protocol::BufferSource::CpuBuffer {
                    handle: canonical.handle,
                })
        {
            transaction.content = self.raster_store.content_set(window, canonical);
        }
        self.last_cpu_buffer_updates.extend(cpu_buffer_updates);
        let mut response = XAuthorityResponsePacket::accepted(transaction_id);
        response.transactions.push(transaction);
        response
    }

    /// Applies one protocol-neutral Engine raster requirement to the
    /// presentation surface currently owning that `SurfaceId`. Late or stale
    /// requirements fail closed before allocating or publishing pixels.
    pub fn apply_surface_raster_requirements(
        &mut self,
        transaction: TransactionId,
        requirements: &sophia_protocol::SurfaceRasterRequirements,
    ) -> Result<crate::XSurfaceRasterOutcome, XAuthorityRuntimeError> {
        use crate::XSurfaceRasterOutcome;
        requirements
            .validate()
            .map_err(|_| XAuthorityRuntimeError::InvalidResource)?;
        if !transaction.is_valid() {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        let record = self
            .windows
            .presentation_for_surface(requirements.surface)
            .cloned()
            .ok_or(XAuthorityRuntimeError::UnknownResource)?;
        // A requirement is advisory demand, not a contract pinned to the
        // generation Engine had committed when it asked. Engine builds
        // requirements from its committed scene and commits authority
        // transactions as an ordered chain, so under a drawing client it names
        // a generation this authority already passed. Answering from current
        // state is correct because this call publishes a complete replacement
        // transaction rather than amending committed content, and the response
        // travels the same ordered egress as ordinary draws, so it commits
        // once Engine's chain reaches the generation it is anchored at.
        //
        // The authority running *behind* the request is the genuine error: it
        // names content that was never produced.
        if record.generation < requirements.committed_content_generation {
            return Ok(XSurfaceRasterOutcome::SampledFallback {
                cause: crate::XRasterFallbackCause::StaleContentGeneration,
                observed_content_generation: record.generation,
            });
        }
        // A surface whose pixels arrived through a renderer or pixmap Present
        // has no canonical CPU drawable, so there is nothing to replay from.
        // That is an ordinary content state, not a runtime failure: reporting
        // it as an error here would propagate out of the connection loop and
        // take the whole X server down over one surface's demand.
        let Some(canonical) = self
            .software_buffers
            .presentation_snapshot(record.id)
            .cloned()
        else {
            return Ok(XSurfaceRasterOutcome::SampledFallback {
                cause: crate::XRasterFallbackCause::NoCanonicalRaster,
                observed_content_generation: record.generation,
            });
        };
        if canonical.size != requirements.logical_extent {
            return Ok(XSurfaceRasterOutcome::SampledFallback {
                cause: crate::XRasterFallbackCause::LogicalExtentMismatch,
                observed_content_generation: record.generation,
            });
        }
        // The store's refusals — a changed extent, a projected size or stride
        // overflow, a backing bound — are all states this surface can
        // legitimately be in, so they answer the demand rather than fail the
        // runtime.
        let satisfied =
            match self
                .raster_store
                .satisfy(record.id, requirements, canonical.bytes.len())
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    return Ok(XSurfaceRasterOutcome::SampledFallback {
                        cause: crate::XRasterFallbackCause::LogicalExtentMismatch,
                        observed_content_generation: record.generation,
                    });
                }
            };
        let updates = match satisfied {
            crate::XRasterSatisfyOutcome::Satisfied(updates) => updates,
            crate::XRasterSatisfyOutcome::Fallback(cause) => {
                return Ok(XSurfaceRasterOutcome::SampledFallback {
                    cause,
                    observed_content_generation: record.generation,
                });
            }
        };
        let content = self.raster_store.content_set(record.id, &canonical);
        let all_satisfied = requirements.classes.iter().all(|class| {
            content.variants().iter().any(|variant| {
                variant.density_millis == class.density_millis
                    && variant.transform == class.transform
                    && variant.fidelity == sophia_protocol::SurfaceContentFidelity::AuthorityRaster
            })
        });
        if !all_satisfied {
            // Guards content-set variant truncation: the store accepted the
            // requirement, but publication could not carry every class.
            return Ok(XSurfaceRasterOutcome::SampledFallback {
                cause: crate::XRasterFallbackCause::BackingCapacity,
                observed_content_generation: record.generation,
            });
        }
        // A window that shaped its input answers the pointer only inside
        // that shape; one that has not is interactive everywhere.
        let input_region = match self.effective_shape(record.id, crate::X_SHAPE_KIND_INPUT) {
            (true, rects) => Some(Region { rects }),
            (false, _) => None,
        };
        let surface_transaction = sophia_protocol::SurfaceTransaction {
            transaction,
            authority: sophia_protocol::AuthorityKind::SophiaX,
            surface: record.surface,
            namespace: Some(record.namespace),
            input_region,
            target_geometry: record.interior_geometry(),
            content,
            // Engine asked for this raster at this extent and the store
            // produced it, so what it fills is what it spans.
            presentation_extent: requirements.logical_extent,
            damage: Region::single(Rect {
                x: 0,
                y: 0,
                width: requirements.logical_extent.width,
                height: requirements.logical_extent.height,
            }),
            readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: record.generation,
        };
        self.windows
            .advance_generation(record.id, record.generation)
            .map_err(XAuthorityRuntimeError::from)?;
        Ok(XSurfaceRasterOutcome::Satisfied(Box::new(
            crate::XAuthorityRasterRequirementResponse {
                identity: sophia_protocol::SurfaceRasterResponseIdentity {
                    transaction,
                    surface: record.surface,
                    // The generation this content was actually produced from,
                    // which may lead the one requested. Reporting the request
                    // back would misdescribe the pixels.
                    source_content_generation: record.generation,
                    requirement_generation: requirements.requirement_generation,
                },
                transaction: surface_transaction,
                cpu_buffer_updates: updates,
            },
        )))
    }
}

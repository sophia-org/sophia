struct ReconciledPublicPolicyProposal {
    policy: sophia_protocol::PolicyProjectionProposal,
    content: BTreeMap<SurfaceId, sophia_protocol::PolicySurfacePlacement>,
    adjusted_surfaces: usize,
}

fn reconcile_public_policy_proposal(
    layout: &PersistentLiveLayout,
    proposal: &sophia_protocol::PolicyProjectionProposal,
    work_areas: &BTreeMap<sophia_protocol::OutputId, Rect>,
    output_bounds: &BTreeMap<sophia_protocol::OutputId, Rect>,
    chrome: sophia_engine::SurfaceChromeStyle,
) -> Result<ReconciledPublicPolicyProposal, Box<dyn std::error::Error>> {
    let mut reconciled = proposal.clone();
    let mut content = BTreeMap::new();
    let mut adjusted_surfaces = BTreeSet::new();
    for output in &mut reconciled.outputs {
        let work_bounds = work_areas
            .get(&output.output)
            .copied()
            .ok_or("public WM projection has no Engine work area")?;
        let full_bounds = output_bounds
            .get(&output.output)
            .copied()
            .ok_or("public WM projection has no Engine output bounds")?;
        for (index, placement) in output.placements.iter_mut().enumerate() {
            // Fullscreen remains a full-output allocation and is occluded by
            // compositor chrome. Every other placement is constrained to the
            // reserved work area. Reconciling them separately prevents one
            // fullscreen surface from widening its siblings' authority.
            let allocation_bounds = if placement.presentation.fullscreen {
                full_bounds
            } else {
                work_bounds
            };
            let content_bounds =
                sophia_engine::content_surface_geometry(allocation_bounds, chrome)?;
            let transaction = sophia_protocol::LayoutTransaction {
                transaction: proposal.transaction,
                requested_sizes: placement
                    .requested_size
                    .map(|size| {
                        vec![sophia_protocol::SurfaceSizeRequest {
                            surface: placement.surface,
                            size,
                        }]
                    })
                    .unwrap_or_default(),
                focus: output.focus,
                render_positions: vec![sophia_protocol::SurfacePlacement {
                    surface: placement.surface,
                    geometry: placement.geometry,
                    z_index: i32::try_from(index)
                        .map_err(|_| "public WM projection stack is excessive")?,
                    crop: placement.crop,
                    transform: match placement.transform {
                        sophia_protocol::PolicyTransform::Identity => Transform::IDENTITY,
                    },
                }],
                timeout_msec: u32::try_from(SESSION_POLICY_RESPONSE_TIMEOUT_MSEC)
                    .unwrap_or(u32::MAX),
            };
            let transaction =
                sophia_engine::apply_surface_chrome_clearance(&transaction, chrome)?;
            // An ordinary first frame can exceed this output. Admit its exact
            // retained pixels before requesting a new size: a Present-driven
            // client may need that retirement to draw again. Its output owner
            // clips the temporary placement. Fullscreen keeps its exact-output
            // geometry contract, and no stale or unretained candidate qualifies.
            let admission_candidate = (!placement.presentation.fullscreen)
                .then(|| layout.retained_admission_candidate(placement.surface))
                .flatten();
            let reconciliation = match admission_candidate {
                Some(candidate) => layout.layout_epochs.reconcile_admission_transaction(
                    &transaction, content_bounds, candidate,
                )?,
                None => layout.layout_epochs.reconcile_transaction(&transaction, content_bounds)?,
            };
            let content_geometry = reconciliation
                .transaction
                .render_positions
                .first()
                .map(|placement| placement.geometry)
                .ok_or("public WM reconciliation dropped a placement")?;
            let content_requested_size = reconciliation
                .transaction
                .requested_sizes
                .iter()
                .find(|request| request.surface == placement.surface)
                .map(|request| request.size);
            let outer_geometry = sophia_engine::outer_surface_geometry(content_geometry, chrome)?;
            let outer_requested_size = placement.requested_size.map(|_| sophia_protocol::Size {
                width: outer_geometry.width,
                height: outer_geometry.height,
            });
            if placement.geometry != outer_geometry
                || placement.requested_size != outer_requested_size
            {
                adjusted_surfaces.insert(placement.surface);
            }
            let mut content_placement = placement.clone();
            content_placement.geometry = content_geometry;
            content_placement.requested_size = content_requested_size;
            content.insert(placement.surface, content_placement);
            // The reducer remains an outer-allocation model. Only the proposal
            // materialized into Engine layers and configure requests crosses
            // the chrome boundary into content coordinates.
            placement.geometry = outer_geometry;
            placement.requested_size = outer_requested_size;
            crate::session_println!(
                "sophia_live_wm_chrome schema=2 status=reconciled transaction={} output={} surface={} clearance={} outer={}x{}_{}_{} content={}x{}_{}_{} request={} adjusted={}",
                proposal.transaction.raw(),
                output.output.raw(),
                placement.surface.index(),
                chrome.clearance(),
                placement.geometry.width,
                placement.geometry.height,
                placement.geometry.x,
                placement.geometry.y,
                content_geometry.width,
                content_geometry.height,
                content_geometry.x,
                content_geometry.y,
                content_requested_size
                    .map(|size| format!("{}x{}", size.width, size.height))
                    .unwrap_or_else(|| "none".to_owned()),
                adjusted_surfaces.contains(&placement.surface),
            );
        }
    }
    Ok(ReconciledPublicPolicyProposal {
        policy: reconciled,
        content,
        adjusted_surfaces: adjusted_surfaces.len(),
    })
}

fn public_live_proposal(
    layout: &PersistentLiveLayout,
    active_output: sophia_protocol::OutputId,
    projections: Vec<sophia_protocol::PolicyOutputProjection>,
    transaction: TransactionId,
    source: LiveWmProposalSource,
    settlement: LivePolicySettlementIdentity,
    reconciliation: &ReconciledPublicPolicyProposal,
) -> Result<LiveWmProposal, Box<dyn std::error::Error>> {
    let mut layers = layout
        .layers
        .values()
        .filter(|layer| !layout.is_policy_managed(layer.surface))
        .cloned()
        .collect::<Vec<_>>();
    let mut requested_sizes = BTreeMap::new();
    let mut presentation_states = BTreeMap::new();
    let mut applied_surfaces = Vec::new();
    let focus = projections
        .iter()
        .find(|projection| projection.output == active_output)
        .and_then(|projection| projection.focus);
    for projection in projections {
        // The output this projection placed for. Every layer built below is
        // composited by this head and no other.
        let owning_output = projection.output;
        let updated = reconciliation
            .policy
            .outputs
            .iter()
            .any(|output| output.output == owning_output);
        for placement in projection.placements {
            applied_surfaces.push(placement.surface);
            presentation_states.insert(placement.surface, placement.presentation);
            if placement.presentation.minimized {
                continue;
            }
            // A staged reducer contains every output, but interaction replies
            // replace only the requested outputs. The others already have
            // committed content coordinates and must not cross chrome clearance
            // again or replay an earlier configure request.
            if !updated {
                let mut layer = layout
                    .layers
                    .get(&placement.surface)
                    .filter(|layer| layer.output == Some(owning_output))
                    .cloned()
                    .ok_or("public WM retained output has no committed content placement")?;
                layer.stack_rank = u32::try_from(layers.len()).unwrap_or(u32::MAX - 1);
                layers.push(layer);
                continue;
            }
            let materialized = reconciliation
                .content
                .get(&placement.surface)
                .ok_or("public WM projection has no reconciled content placement")?;
            let mut layer = if let Some(layer) = layout.layers.get(&placement.surface) {
                layer.clone()
            } else {
                let facts = layout
                    .layout_facts(placement.surface)
                    .ok_or("public WM projection names a missing planning surface")?;
                LayerSnapshot {
                    input_region: None,
                    translation: None,
            surface: facts.surface,
                    authority_local_id: None,
                    // The projection that placed this names the output, and
                    // that is what decides which head composites it.
                    output: Some(owning_output),
                    namespace: None,
                    stack_rank: facts.stack_rank,
                    geometry: facts.geometry,
                    source: BufferSource::None,
                    // A planning surface names no raster yet, so this is a placeholder.
                    source_size: sophia_protocol::Size {
                        width: facts.geometry.width,
                        height: facts.geometry.height,
                    },
                    damage: Region::empty(),
                    opacity: 1.0,
                    crop: None,
                    transform: Transform::IDENTITY,
                    generation: facts.generation,
                    resize_sync: ResizeSyncCapability::ImplicitOnly,
                }
            };
            // Cached pixels may predate the first placement or belong to the
            // previous output. Every accepted placement supplies its owner.
            layer.output = Some(owning_output);
            layer.geometry = materialized.geometry;
            layer.stack_rank = u32::try_from(layers.len()).unwrap_or(u32::MAX - 1);
            if let Some(size) = materialized.requested_size {
                requested_sizes.insert(placement.surface, size);
            }
            layers.push(layer);
        }
    }
    let moved_surfaces = layers
        .iter()
        .filter(|layer| {
            layout
                .layers
                .get(&layer.surface)
                .is_some_and(|current| current.geometry != layer.geometry)
        })
        .count();
    Ok(LiveWmProposal {
        transaction,
        layers,
        requested_sizes,
        presentation_states,
        configure_deliveries: 0,
        focus,
        timeout: Duration::from_millis(SESSION_POLICY_RESPONSE_TIMEOUT_MSEC),
        update: WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces,
            },
        },
        moved_surfaces,
        source: Some(source),
        policy_settlement: Some(settlement),
    })
}

fn public_operation_proposal(
    layout: &PersistentLiveLayout,
    transaction: TransactionId,
    settlement: LivePolicySettlementIdentity,
) -> LiveWmProposal {
    LiveWmProposal {
        transaction,
        layers: layout.layers.values().cloned().collect(),
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: None,
        timeout: Duration::from_secs(1),
        update: WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: Vec::new(),
            },
        },
        moved_surfaces: 0,
        source: None,
        policy_settlement: Some(settlement),
    }
}

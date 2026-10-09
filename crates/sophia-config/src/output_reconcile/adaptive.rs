use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopOutputAdjustmentReason {
    Mode,
    Scale,
    Transform,
    Vrr,
    Position,
    Unavailable,
    MirrorUnavailable,
    Fallback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopOutputAdjustment {
    pub connector: String,
    pub reason: DesktopOutputAdjustmentReason,
}

/// Availability is a state, not a malformed profile. The caller retains its
/// logical checkpoint while waiting and must not publish an invented output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopOutputResolution {
    Active(DesktopOutputReconciliation),
    Waiting {
        generation: ConfigGeneration,
        digest: ConfigDigest,
        adjustments: Vec<DesktopOutputAdjustment>,
    },
}

/// Resolve preferences against one inventory. `previous` is the last committed
/// realization, with its current focus; speculative candidates never go here.
pub fn resolve_desktop_output_candidate(
    candidate: &DesktopOutputCandidate,
    topology: &DesktopOutputTopologySnapshot,
    previous: Option<&DesktopOutputReconciliation>,
) -> Result<DesktopOutputResolution, DesktopOutputReconcileError> {
    validate_candidate(candidate)?;
    validate_topology_for_availability(
        topology,
        candidate.availability == DesktopOutputAvailability::Adaptive,
    )?;
    let bound = super::identity::bind_candidate(candidate, topology)?;
    let candidate = &bound;
    validate_candidate(candidate)?;
    let mut resolved = if candidate.availability == DesktopOutputAvailability::Strict {
        reconcile_required_outputs(candidate, topology)?
    } else {
        validate_topology_for_availability(topology, true)?;
        match adaptive(candidate, topology, previous)? {
            DesktopOutputResolution::Active(resolved) => resolved,
            waiting @ DesktopOutputResolution::Waiting { .. } => return Ok(waiting),
        }
    };
    for named in &candidate.named {
        if let Some(key) = named.policy_key
            && resolved
                .outputs
                .iter()
                .any(|output| output.enabled && output.connector == named.connector)
        {
            resolved.policy_keys.insert(named.connector.clone(), key);
        }
    }
    if let (Some(connector), Some(key)) =
        (&resolved.fallback_connector, candidate.fallback_policy_key)
    {
        resolved.policy_keys.retain(|_, existing| *existing != key);
        resolved.policy_keys.insert(connector.clone(), key);
    }
    if let Some(previous) = previous {
        // Startup focus is not a hotplug focus-stealing request. Follow a live
        // focus first, then the same logical workspace owner across a port move.
        let focused = previous.focused_connector.as_ref();
        let live = focused.filter(|name| {
            resolved
                .outputs
                .iter()
                .any(|output| output.enabled && &output.connector == *name)
        });
        let key = focused.and_then(|name| previous.policy_keys.get(name));
        resolved.focused_connector = live
            .cloned()
            .or_else(|| {
                key.and_then(|key| {
                    resolved
                        .policy_keys
                        .iter()
                        .find(|(_, value)| *value == key)
                        .map(|(name, _)| name.clone())
                })
            })
            .or_else(|| {
                resolved
                    .outputs
                    .iter()
                    .filter(|output| output.enabled && output.mirror_of.is_none())
                    .map(|output| output.connector.clone())
                    .min()
            });
    }
    validate_desktop_output_reconciliation(&resolved, topology)?;
    Ok(DesktopOutputResolution::Active(resolved))
}

fn adjustment(
    adjustments: &mut Vec<DesktopOutputAdjustment>,
    connector: &str,
    reason: DesktopOutputAdjustmentReason,
) {
    let item = DesktopOutputAdjustment {
        connector: connector.into(),
        reason,
    };
    if !adjustments.contains(&item) {
        adjustments.push(item);
    }
}

/// Advertised timings only. Prefer the monitor's choice, then a predictable
/// conventional refresh and pixel area; enumeration order is never policy.
fn safe_mode(connector: &DesktopOutputTopologyConnector) -> Option<DesktopOutputTiming> {
    connector.preferred_mode.or_else(|| {
        connector.modes.iter().copied().min_by_key(|mode| {
            (
                mode.refresh_millihz.abs_diff(60_000),
                std::cmp::Reverse(u64::from(mode.width) * u64::from(mode.height)),
                *mode,
            )
        })
    })
}

fn adaptive(
    candidate: &DesktopOutputCandidate,
    topology: &DesktopOutputTopologySnapshot,
    previous: Option<&DesktopOutputReconciliation>,
) -> Result<DesktopOutputResolution, DesktopOutputReconcileError> {
    use DesktopOutputAdjustmentReason as Reason;
    let mut adjustments = Vec::new();
    let mut outputs = topology
        .connectors
        .iter()
        .map(|connector| {
            if candidate.inherit_sophia {
                connector.current.clone()
            } else {
                DesktopOutputState {
                    connector: connector.connector.clone(),
                    enabled: false,
                    mode: safe_mode(connector).unwrap_or(connector.current.mode),
                    scale_milli: connector.scales.automatic_milli,
                    position: (0, 0),
                    transform: DesktopOutputTransform::Normal,
                    vrr: DesktopOutputVrrMode::Disabled,
                    mirror_of: None,
                }
            }
        })
        .collect::<Vec<_>>();
    let index_of = |name: &str| {
        topology
            .connectors
            .iter()
            .position(|head| head.connector == name)
    };
    let mut focused_connector = None;
    for named in &candidate.named {
        let index = index_of(&named.connector);
        let enabled = named.enabled.unwrap_or_else(|| {
            index.is_none_or(|i| !candidate.inherit_sophia || outputs[i].enabled)
        });
        if !enabled && named.focus_at_startup == Some(true) {
            return Err(DesktopOutputReconcileError::FocusedOutputDisabled(
                named.connector.clone(),
            ));
        }
        let members = std::iter::once(&named.connector)
            .chain(&named.mirror)
            .collect::<Vec<_>>();
        let available = members.iter().all(|name| {
            index_of(name).is_some_and(|i| {
                let head = &topology.connectors[i];
                head.connected && safe_mode(head).is_some()
            })
        });
        if !enabled || !available {
            for name in &members {
                if let Some(i) = index_of(name) {
                    outputs[i].enabled = false;
                    outputs[i].mirror_of = None;
                }
            }
            if enabled {
                adjustment(
                    &mut adjustments,
                    &named.connector,
                    if named.mirror.is_empty() {
                        Reason::Unavailable
                    } else {
                        Reason::MirrorUnavailable
                    },
                );
            }
            continue;
        }
        let index = index.expect("available group has a primary");
        let connector = &topology.connectors[index];
        let output = &mut outputs[index];
        output.enabled = true;
        if let Some(mode) = named.mode {
            output.mode = match resolve_mode(connector, mode) {
                Ok(mode) => mode,
                Err(_) => {
                    adjustment(&mut adjustments, &named.connector, Reason::Mode);
                    safe_mode(connector).expect("available primary has modes")
                }
            };
        }
        if let Some(scale) = named.scale {
            output.scale_milli = match scale {
                DesktopOutputScale::Automatic => connector.scales.automatic_milli,
                DesktopOutputScale::FixedMilli(value) => value,
            };
        }
        let supports_scale = |scale| {
            members.iter().all(|name| {
                topology.connectors[index_of(name).expect("available member")]
                    .scales
                    .supports(scale)
            })
        };
        if !supports_scale(output.scale_milli) {
            let scale = [connector.scales.automatic_milli, 1_000]
                .into_iter()
                .chain(250..=8_000)
                .find(|scale| supports_scale(*scale));
            let Some(scale) = scale else {
                for name in &members {
                    outputs[index_of(name).expect("available member")].enabled = false;
                }
                adjustment(&mut adjustments, &named.connector, Reason::Unavailable);
                continue;
            };
            output.scale_milli = scale;
            adjustment(&mut adjustments, &named.connector, Reason::Scale);
        }
        output.position = named.position.unwrap_or(output.position);
        output.transform = named.transform.unwrap_or(output.transform);
        if !members.iter().all(|name| {
            topology.connectors[index_of(name).expect("available member")]
                .transforms
                .contains(output.transform)
        }) {
            output.transform = DesktopOutputTransform::Normal;
            adjustment(&mut adjustments, &named.connector, Reason::Transform);
        }
        output.vrr = named.vrr.unwrap_or(output.vrr);
        if output.vrr != DesktopOutputVrrMode::Disabled
            && !members.iter().all(|name| {
                topology.connectors[index_of(name).expect("available member")].vrr_capable
            })
        {
            output.vrr = DesktopOutputVrrMode::Disabled;
            adjustment(&mut adjustments, &named.connector, Reason::Vrr);
        }
        let primary = output.clone();
        for name in &named.mirror {
            let i = index_of(name).expect("available mirror");
            outputs[i] = DesktopOutputState {
                connector: name.clone(),
                mode: safe_mode(&topology.connectors[i]).expect("available mirror has modes"),
                mirror_of: Some(primary.connector.clone()),
                ..primary.clone()
            };
        }
        if named.focus_at_startup == Some(true) {
            focused_connector = Some(named.connector.clone());
        }
    }
    let mut fallback_connector = None;
    if !outputs.iter().any(|output| output.enabled) {
        let claimed = candidate
            .named
            .iter()
            .flat_map(|named| std::iter::once(&named.connector).chain(&named.mirror))
            .collect::<BTreeSet<_>>();
        let previous_fallback = previous.and_then(|previous| previous.fallback_connector.as_ref());
        let fallback = topology
            .connectors
            .iter()
            .enumerate()
            .filter(|(_, head)| {
                head.connected && !claimed.contains(&head.connector) && safe_mode(head).is_some()
            })
            .min_by_key(|(_, head)| (previous_fallback != Some(&head.connector), &head.connector));
        if let Some((index, head)) = fallback {
            outputs[index] = DesktopOutputState {
                connector: head.connector.clone(),
                enabled: true,
                mode: safe_mode(head).expect("usable fallback"),
                scale_milli: head.scales.automatic_milli,
                position: (0, 0),
                transform: DesktopOutputTransform::Normal,
                vrr: DesktopOutputVrrMode::Disabled,
                mirror_of: None,
            };
            focused_connector = Some(head.connector.clone());
            fallback_connector = Some(head.connector.clone());
            adjustment(&mut adjustments, &head.connector, Reason::Fallback);
        }
    }
    if !outputs.iter().any(|output| output.enabled) {
        return Ok(DesktopOutputResolution::Waiting {
            generation: candidate.generation,
            digest: candidate.digest,
            adjustments,
        });
    }
    if reject_overlaps(&outputs).is_err() {
        let mut groups = outputs
            .iter()
            .filter(|output| output.enabled)
            .map(|output| mirror_group_of(output).to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let anchor = previous
            .and_then(|previous| previous.focused_connector.as_ref())
            .or(focused_connector.as_ref());
        groups.sort_by_key(|name| (anchor != Some(name), name.clone()));
        let mut x = 0i32;
        for group in groups {
            let width = outputs
                .iter()
                .filter(|output| output.enabled && mirror_group_of(output) == group)
                .map(|output| output_rect(output).width)
                .max()
                .expect("live group");
            for output in outputs
                .iter_mut()
                .filter(|output| output.enabled && mirror_group_of(output) == group)
            {
                output.position = (x, 0);
                adjustment(&mut adjustments, &output.connector, Reason::Position);
            }
            x = x
                .checked_add(i32::try_from(width).map_err(|_| {
                    DesktopOutputReconcileError::InvalidTopology("logical width overflow".into())
                })?)
                .ok_or_else(|| {
                    DesktopOutputReconcileError::InvalidTopology(
                        "logical placement overflow".into(),
                    )
                })?;
        }
    }
    Ok(DesktopOutputResolution::Active(
        DesktopOutputReconciliation {
            generation: candidate.generation,
            digest: candidate.digest,
            outputs,
            focused_connector,
            fallback_connector,
            policy_keys: BTreeMap::new(),
            adjustments,
        },
    ))
}

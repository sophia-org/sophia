use super::*;
use crate::drm::seat_inventory::{SeatDrmCard, discover_admitted_seat_cards};
use std::io;

/// Capability observations made before allocating renderers or activating heads.
#[derive(Clone, Debug)]
pub struct LiveNativeOutputProbe {
    pub connector: String,
    pub gpu_identity: Option<String>,
    pub connector_id: u32,
    pub connected: bool,
    pub modes: Vec<LibdrmNativeOutputTiming>,
    pub preferred_mode: Option<LibdrmNativeOutputTiming>,
    pub usable: bool,
    pub vrr_capable: bool,
}

struct ProbedCard {
    admitted: SeatDrmCard,
    card: RealAtomicScanoutCard,
}

/// Dropping a discovery only releases its probe descriptors. It has no render
/// contexts, scanout buffers, modesets, or custody of a suspended session.
pub struct LiveNativeOutputDiscovery {
    cards: Vec<ProbedCard>,
    probes: Vec<LiveNativeOutputProbe>,
    probe_cards: BTreeMap<String, usize>,
}

pub struct LiveResolvedOutputReplacement {
    admitted: Vec<SeatDrmCard>,
    selection: RealAtomicScanoutSelectionSet,
    records: Vec<LiveSysfsConnectorRecord>,
    grouping: NativeMirrorGrouping,
    requests: BTreeMap<String, LiveNativeOutputRequest>,
}

impl LiveNativeOutputDiscovery {
    pub fn probe(opener: &crate::LiveSeatDeviceOpener) -> io::Result<Self> {
        use ::drm::control::Device;
        let admitted = discover_admitted_seat_cards(opener.name(), opener.gpu_admission())?;
        let mut cards = Vec::new();
        let mut probes = Vec::new();
        let mut probe_cards = BTreeMap::new();
        for identity in admitted {
            let card = RealAtomicScanoutCard::open_admitted_with_seat(opener, &identity)?;
            let atomic = crate::hardware_validation::atomic_scanout_card::admit_atomic_scanout_client_capabilities(&card);
            let resources = card.resource_handles()?;
            for handle in resources.connectors() {
                let info = card.get_connector(*handle, true)?;
                let connector_id = u32::from(*handle);
                let connector = card.connector_key(&info.to_string());
                let connected = info.state() == ::drm::control::connector::State::Connected;
                let mut modes = info
                    .modes()
                    .iter()
                    .copied()
                    .map(crate::drm::native_output_timing)
                    .filter(|mode| mode.valid())
                    .collect::<Vec<_>>();
                let preferred_mode = info
                    .modes()
                    .iter()
                    .copied()
                    .find(|mode| {
                        mode.mode_type()
                            .contains(::drm::control::ModeTypeFlags::PREFERRED)
                    })
                    .map(crate::drm::native_output_timing)
                    .filter(|mode| mode.valid());
                modes.sort();
                modes.dedup();
                let selected = (atomic && connected && !modes.is_empty()).then(|| {
                    select_native_primary_plane_targets_matching(&card, |id| id == connector_id)
                });
                let target = selected.and_then(|selected| selected.selections.into_iter().next());
                let usable = target.is_some_and(|target| {
                    discover_native_primary_plane_property_handles(
                        &card,
                        target.connector,
                        target.crtc,
                        target.plane,
                    )
                    .status
                        == LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered
                });
                let vrr_capable = usable
                    && target.is_some_and(|target| {
                        discover_native_vrr_properties(&card, target.connector, target.crtc).status
                            == LibdrmNativeVrrPropertyDiscoveryStatus::Discovered
                    });
                if probes.len() == sophia_engine::MAX_DRM_KMS_OUTPUTS || modes.len() > 256 {
                    return Err(io::Error::other(
                        "native output discovery exceeds its bound",
                    ));
                }
                probe_cards.insert(connector.clone(), cards.len());
                probes.push(LiveNativeOutputProbe {
                    connector,
                    gpu_identity: card.gpu_identity().map(str::to_owned),
                    connector_id,
                    connected,
                    modes,
                    preferred_mode,
                    usable,
                    vrr_capable,
                });
            }
            identity.validate_current(opener.name())?;
            cards.push(ProbedCard {
                admitted: identity,
                card,
            });
        }
        probes.sort_by(|a, b| a.connector.cmp(&b.connector));
        if probes
            .windows(2)
            .any(|pair| pair[0].connector == pair[1].connector)
        {
            return Err(io::Error::other(
                "native connectors have ambiguous GPU identity",
            ));
        }
        Ok(Self {
            cards,
            probes,
            probe_cards,
        })
    }

    pub fn connectors(&self) -> &[LiveNativeOutputProbe] {
        &self.probes
    }

    /// Allocate CRTCs/planes only to the resolved heads. Still no renderer,
    /// framebuffer or KMS commit exists at this point.
    pub fn resolve(
        self,
        requests: Vec<LiveNativeOutputRequest>,
    ) -> io::Result<LiveResolvedOutputReplacement> {
        let requests = validate_requests(&self.probes, requests)?;
        let groups = requests
            .values()
            .filter(|request| request.mirror_of.is_none())
            .filter_map(|primary| {
                let mut group = vec![primary.connector.clone()];
                group.extend(
                    requests
                        .values()
                        .filter(|member| member.mirror_of.as_deref() == Some(&primary.connector))
                        .map(|member| member.connector.clone()),
                );
                (group.len() > 1).then_some(group)
            });
        let grouping = NativeMirrorGrouping::new(groups)
            .map_err(|error| io::Error::other(format!("resolved mirror grouping: {error:?}")))?;
        let mut selected_cards = Vec::new();
        let mut records = Vec::new();
        let mut admitted = Vec::new();
        for (index, probed) in self.cards.into_iter().enumerate() {
            probed.admitted.validate_current(&probed.admitted.seat)?;
            let chosen = self
                .probes
                .iter()
                .filter(|probe| {
                    self.probe_cards[&probe.connector] == index
                        && requests.contains_key(&probe.connector)
                })
                .collect::<Vec<_>>();
            if !chosen.is_empty() {
                let selected = select_native_primary_plane_targets_matching(&probed.card, |id| {
                    chosen.iter().any(|probe| probe.connector_id == id)
                });
                if selected.status != LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll
                    || selected.selections.len() != chosen.len()
                {
                    return Err(io::Error::other(
                        "resolved heads have no complete KMS assignment",
                    ));
                }
                let mut selections = Vec::new();
                for mut selection in selected.selections {
                    let probe = chosen
                        .iter()
                        .find(|probe| probe.connector_id == selection.connector_id())
                        .expect("selected only requested connectors");
                    let request = &requests[&probe.connector];
                    let mode = resolve_native_connector_mode(
                        &probed.card,
                        selection.connector,
                        request.mode,
                    )?
                    .ok_or_else(|| {
                        io::Error::other("resolved timing disappeared before activation")
                    })?;
                    selection.mode = Some(mode);
                    selection.size = sophia_protocol::Size {
                        width: i32::from(mode.size().0),
                        height: i32::from(mode.size().1),
                    };
                    if discover_native_primary_plane_property_handles(
                        &probed.card,
                        selection.connector,
                        selection.crtc,
                        selection.plane,
                    )
                    .status
                        != LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered
                    {
                        return Err(io::Error::other("resolved head lost its atomic properties"));
                    }
                    if request.vrr != sophia_protocol::OutputVrrPolicy::Disabled
                        && discover_native_vrr_properties(
                            &probed.card,
                            selection.connector,
                            selection.crtc,
                        )
                        .status
                            != LibdrmNativeVrrPropertyDiscoveryStatus::Discovered
                    {
                        return Err(io::Error::other("resolved head lost VRR support"));
                    }
                    let card_name = probed
                        .admitted
                        .node
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| io::Error::other("admitted card has no node name"))?;
                    let connector_name = probe.connector.rsplit('/').next().expect("connector key");
                    records.push(LiveSysfsConnectorRecord {
                        connector_name: format!("{card_name}-{connector_name}"),
                        connector_id: selection.connector_id(),
                        crtc_id: selection.crtc_id(),
                        mode: sophia_engine::DrmKmsMode::new(
                            selection.size.width,
                            selection.size.height,
                            request.mode.refresh_millihz,
                        ),
                        scale: request.scale,
                    });
                    selections.push(selection);
                }
                probed.admitted.validate_current(&probed.admitted.seat)?;
                selected_cards.push(RealAtomicScanoutCardTargetSet {
                    card: probed.card,
                    selections,
                });
            }
            admitted.push(probed.admitted);
        }
        let selection = RealAtomicScanoutSelectionSet {
            status: RealAtomicScanoutSelectionSetStatus::SelectedAll,
            cards: selected_cards,
            connected_connectors: records.len(),
        };
        Ok(LiveResolvedOutputReplacement {
            admitted,
            selection,
            records,
            grouping,
            requests,
        })
    }
}

fn validate_requests(
    probes: &[LiveNativeOutputProbe],
    requests: Vec<LiveNativeOutputRequest>,
) -> io::Result<BTreeMap<String, LiveNativeOutputRequest>> {
    if requests.is_empty() || requests.len() > sophia_engine::MAX_DRM_KMS_OUTPUTS {
        return Err(io::Error::other(
            "resolved output set must be nonempty and bounded",
        ));
    }
    let mut unique = BTreeMap::new();
    for request in requests {
        let probe = probes
            .iter()
            .find(|probe| probe.connector == request.connector)
            .ok_or_else(|| io::Error::other("resolved connector was not admitted"))?;
        if !probe.connected
            || !probe.usable
            || !probe.modes.contains(&request.mode)
            || !(1..=8).contains(&request.scale)
            || (request.vrr != sophia_protocol::OutputVrrPolicy::Disabled && !probe.vrr_capable)
        {
            return Err(io::Error::other(
                "resolved connector settings exceed discovered capabilities",
            ));
        }
        if unique.insert(request.connector.clone(), request).is_some() {
            return Err(io::Error::other("resolved connector has two owners"));
        }
    }
    for request in unique.values() {
        if let Some(primary) = &request.mirror_of {
            let primary = unique
                .get(primary)
                .filter(|primary| {
                    primary.mirror_of.is_none() && primary.connector != request.connector
                })
                .ok_or_else(|| io::Error::other("resolved mirror has no primary"))?;
            if primary.scale != request.scale || primary.vrr != request.vrr {
                return Err(io::Error::other("resolved mirror settings disagree"));
            }
        }
    }
    Ok(unique)
}

impl LiveProductionNativeScanout {
    pub fn from_resolved_replacement(
        replacement: LiveResolvedOutputReplacement,
        mapping: sophia_protocol::OutputHeadMapping,
        cursor: sophia_engine::CursorAsset,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        for card in &replacement.admitted {
            card.validate_current(&card.seat)?;
        }
        let mut native = Self::new_with_selection_and_settings(
            replacement.selection,
            replacement.records,
            &replacement.grouping,
            mapping,
            &replacement.requests,
        )?;
        for group in &mut native.groups {
            group.session.set_hardware_cursor_asset(cursor.clone())?;
        }
        #[cfg(feature = "drm-hotplug")]
        {
            native.image_import_devices =
                crate::drm::discover_render_devices_on_cards(&replacement.admitted)?
                    .into_iter()
                    .map(|device| device.file)
                    .collect();
        }
        native.refresh_allocation_devices();
        Ok(native)
    }
}

#[path = "../../../../tests/support/native_output_discovery.rs"]
mod tests;

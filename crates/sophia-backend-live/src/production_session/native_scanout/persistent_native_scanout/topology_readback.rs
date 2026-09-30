//! Opt-in physical proof reads from the already owned DRM cards. Framebuffer
//! and mode-blob handles are intentionally excluded: restoration allocates new
//! resources, so resource identity is not display-state equality.
use super::LiveProductionNativeScanout;
use drm::control::{Device, ResourceHandle};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionOutputKmsReadback {
    pub head: sophia_engine::RenderHeadId,
    pub card: usize,
    pub connector: u32,
    pub crtc: u32,
    pub plane: u32,
    /// Effective timing tuple in evidence order. Mode names and DRIVER /
    /// PREFERRED metadata do not describe programmed timing and are excluded.
    pub mode: Option<[u32; 13]>,
    /// Names include the object class; absent optional properties stay absent.
    pub properties: BTreeMap<String, u64>,
}

impl LiveProductionNativeScanout {
    /// Read actual routing, timing and plane state without submitting KMS.
    /// This uses existing device custody and never opens or discovers a card.
    pub fn output_topology_kms_readback(
        &self,
    ) -> std::io::Result<Vec<LiveProductionOutputKmsReadback>> {
        let mut records = Vec::with_capacity(self.heads.len());
        for head in &self.heads {
            let card = self.groups[head.group].session.card();
            let connector = head.selection.connector_handle();
            let crtc = head.selection.crtc_handle();
            let plane = head.selection.plane_handle();
            let mut properties = BTreeMap::new();
            read_properties(
                card,
                connector,
                "connector",
                &["CRTC_ID"],
                &[],
                &mut properties,
            )?;
            read_properties(
                card,
                crtc,
                "crtc",
                &["ACTIVE"],
                &["VRR_ENABLED"],
                &mut properties,
            )?;
            read_properties(
                card,
                plane,
                "plane",
                &[
                    "CRTC_ID", "SRC_X", "SRC_Y", "SRC_W", "SRC_H", "CRTC_X", "CRTC_Y", "CRTC_W",
                    "CRTC_H",
                ],
                &["rotation"],
                &mut properties,
            )?;
            let mode = if properties["crtc.ACTIVE"] == 0 {
                None
            } else {
                let mode = card.get_crtc(crtc)?.mode().ok_or_else(|| {
                    std::io::Error::other("active output proof CRTC has no readable mode")
                })?;
                let (width, height) = mode.size();
                let (hs, he, ht) = mode.hsync();
                let (vs, ve, vt) = mode.vsync();
                Some([
                    u32::from(width),
                    u32::from(height),
                    mode.vrefresh(),
                    mode.clock(),
                    u32::from(hs),
                    u32::from(he),
                    u32::from(ht),
                    u32::from(vs),
                    u32::from(ve),
                    u32::from(vt),
                    u32::from(mode.hskew()),
                    u32::from(mode.vscan()),
                    mode.flags().bits(),
                ])
            };
            records.push(LiveProductionOutputKmsReadback {
                head: head.head,
                card: head.group,
                connector: connector.into(),
                crtc: crtc.into(),
                plane: plane.into(),
                mode,
                properties,
            });
        }
        records.sort_by_key(|record| record.head);
        Ok(records)
    }
}

fn read_properties<D: Device, H: ResourceHandle>(
    device: &D,
    handle: H,
    object: &str,
    required: &[&str],
    optional: &[&str],
    target: &mut BTreeMap<String, u64>,
) -> std::io::Result<()> {
    let properties = device.get_properties(handle)?;
    let mut observed = Vec::new();
    for (property, value) in properties.iter() {
        let information = device.get_property(*property)?;
        if let Ok(name) = information.name().to_str() {
            observed.push((name.to_owned(), *value));
        }
    }
    select_properties(object, required, optional, observed, target)
}

pub(super) fn select_properties(
    object: &str,
    required: &[&str],
    optional: &[&str],
    observed: impl IntoIterator<Item = (String, u64)>,
    target: &mut BTreeMap<String, u64>,
) -> std::io::Result<()> {
    for (name, value) in observed {
        if (required.contains(&name.as_str()) || optional.contains(&name.as_str()))
            && target.insert(format!("{object}.{name}"), value).is_some()
        {
            return Err(std::io::Error::other(
                "output proof property name is ambiguous",
            ));
        }
    }
    for name in required {
        if !target.contains_key(&format!("{object}.{name}")) {
            return Err(std::io::Error::other(format!(
                "output proof cannot read required property {object}.{name}"
            )));
        }
    }
    Ok(())
}

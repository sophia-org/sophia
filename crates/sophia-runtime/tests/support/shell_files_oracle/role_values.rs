use super::*;
impl Fixture {
    pub(super) fn opening(&self) -> NativeLauncherOpening {
        NativeLauncherOpening {
            grant: self.grant,
            opening: 7,
            output: output(),
            catalog_generation: self.catalog_generation,
            state_revision: 1,
        }
    }
    pub(super) fn catalog(&self) -> ShellApplicationCatalog {
        self.persistent_catalog(false).catalog
    }
    pub(super) fn persistent_catalog(&self, maximum: bool) -> ShellPersistentCatalog {
        let entries = (1..=if maximum { 4096 } else { 3 })
            .map(|slot| ShellApplicationDescriptor {
                slot,
                available: slot != 3,
                label: format!("app{slot}-{}", self.catalog_generation),
                keywords: String::new(),
            })
            .collect::<Vec<_>>();
        let identities = if self.native {
            Default::default()
        } else {
            entries
                .iter()
                .map(|e| (e.slot, format!("registered:app{}", e.slot)))
                .collect()
        };
        ShellPersistentCatalog {
            catalog: ShellApplicationCatalog {
                connection_epoch: self.grant.connection_epoch,
                generation: self.catalog_generation,
                entries,
            },
            identities,
        }
    }
    pub(super) fn indicators(&self, changed: bool) -> ShellIndicatorSnapshot {
        ShellIndicatorSnapshot {
            connection_epoch: self.grant.connection_epoch,
            generation: self.indicator_generation,
            active_output: Some(OutputId::from_raw(2)),
            statuses: vec![ShellOutputStatus {
                output: OutputId::from_raw(2),
                focus_bits: if changed { 2 } else { 1 },
                layout: "tile".into(),
            }],
            indicators: vec![
                ShellIndicator {
                    output: OutputId::from_raw(2),
                    indicator: 1,
                    action: 2,
                    slot: 0,
                    state_bits: 1,
                    label: "clock".into(),
                },
                ShellIndicator {
                    output: OutputId::from_raw(2),
                    indicator: 2,
                    action: 0,
                    slot: 1,
                    state_bits: 0,
                    label: "status".into(),
                },
            ],
        }
    }
    pub(super) fn allocation(&self) -> ContentAllocationSnapshot {
        ContentAllocationSnapshot {
            native_opening: if self.native { Some(7) } else { None },
            output: output(),
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            scale_generation: 5,
            scale_numerator: 1,
            scale_denominator: 1,
            role: if self.native { 3 } else { 1 },
            edge: 1,
            margins: ContentMargins::default(),
            logical: ContentLogicalRect {
                x: 0,
                y: 0,
                width: 64,
                height: 32,
            },
            pixel: ContentPixelRect {
                x: 0,
                y: 0,
                width: 64,
                height: 32,
            },
            parent: ContentAllocationId::default(),
            anchor_parent_rect: ContentPixelRect::default(),
            allowed_reservation_extent: if self.native { 0 } else { 32 },
        }
    }
}

use super::*;
use std::fs::File;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

mod identity;

/// Identity of the retained render node after physical card-to-render mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRenderDeviceNodeIdentity {
    pub device: u64,
    pub inode: u64,
    pub device_number: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveOutputAllocationFormatPreference {
    pub format: u32,
    pub modifiers: Vec<u64>,
}

/// Native allocation identity; restoring a target does not restore its generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveOutputAllocationContext {
    pub generation: u64,
    pub head: sophia_engine::RenderHeadId,
    pub target_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveOutputAllocationPreference {
    pub output: OutputId,
    pub device_number: u64,
    pub identity: Option<LiveRenderDeviceNodeIdentity>,
    pub context: Option<LiveOutputAllocationContext>,
    pub formats: Vec<LiveOutputAllocationFormatPreference>,
}

fn allocation_format_preferences(
    snapshot: &crate::LibdrmNativePlaneFormatSnapshot,
) -> Vec<LiveOutputAllocationFormatPreference> {
    use ::drm::buffer::DrmFourcc;

    [DrmFourcc::Xrgb8888, DrmFourcc::Argb8888]
        .into_iter()
        .filter_map(|format| {
            let modifiers = snapshot
                .modifiers(format)?
                .iter()
                .copied()
                .map(u64::from)
                .filter(|modifier| {
                    !matches!(
                        *modifier,
                        sophia_protocol::DRM_FORMAT_MOD_INVALID | u64::MAX
                    )
                })
                .collect::<Vec<_>>();
            (!modifiers.is_empty()).then_some(LiveOutputAllocationFormatPreference {
                format: format as u32,
                modifiers,
            })
        })
        .collect()
}

pub(super) struct LiveRenderDeviceState {
    group_devices: Vec<Option<LiveRenderDeviceNodeIdentity>>,
    context_generation: Option<u64>,
    pub(super) generation: u64,
    pub(super) pending: BTreeMap<usize, u64>,
    pub(super) applied: BTreeMap<usize, u64>,
}

// Process-wide identities survive native-owner reconstruction without wrapping.
static NEXT_ALLOCATION_CONTEXT: AtomicU64 = AtomicU64::new(1);

fn next_allocation_context(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .ok()
}

impl LiveRenderDeviceState {
    pub(super) fn group_identity(&self, group: usize) -> Option<LiveRenderDeviceNodeIdentity> {
        self.group_devices.get(group).copied().flatten()
    }
    pub(super) fn new() -> Self {
        Self {
            group_devices: Vec::new(),
            context_generation: next_allocation_context(&NEXT_ALLOCATION_CONTEXT),
            generation: 0,
            pending: BTreeMap::new(),
            applied: BTreeMap::new(),
        }
    }

    pub(super) fn invalidate_context(&mut self) {
        self.context_generation = next_allocation_context(&NEXT_ALLOCATION_CONTEXT);
    }

    fn output_context(
        &self,
        heads: &[LiveProductionNativeHead],
        output: OutputId,
        preparing: bool,
    ) -> Option<(LiveOutputAllocationContext, LiveRenderDeviceNodeIdentity)> {
        if preparing {
            return None;
        }
        let mut members = heads.iter().filter(|head| head.output.id == output);
        let head = members.next()?;
        if !head.enabled || members.next().is_some() {
            return None;
        }
        Some((
            LiveOutputAllocationContext {
                generation: self.context_generation?,
                head: head.head,
                target_generation: head.target_generation,
            },
            self.group_identity(head.group)?,
        ))
    }

    fn output_preference(
        &self,
        heads: &[LiveProductionNativeHead],
        output: OutputId,
        preparing: bool,
    ) -> Option<LiveOutputAllocationPreference> {
        let mut members = heads
            .iter()
            .filter(|head| head.enabled && head.output.id == output);
        let head = members.next()?;
        if members.next().is_some() {
            return None;
        }
        let identity = self.group_identity(head.group)?;
        let formats = allocation_format_preferences(&head.format_capabilities.snapshot);
        if formats.is_empty() {
            return None;
        }
        Some(LiveOutputAllocationPreference {
            output,
            device_number: identity.device_number,
            identity: Some(identity),
            context: self
                .output_context(heads, output, preparing)
                .map(|(context, _)| context),
            formats,
        })
    }
}

fn physical_device(file: &File) -> Option<(LiveRenderDeviceNodeIdentity, std::path::PathBuf)> {
    let (metadata, physical) = identity::physical_device(file)?;
    Some((
        LiveRenderDeviceNodeIdentity {
            device: metadata.st_dev,
            inode: metadata.st_ino,
            device_number: metadata.st_rdev,
        },
        physical,
    ))
}

impl LiveProductionNativeScanout {
    pub(super) fn refresh_allocation_devices(&mut self) {
        self.invalidate_layout_probes();
        let render_devices = self
            .image_import_devices
            .iter()
            .filter_map(physical_device)
            .collect::<Vec<_>>();
        self.render_devices.group_devices = self
            .groups
            .iter()
            .map(|group| {
                let card = group.session.card().try_clone_file().ok()?;
                let (_, physical) = physical_device(&card)?;
                let mut matching = render_devices.iter().filter(|(_, path)| *path == physical);
                let device = matching.next()?.0;
                matching.next().is_none().then_some(device)
            })
            .collect();
    }

    /// Cached plane preferences. Mirrors offer no single-device flip preference.
    pub fn output_allocation_preferences(&self) -> Vec<LiveOutputAllocationPreference> {
        self.logical_outputs
            .iter()
            .filter_map(|output| {
                self.render_devices.output_preference(
                    &self.heads,
                    output.id,
                    self.output_topology_preparation.is_some(),
                )
            })
            .collect()
    }

    /// Current single-head identity without allocating or rebuilding format rows.
    pub fn output_allocation_context(
        &self,
        output: OutputId,
    ) -> Option<(LiveOutputAllocationContext, LiveRenderDeviceNodeIdentity)> {
        if !self
            .logical_outputs
            .iter()
            .any(|current| current.id == output)
        {
            return None;
        }
        self.render_devices.output_context(
            &self.heads,
            output,
            self.output_topology_preparation.is_some(),
        )
    }

    /// Replaces source-device candidates without replacing output or image ownership.
    pub fn request_image_import_inventory(
        &mut self,
        generation: u64,
        devices: Vec<File>,
    ) -> io::Result<()> {
        if generation == 0 || generation <= self.render_devices.generation {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stale render inventory",
            ));
        }
        if devices.len() > 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "render inventory capacity",
            ));
        }
        self.image_import_devices = devices;
        self.preview_images.invalidate_foreign();
        self.render_devices.generation = generation;
        self.refresh_allocation_devices();
        Ok(())
    }

    pub fn image_import_inventory_generation(&self) -> u64 {
        self.render_devices.generation
    }

    /// Polls bounded worker commands; queued renders and retained images stay owned.
    pub fn poll_image_import_inventory(&mut self) -> io::Result<bool> {
        let generation = self.render_devices.generation;
        if generation == 0 {
            return Ok(true);
        }
        let shared = Self::shared_renderer_worker_enabled();
        let mut visited = BTreeSet::new();
        let mut complete = true;
        for index in 0..self.heads.len() {
            if !self.heads[index].enabled || !self.exporters[index].worker_enabled() {
                continue;
            }
            let key = if shared {
                self.heads[index].group
            } else {
                index
            };
            if !visited.insert(key) {
                continue;
            }
            if self.render_devices.pending.contains_key(&key) {
                let result = if shared {
                    self.groups[key]
                        .renderer_core
                        .as_ref()
                        .ok_or_else(|| io::Error::other("renderer core unavailable"))?
                        .poll_image_import_device_replacement()
                } else {
                    self.exporters[index].poll_image_import_device_replacement()
                };
                match result {
                    Ok(Some(applied)) => {
                        self.render_devices.pending.remove(&key);
                        self.render_devices.applied.insert(key, applied);
                    }
                    Ok(None) => {
                        complete = false;
                        continue;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        self.render_devices.pending.remove(&key);
                        complete = false;
                        continue;
                    }
                    Err(error) => {
                        self.render_devices.pending.remove(&key);
                        return Err(error);
                    }
                }
            }
            if self.render_devices.applied.get(&key) == Some(&generation) {
                continue;
            }
            let devices = self.image_import_device_fds()?;
            let requested = if shared {
                self.groups[key]
                    .renderer_core
                    .as_ref()
                    .ok_or_else(|| io::Error::other("renderer core unavailable"))?
                    .request_image_import_device_replacement(generation, devices)
            } else {
                self.exporters[index].request_image_import_device_replacement(generation, devices)
            };
            match requested {
                Ok(()) => {
                    self.render_devices.pending.insert(key, generation);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            complete = false;
        }
        Ok(complete)
    }
}

#[path = "../../../../tests/support/output_allocation_formats.rs"]
mod tests;

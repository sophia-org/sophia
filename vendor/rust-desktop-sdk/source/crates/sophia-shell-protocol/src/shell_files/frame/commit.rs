use super::*;
use crate::shell::encoding::content::{content_candidate_bytes, put_candidate};

pub(super) fn body_bytes(value: &ShellFrameCommit) -> Result<usize, ShellFilePayloadError> {
    let scene = &value.candidate;
    if scene.surfaces.len() != 1
        || scene.placements.iter().any(|p| p.surface_index != 0)
        || scene.targets.iter().any(|t| t.surface_index != 0)
    {
        return Err(ShellFilePayloadError::Value);
    }
    let scene_bytes = content_candidate_bytes(scene)?;
    if let Some(offer) = value.image {
        offer.validate().map_err(|_| ShellFilePayloadError::Value)?;
        let allocation = scene.surfaces[0].allocation;
        if offer.slot >= 2
            || offer
                .resource_bytes()
                .is_none_or(|n| n > OWNED_RING_MAX_CAPACITY_BYTES)
            || offer.identity.connection_epoch != scene.grant.connection_epoch
            || offer.identity.content_grant_epoch != scene.grant.content_grant_epoch
            || offer.allocation != allocation.id
            || offer.allocation_generation != allocation.generation
            || offer.output != scene.output.id
            || offer.output_generation != scene.output.generation
            || offer.topology_generation != scene.facts_generation
            || !scene.placements.iter().any(|p| {
                p.resource.id == offer.resource
                    && p.resource.generation == offer.resource_generation
            })
        {
            return Err(ShellFilePayloadError::Value);
        }
    }
    if let Some(retire) = value.retire {
        nonzero(&[
            retire.transaction.raw(),
            retire.resource.id,
            retire.resource.generation,
        ])?;
        if retire.transaction == value.transaction
            || scene
                .placements
                .iter()
                .any(|p| p.resource == retire.resource)
        {
            return Err(ShellFilePayloadError::Value);
        }
    }
    scene_bytes
        .checked_add(
            16 + if value.image.is_some() { 152 } else { 0 }
                + if value.retire.is_some() { 24 } else { 0 },
        )
        .ok_or(ShellFileCodecError::Length.into())
}

pub(super) fn put(value: &ShellFrameCommit, bytes: &mut Vec<u8>) {
    let scene = &value.candidate;
    let scene_bytes =
        72 + scene.surfaces.len() * 64 + scene.placements.len() * 32 + scene.targets.len() * 48;
    bytes.extend_from_slice(&(scene_bytes as u32).to_le_bytes());
    let flags = u16::from(value.image.is_some()) | (u16::from(value.retire.is_some()) << 1);
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    put_candidate(scene, bytes);
    if let Some(image) = value.image {
        image.append(bytes);
    }
    if let Some(retire) = value.retire {
        words(
            bytes,
            &[
                retire.transaction.raw(),
                retire.resource.id,
                retire.resource.generation,
            ],
        );
    }
}

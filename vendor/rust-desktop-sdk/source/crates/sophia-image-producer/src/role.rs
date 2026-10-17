use sophia_lock_protocol::lock_files::{
    LockImageDiscovery, LockImageExpect, LockImageFormats, LockImageLimits, LockImageMetadata,
    LockImagePacket, LockImagePlane, LockImageRoleBounds,
};
use sophia_shell_protocol::shell_files::{
    ShellImageDiscovery, ShellImageExpect, ShellImageFormats, ShellImageLimits, ShellImageMetadata,
    ShellImagePacket, ShellImagePlane, ShellImageRoleBounds,
};

use crate::{ImageRole, ProducerError, ProducerMetadata, ProducerRoleBounds};

/// The role codec's packet expectation after discovery coherence.
pub(crate) enum Expect {
    Shell(ShellImageExpect),
    Lock(LockImageExpect),
}

impl Expect {
    pub(crate) fn channel_generation(&self) -> u64 {
        match self {
            Self::Shell(expect) => expect.channel_generation,
            Self::Lock(expect) => expect.channel_generation,
        }
    }
    pub(crate) fn source_kinds(&self) -> u32 {
        match self {
            Self::Shell(expect) => expect.granted_source_kinds,
            Self::Lock(expect) => expect.granted_source_kinds,
        }
    }
    pub(crate) fn set_source_kinds(&mut self, kinds: u32) {
        match self {
            Self::Shell(expect) => expect.granted_source_kinds = kinds,
            Self::Lock(expect) => expect.granted_source_kinds = kinds,
        }
    }
}

/// Decode one discovery set through the role codec. Only a
/// generation mismatch between the three objects returns `Again`;
/// every other failure is `Refused` for the caller to make fatal.
/// One borrowed discovery set: the channel text and both object
/// bodies, with the connection epochs their record headers carried.
pub(crate) struct DiscoverySet<'a> {
    pub(crate) text: &'a [u8],
    pub(crate) limits_body: &'a [u8],
    pub(crate) formats_body: &'a [u8],
    pub(crate) limits_connection_epoch: u64,
    pub(crate) formats_connection_epoch: u64,
}

pub(crate) fn decode_discovery(
    role: ImageRole,
    connection_epoch: u64,
    bounds: &ProducerRoleBounds,
    set: DiscoverySet<'_>,
) -> Result<(Expect, u32, u32), ProducerError> {
    let DiscoverySet {
        text,
        limits_body,
        formats_body,
        limits_connection_epoch,
        formats_connection_epoch,
    } = set;
    let text = std::str::from_utf8(text).map_err(|_| ProducerError::Refused)?;
    match role {
        ImageRole::Shell => {
            let discovery =
                ShellImageDiscovery::decode(text).map_err(|_| ProducerError::Refused)?;
            let limits =
                ShellImageLimits::decode(limits_body).map_err(|_| ProducerError::Refused)?;
            let formats =
                ShellImageFormats::decode(formats_body).map_err(|_| ProducerError::Refused)?;
            if discovery.connection_epoch != connection_epoch {
                return Err(ProducerError::Refused);
            }
            if limits.channel_generation != discovery.channel_generation
                || formats.channel_generation != discovery.channel_generation
                || limits.limits_generation != discovery.limits_generation
                || formats.limits_generation != discovery.limits_generation
                || formats.formats_generation != discovery.formats_generation
            {
                return Err(ProducerError::Again);
            }
            let role_bounds = ShellImageRoleBounds {
                max_resource_bytes: bounds.max_resource_bytes,
                max_width: bounds.max_width,
                max_height: bounds.max_height,
                max_admitted_outputs: bounds.max_admitted_outputs,
                transfer_slots: bounds.transfer_slots,
            };
            discovery
                .coherent(
                    &limits,
                    &formats,
                    limits_connection_epoch,
                    formats_connection_epoch,
                    &role_bounds,
                )
                .map(|expect| {
                    (
                        Expect::Shell(expect),
                        discovery.hello_timeout_ms,
                        discovery.max_offers,
                    )
                })
                .map_err(|_| ProducerError::Refused)
        }
        ImageRole::Lock => {
            let discovery = LockImageDiscovery::decode(text).map_err(|_| ProducerError::Refused)?;
            let limits =
                LockImageLimits::decode(limits_body).map_err(|_| ProducerError::Refused)?;
            let formats =
                LockImageFormats::decode(formats_body).map_err(|_| ProducerError::Refused)?;
            if discovery.connection_epoch != connection_epoch {
                return Err(ProducerError::Refused);
            }
            if limits.channel_generation != discovery.channel_generation
                || formats.channel_generation != discovery.channel_generation
                || limits.limits_generation != discovery.limits_generation
                || formats.limits_generation != discovery.limits_generation
                || formats.formats_generation != discovery.formats_generation
            {
                return Err(ProducerError::Again);
            }
            let role_bounds = LockImageRoleBounds {
                max_resource_bytes: bounds.max_resource_bytes,
                max_width: bounds.max_width,
                max_height: bounds.max_height,
                max_admitted_outputs: bounds.max_admitted_outputs,
                transfer_slots: bounds.transfer_slots,
            };
            discovery
                .coherent(
                    &limits,
                    &formats,
                    limits_connection_epoch,
                    formats_connection_epoch,
                    &role_bounds,
                )
                .map(|expect| {
                    (
                        Expect::Lock(expect),
                        discovery.hello_timeout_ms,
                        discovery.max_offers,
                    )
                })
                .map_err(|_| ProducerError::Refused)
        }
    }
}

pub(crate) fn hello_bytes(expect: &Expect) -> Result<Vec<u8>, ()> {
    match expect {
        Expect::Shell(expect) => ShellImagePacket::Hello {
            connection_epoch: expect.connection_epoch,
            channel_generation: expect.channel_generation,
        }
        .encode()
        .map_err(|_| ()),
        Expect::Lock(expect) => LockImagePacket::Hello {
            connection_epoch: expect.connection_epoch,
            channel_generation: expect.channel_generation,
        }
        .encode()
        .map_err(|_| ()),
    }
}

pub(crate) fn source_bytes(expect: &Expect, metadata: ProducerMetadata) -> Result<Vec<u8>, ()> {
    match expect {
        Expect::Shell(expect) => {
            let zero = ShellImagePlane {
                offset: 0,
                stride: 0,
                fd_index: 0,
            };
            ShellImagePacket::Source {
                connection_epoch: expect.connection_epoch,
                channel_generation: expect.channel_generation,
                metadata: to_shell(metadata),
                planes: [
                    ShellImagePlane {
                        offset: 0,
                        stride: metadata.stride,
                        fd_index: 0,
                    },
                    zero,
                    zero,
                    zero,
                ],
            }
            .encode()
            .map_err(|_| ())
        }
        Expect::Lock(expect) => {
            let zero = LockImagePlane {
                offset: 0,
                stride: 0,
                fd_index: 0,
            };
            LockImagePacket::Source {
                connection_epoch: expect.connection_epoch,
                channel_generation: expect.channel_generation,
                metadata: to_lock(metadata),
                planes: [
                    LockImagePlane {
                        offset: 0,
                        stride: metadata.stride,
                        fd_index: 0,
                    },
                    zero,
                    zero,
                    zero,
                ],
            }
            .encode()
            .map_err(|_| ())
        }
    }
}

/// Admissibility under the discovered limits and the grant.
pub(crate) fn admissible(expect: &Expect, metadata: ProducerMetadata) -> bool {
    match expect {
        Expect::Shell(expect) => to_shell(metadata).admissible(expect).is_ok(),
        Expect::Lock(expect) => to_lock(metadata).admissible(expect).is_ok(),
    }
}

/// The 96-byte ImageBegin body for the role.
pub(crate) fn metadata_body(role: ImageRole, metadata: ProducerMetadata) -> Result<[u8; 96], ()> {
    match role {
        ImageRole::Shell => to_shell(metadata).encode().map_err(|_| ()),
        ImageRole::Lock => to_lock(metadata).encode().map_err(|_| ()),
    }
}

pub(crate) fn to_shell(metadata: ProducerMetadata) -> ShellImageMetadata {
    ShellImageMetadata {
        offer_id: metadata.offer_id,
        resource: metadata.resource,
        resource_generation: metadata.resource_generation,
        admission_epoch: metadata.admission_epoch,
        output_id: metadata.output_id,
        allocation_id: metadata.allocation_id,
        topology_generation: metadata.topology_generation,
        source_kind: metadata.source_kind,
        source_format: metadata.source_format,
        width: metadata.width,
        height: metadata.height,
        stride: metadata.stride,
        plane_count: metadata.plane_count,
        descriptor_count: metadata.descriptor_count,
        declared_bytes: metadata.declared_bytes,
        modifier: metadata.modifier,
    }
}

pub(crate) fn to_lock(metadata: ProducerMetadata) -> LockImageMetadata {
    LockImageMetadata {
        offer_id: metadata.offer_id,
        resource: metadata.resource,
        resource_generation: metadata.resource_generation,
        admission_epoch: metadata.admission_epoch,
        output_id: metadata.output_id,
        allocation_id: metadata.allocation_id,
        topology_generation: metadata.topology_generation,
        source_kind: metadata.source_kind,
        source_format: metadata.source_format,
        width: metadata.width,
        height: metadata.height,
        stride: metadata.stride,
        plane_count: metadata.plane_count,
        descriptor_count: metadata.descriptor_count,
        declared_bytes: metadata.declared_bytes,
        modifier: metadata.modifier,
    }
}

//! Retained output-role peer from the retired sophia-wm-demo at 7c9134eff.
//! It uses Sophia's codec, so it is not an independent implementation. Output
//! IPC remains until t253/t272; this fixture adds no WM transport or policy.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    IpcCodecError, OutputAuthoritySnapshot, OutputGroupMember, OutputHeadDescriptor,
    OutputHeadMapping, OutputHeadTargetProposal, OutputLogicalGroupProposal,
    OutputLogicalGroupState, OutputTopologyCandidate, OutputTopologyCandidateError,
    OutputTopologyIntent, OutputTransform, OutputV1ClientHello, OutputV1Outcome, OutputV1Proposal,
    OutputVrrPolicy, Rect, SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAX_PAYLOAD_LEN,
    SOPHIA_OUTPUT_CAPABILITY_CONFIGURE, SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
    SOPHIA_OUTPUT_INTERFACE_REVISION, TransactionId, decode_output_v1_outcome_frame,
    decode_output_v1_server_welcome_frame, decode_output_v1_snapshot_frame,
    encode_output_v1_client_hello_frame, encode_output_v1_proposal_frame,
};

#[derive(Debug)]
pub enum OutputV1ClientError {
    Io(std::io::Error),
    Codec(IpcCodecError),
    Candidate(OutputTopologyCandidateError),
    UnsupportedRevision(u16),
    MissingCapability,
    InvalidWelcome,
    ConnectionEpochMismatch,
    TransactionMismatch,
    InvalidProofTopology(&'static str),
    TransactionExhausted,
}

impl core::fmt::Display for OutputV1ClientError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "output I/O: {error}"),
            Self::Codec(error) => write!(formatter, "output codec: {error:?}"),
            Self::Candidate(error) => write!(formatter, "output candidate: {error}"),
            Self::UnsupportedRevision(revision) => {
                write!(formatter, "unsupported output revision: {revision}")
            }
            Self::InvalidProofTopology(reason) => {
                write!(formatter, "invalid proof topology: {reason}")
            }
            _ => write!(formatter, "{self:?}"),
        }
    }
}

impl std::error::Error for OutputV1ClientError {}

impl From<std::io::Error> for OutputV1ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<IpcCodecError> for OutputV1ClientError {
    fn from(error: IpcCodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<OutputTopologyCandidateError> for OutputV1ClientError {
    fn from(error: OutputTopologyCandidateError) -> Self {
        Self::Candidate(error)
    }
}

/// Reference client for the exclusive physical-output role.
///
/// Physical head labels remain private to this role. The policy connection
/// still receives only logical outputs and opaque surface identities.
pub struct OutputV1Client {
    stream: UnixStream,
    connection_epoch: u64,
    max_heads: usize,
    max_groups: usize,
    max_modes_per_head: usize,
    max_heads_per_group: usize,
    next_transaction: u64,
}

impl OutputV1Client {
    pub fn connect(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, OutputV1ClientError> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(&encode_output_v1_client_hello_frame(OutputV1ClientHello {
            minimum_revision: SOPHIA_OUTPUT_INTERFACE_REVISION,
            maximum_revision: SOPHIA_OUTPUT_INTERFACE_REVISION,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
        })?)?;
        stream.flush()?;
        let welcome = decode_output_v1_server_welcome_frame(&read_frame(&mut stream)?)?;
        if welcome.selected_revision != SOPHIA_OUTPUT_INTERFACE_REVISION {
            return Err(OutputV1ClientError::UnsupportedRevision(
                welcome.selected_revision,
            ));
        }
        let required = SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE;
        if welcome.capabilities & required != required {
            return Err(OutputV1ClientError::MissingCapability);
        }
        if welcome.connection_epoch == 0
            || welcome.max_heads < 3
            || welcome.max_groups < 2
            || welcome.max_modes_per_head == 0
            || welcome.max_heads_per_group < 2
        {
            return Err(OutputV1ClientError::InvalidWelcome);
        }
        Ok(Self {
            stream,
            connection_epoch: welcome.connection_epoch,
            max_heads: usize::from(welcome.max_heads),
            max_groups: usize::from(welcome.max_groups),
            max_modes_per_head: usize::from(welcome.max_modes_per_head),
            max_heads_per_group: usize::from(welcome.max_heads_per_group),
            next_transaction: 1,
        })
    }

    pub fn receive_snapshot(
        &mut self,
    ) -> Result<(TransactionId, OutputAuthoritySnapshot), OutputV1ClientError> {
        let (transaction, message) =
            decode_output_v1_snapshot_frame(&read_frame(&mut self.stream)?)?;
        if message.connection_epoch != self.connection_epoch {
            return Err(OutputV1ClientError::ConnectionEpochMismatch);
        }
        if message.snapshot.heads.len() > self.max_heads
            || message.snapshot.groups.len() > self.max_groups
            || message
                .snapshot
                .heads
                .iter()
                .any(|head| head.modes.len() > self.max_modes_per_head)
            || message
                .snapshot
                .groups
                .iter()
                .any(|group| group.members.len() > self.max_heads_per_group)
        {
            return Err(OutputV1ClientError::InvalidWelcome);
        }
        message.snapshot.validate()?;
        Ok((transaction, message.snapshot))
    }

    pub fn submit(
        &mut self,
        candidate: OutputTopologyCandidate,
        snapshot: &OutputAuthoritySnapshot,
    ) -> Result<OutputV1Outcome, OutputV1ClientError> {
        candidate.validate_against(snapshot)?;
        let transaction = TransactionId::from_raw(self.next_transaction);
        self.next_transaction = self
            .next_transaction
            .checked_add(1)
            .ok_or(OutputV1ClientError::TransactionExhausted)?;
        let frame = encode_output_v1_proposal_frame(
            transaction,
            &OutputV1Proposal {
                connection_epoch: self.connection_epoch,
                candidate,
            },
        )?;
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        // A snapshot is an unsolicited update and may arrive at any moment,
        // including between a proposal and the outcome answering it. Reading
        // the next frame as an outcome regardless once turned a published
        // topology into a decode failure and took the session down with it, so
        // updates are consumed while waiting rather than tripped over.
        let (outcome_transaction, outcome) = loop {
            let frame = read_frame(&mut self.stream)?;
            match decode_output_v1_snapshot_frame(&frame) {
                Ok(_) => continue,
                Err(_) => break decode_output_v1_outcome_frame(&frame)?,
            }
        };
        if outcome_transaction != transaction {
            return Err(OutputV1ClientError::TransactionMismatch);
        }
        if outcome.connection_epoch != self.connection_epoch {
            return Err(OutputV1ClientError::ConnectionEpochMismatch);
        }
        Ok(outcome)
    }
}

/// How a mirror group chooses its logical size, and what that costs.
///
/// A mirror group has one logical size and its members are placed into it, so a
/// member whose mode differs from that size either resamples to reach it or
/// keeps its own pixels and leaves the remainder unused. There is no third
/// outcome, and which one a group takes is a property of the group rather than a
/// mode change: every head keeps its own mode under all three policies.
///
/// Named for the policy rather than for a head. Two of these do name a head, and
/// the third deliberately optimizes for neither, so a type called "which head is
/// optimized" could not hold it without one of its values meaning something else.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MirrorSizingPolicy {
    /// Size to the primary. It stays pixel-exact and the member resamples down.
    #[default]
    OptimizeForPrimary,
    /// Size to the member. It stays pixel-exact and the primary resamples up.
    OptimizeForMember,
    /// Size so that every member contains the logical image at its own scale.
    ///
    /// Nothing resamples. The size is the smallest mode on each axis across the
    /// group, so it fits inside every member, and `OutputHeadMapping::Exact`
    /// centres it there — a head larger than the image shows a border rather than
    /// a stretch. This is what macOS's "optimize for" cannot express, and it is
    /// the only policy under which both panels are pixel-exact at once.
    ///
    /// The per-axis minimum can name a size no member's mode matches, when two
    /// heads are larger on different axes. That is still correct: every member
    /// contains it exactly, and every member shows a border.
    CenterUnscaled,
}

/// Builds a complete three-head candidate: two heads mirror one logical output,
/// and the third extends it to the right.
///
/// The proof shape is deliberately exact. Extra connected heads are rejected
/// instead of being disabled as a side effect, and all three heads keep their
/// current modes under every policy. The extended head is always exact; within
/// the mirror group, `policy` decides both the logical size and which members
/// reach it by resampling. Head identity comes entirely from the labels this
/// call is given: nothing here knows a connector name or a panel size.
pub fn mixed_mirror_extended_candidate(
    snapshot: &OutputAuthoritySnapshot,
    mirror_primary_label: &str,
    mirror_member_label: &str,
    extended_label: &str,
    policy: MirrorSizingPolicy,
) -> Result<OutputTopologyCandidate, OutputV1ClientError> {
    snapshot.validate()?;
    if mirror_primary_label == mirror_member_label
        || mirror_primary_label == extended_label
        || mirror_member_label == extended_label
    {
        return Err(OutputV1ClientError::InvalidProofTopology(
            "proof labels are not distinct",
        ));
    }
    let connected = snapshot
        .heads
        .iter()
        .filter(|head| head.connected)
        .collect::<Vec<_>>();
    if connected.len() != 3 {
        return Err(OutputV1ClientError::InvalidProofTopology(
            "proof requires exactly three connected heads",
        ));
    }
    let primary = head_by_label(&connected, mirror_primary_label)?;
    let member = head_by_label(&connected, mirror_member_label)?;
    let extended = head_by_label(&connected, extended_label)?;
    for head in [primary, member, extended] {
        if !head.enabled || head.current_mode.is_none() {
            return Err(OutputV1ClientError::InvalidProofTopology(
                "proof head is not enabled with a current mode",
            ));
        }
        if !head.transforms.contains(OutputTransform::Normal) {
            return Err(OutputV1ClientError::InvalidProofTopology(
                "proof head does not support the normal transform",
            ));
        }
    }

    let primary_group = group_for_head(snapshot, primary)?;
    let member_group = group_for_head(snapshot, member)?;
    let extended_group = group_for_head(snapshot, extended)?;
    if primary_group.output == extended_group.output {
        return Err(OutputV1ClientError::InvalidProofTopology(
            "mirror primary and extended head already share one logical output",
        ));
    }
    // A pre-existing mirror is acceptable, but an unrelated shared identity is
    // not: consuming it would silently remove another logical placement.
    if member_group.output != primary_group.output && member_group.output == extended_group.output {
        return Err(OutputV1ClientError::InvalidProofTopology(
            "mirror member currently belongs to the extended output",
        ));
    }

    let mirror_size = mirror_logical_size(primary, member, policy)?;
    let mirror_logical = Rect {
        x: primary_group.logical.x,
        y: primary_group.logical.y,
        width: mirror_size.width,
        height: mirror_size.height,
    };
    let extended_x = mirror_logical.x.checked_add(mirror_logical.width).ok_or(
        OutputV1ClientError::InvalidProofTopology("extended placement overflows root coordinates"),
    )?;
    let (primary_mapping, member_mapping) = mirror_member_mappings(policy);
    let targets = [primary, member, extended]
        .into_iter()
        .map(|head| OutputHeadTargetProposal {
            head: head.head,
            head_generation: head.generation,
            mode: head
                .current_mode
                .expect("enabled proof head has a current mode"),
            transform: OutputTransform::Normal,
            vrr: OutputVrrPolicy::Disabled,
        })
        .collect::<Vec<_>>();
    let candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: targets,
        groups: vec![
            OutputLogicalGroupProposal {
                output: primary_group.output,
                logical: mirror_logical,
                members: vec![
                    OutputGroupMember {
                        head: primary.head,
                        mapping: primary_mapping,
                    },
                    OutputGroupMember {
                        head: member.head,
                        mapping: member_mapping,
                    },
                ],
            },
            OutputLogicalGroupProposal {
                output: extended_group.output,
                logical: Rect {
                    x: extended_x,
                    y: mirror_logical.y,
                    width: extended_group.logical.width,
                    height: extended_group.logical.height,
                },
                members: vec![OutputGroupMember {
                    head: extended.head,
                    mapping: OutputHeadMapping::Exact,
                }],
            },
        ],
    };
    candidate.validate_against(snapshot)?;
    Ok(candidate)
}

/// Whether the snapshot already shows the topology this proof asks for.
///
/// A supervised policy is restarted, and a restart lands after the topology it
/// applied is already live. Re-submitting the candidate it built the first time
/// names a base epoch the compositor has moved past, which is refused as stale
/// -- correctly, and fatally for a proof that reads any non-commit as failure.
/// So the proof asks first whether the desk already looks the way it wants,
/// which is the only question that survives being asked twice.
pub fn mixed_mirror_extended_topology_is_applied(
    snapshot: &OutputAuthoritySnapshot,
    mirror_primary_label: &str,
    mirror_member_label: &str,
    extended_label: &str,
    policy: MirrorSizingPolicy,
) -> bool {
    let connected = snapshot
        .heads
        .iter()
        .filter(|head| head.connected)
        .collect::<Vec<_>>();
    let (Ok(primary), Ok(member), Ok(extended)) = (
        head_by_label(&connected, mirror_primary_label),
        head_by_label(&connected, mirror_member_label),
        head_by_label(&connected, extended_label),
    ) else {
        return false;
    };
    let (Ok(primary_group), Ok(member_group), Ok(extended_group)) = (
        group_for_head(snapshot, primary),
        group_for_head(snapshot, member),
        group_for_head(snapshot, extended),
    ) else {
        return false;
    };
    if primary_group.output != member_group.output
        || primary_group.output == extended_group.output
        || extended_group.members.len() != 1
    {
        return false;
    }
    let (primary_mapping, member_mapping) = mirror_member_mappings(policy);
    // The size, not only the mappings. Two exact members sized to the larger
    // head crop the smaller one instead of bordering it, and that configuration
    // wears the same pair of mappings as the one that does not.
    let Ok(expected_size) = mirror_logical_size(primary, member, policy) else {
        return false;
    };
    if primary_group.logical.width != expected_size.width
        || primary_group.logical.height != expected_size.height
    {
        return false;
    }
    let mapping_of = |head: &OutputHeadDescriptor| {
        primary_group
            .members
            .iter()
            .find(|candidate| candidate.head == head.head)
            .map(|candidate| candidate.mapping)
    };
    mapping_of(primary) == Some(primary_mapping) && mapping_of(member) == Some(member_mapping)
}

/// The logical size a mirror group takes under a policy.
///
/// Shared by the builder and the applied-topology predicate on purpose. The two
/// used to agree only because the predicate did not look at the size at all, and
/// a policy whose whole point is which size gets chosen cannot be confirmed by
/// reading the mappings alone.
fn mirror_logical_size(
    primary: &OutputHeadDescriptor,
    member: &OutputHeadDescriptor,
    policy: MirrorSizingPolicy,
) -> Result<sophia_protocol::Size, OutputV1ClientError> {
    match policy {
        MirrorSizingPolicy::OptimizeForPrimary => current_mode_pixel_size(primary),
        MirrorSizingPolicy::OptimizeForMember => current_mode_pixel_size(member),
        MirrorSizingPolicy::CenterUnscaled => {
            let primary_size = current_mode_pixel_size(primary)?;
            let member_size = current_mode_pixel_size(member)?;
            // Per axis, not whichever head is smaller overall: a group of a
            // 2560x1080 and a 1920x1440 head has no smaller member, and taking
            // either one whole would leave the other cropped by `clip_to_target`
            // rather than bordered. The minimum on each axis is the largest
            // rectangle both heads contain.
            Ok(sophia_protocol::Size {
                width: primary_size.width.min(member_size.width),
                height: primary_size.height.min(member_size.height),
            })
        }
    }
}

/// The mappings a mirror group's two members take under a policy.
///
/// `Exact` means the same placement in all three: take the logical size verbatim
/// and centre it. What differs is whether there is any remainder to leave, which
/// is decided by `mirror_logical_size` rather than here.
const fn mirror_member_mappings(
    policy: MirrorSizingPolicy,
) -> (OutputHeadMapping, OutputHeadMapping) {
    match policy {
        MirrorSizingPolicy::OptimizeForPrimary => {
            (OutputHeadMapping::Exact, OutputHeadMapping::Fit)
        }
        MirrorSizingPolicy::OptimizeForMember => (OutputHeadMapping::Fit, OutputHeadMapping::Exact),
        MirrorSizingPolicy::CenterUnscaled => (OutputHeadMapping::Exact, OutputHeadMapping::Exact),
    }
}

/// The pixel size of the mode this head is currently running.
fn current_mode_pixel_size(
    head: &OutputHeadDescriptor,
) -> Result<sophia_protocol::Size, OutputV1ClientError> {
    let mode = head
        .current_mode
        .ok_or(OutputV1ClientError::InvalidProofTopology(
            "proof head is not enabled with a current mode",
        ))?;
    head.modes
        .iter()
        .find(|descriptor| descriptor.mode == mode)
        .map(|descriptor| descriptor.pixel_size)
        .ok_or(OutputV1ClientError::InvalidProofTopology(
            "proof head reports a current mode it does not advertise",
        ))
}

fn head_by_label<'a>(
    heads: &[&'a OutputHeadDescriptor],
    label: &str,
) -> Result<&'a OutputHeadDescriptor, OutputV1ClientError> {
    let mut matches = heads.iter().copied().filter(|head| head.label == label);
    let head = matches
        .next()
        .ok_or(OutputV1ClientError::InvalidProofTopology(
            "proof label is absent",
        ))?;
    if matches.next().is_some() {
        return Err(OutputV1ClientError::InvalidProofTopology(
            "proof label is ambiguous",
        ));
    }
    Ok(head)
}

fn group_for_head<'a>(
    snapshot: &'a OutputAuthoritySnapshot,
    head: &OutputHeadDescriptor,
) -> Result<&'a OutputLogicalGroupState, OutputV1ClientError> {
    snapshot
        .groups
        .iter()
        .find(|group| group.members.iter().any(|member| member.head == head.head))
        .ok_or(OutputV1ClientError::InvalidProofTopology(
            "proof head has no logical output",
        ))
}

fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>, OutputV1ClientError> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header)?;
    let payload_len = u32::from_le_bytes(
        header[16..20]
            .try_into()
            .expect("fixed frame payload range is present"),
    ) as usize;
    if payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(OutputV1ClientError::Codec(IpcCodecError::PayloadTooLarge(
            payload_len,
        )));
    }
    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload_len);
    frame.extend_from_slice(&header);
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream.read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])?;
    Ok(frame)
}

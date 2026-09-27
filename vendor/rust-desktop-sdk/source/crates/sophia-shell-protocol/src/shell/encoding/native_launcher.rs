//! Value encoding for the revision-7 native launcher records
//! (`crate::shell::native_launcher`): the per-variant body encode and decode
//! of a [`ShellNativeLauncherRecord`].
//!
//! The frame codec's message-kind enum, frame headers and transaction-id
//! rules stay with the frame codec, which maps its own message kinds onto
//! [`ShellNativeLauncherValueKind`] and wraps [`ValueError`] into its own
//! error type at the boundary.
use super::{Wire, fields, reserved, rows, table_count};
use crate::byte_cursor::Cursor;
use crate::shell::encoding::ValueError;
use crate::*;

fields!(NativeLauncherOpening {
    grant: ContentGrant,
    opening: u64,
    output: ContentOutputId,
    catalog_generation: u64,
    state_revision: u64,
});
fields!(NativeLauncherAllocationRequest {
    grant: ContentGrant,
    opening: u64,
    output: ContentOutputId,
    request_id: u64,
    prior: ContentAllocationId,
    operation: u16,
    edge: u16,
    desired_width: u32,
    desired_height: u32,
    margins: ContentMargins,
});
fields!(NativeLauncherBinding {
    grant: ContentGrant,
    opening: u64,
    output: ContentOutputId,
    allocation: ContentAllocationId,
    catalog_generation: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    interaction_generation: u64,
    state_revision: u64,
    focus_lease: u64,
});
fields!(NativeLauncherEvent {
    binding: NativeLauncherBinding,
    event_id: u64,
    state_revision: u64,
});
fields!(NativeLauncherActivation {
    event: NativeLauncherEvent,
    cause: u16,
    slot: u16,
});
fields!(NativeLauncherActivationOutcome {
    activation: NativeLauncherActivation,
    status: u16,
    reason: u16,
});

macro_rules! reserved_tail {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        impl Wire for $name {
            fn put(&self, bytes: &mut Vec<u8>) {
                $(self.$field.put(bytes);)*
                0u16.put(bytes);
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
                let value = Self { $($field: <$ty>::take(cursor)?),* };
                reserved::<u16>(cursor)?;
                Ok(value)
            }
        }
    };
}
reserved_tail!(NativeLauncherFocusRevoked {
    binding: NativeLauncherBinding,
    reason: u16
});
reserved_tail!(NativeLauncherInputAck {
    event: NativeLauncherEvent,
    disposition: u16
});
reserved_tail!(NativeLauncherClosed {
    grant: ContentGrant,
    opening: u64,
    reason: u16
});

impl Wire for NativeLauncherCandidateBegin {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.content.put(bytes);
        self.opening.put(bytes);
        self.catalog_generation.put(bytes);
        self.state_revision.put(bytes);
        self.selected.put(bytes);
        (self.rows.len() as u16).put(bytes);
        for row in &self.rows {
            row.put(bytes);
        }
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let content = ContentCandidateBegin::take(cursor)?;
        let opening = cursor.u64()?;
        let catalog_generation = cursor.u64()?;
        let state_revision = cursor.u64()?;
        let selected = cursor.u16()?;
        let count = usize::from(cursor.u16()?);
        if count > SOPHIA_SHELL_MAX_LAUNCHER_ROWS {
            return Err(ValueError::CountTooLarge {
                count,
                max: SOPHIA_SHELL_MAX_LAUNCHER_ROWS,
            });
        }
        // Establish complete bounded payload before allocating rows.
        let raw = cursor.slice(count * 2)?;
        let rows = raw
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        Ok(Self {
            content,
            opening,
            catalog_generation,
            state_revision,
            selected,
            rows,
        })
    }
}

impl Wire for NativeLauncherInput {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.event.put(bytes);
        self.issued_mono_usec.put(bytes);
        (self.kind as u16).put(bytes);
        (self.text.len() as u16).put(bytes);
        bytes.extend_from_slice(self.text.as_bytes());
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let event = NativeLauncherEvent::take(cursor)?;
        let issued_mono_usec = cursor.u64()?;
        let kind = NativeLauncherInputKind::try_from(cursor.u16()?)?;
        let count = usize::from(cursor.u16()?);
        if count > SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES {
            return Err(ValueError::CountTooLarge {
                count,
                max: SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES,
            });
        }
        let text = std::str::from_utf8(cursor.slice(count)?)
            .map_err(|_| ValueError::InvalidRecord("native launcher UTF-8"))?
            .to_owned();
        Ok(Self {
            event,
            issued_mono_usec,
            kind,
            text,
        })
    }
}

impl TryFrom<u16> for NativeLauncherInputKind {
    type Error = ValueError;
    fn try_from(raw: u16) -> Result<Self, Self::Error> {
        use NativeLauncherInputKind::*;
        Ok(match raw {
            1 => Text,
            2 => Left,
            3 => Right,
            4 => Home,
            5 => End,
            6 => Backspace,
            7 => Delete,
            8 => Previous,
            9 => Next,
            10 => PagePrevious,
            11 => PageNext,
            12 => First,
            13 => Last,
            14 => DeleteToStart,
            15 => DeleteToEnd,
            16 => DeleteWord,
            17 => Accept,
            _ => {
                return Err(ValueError::InvalidEnum {
                    field: "native launcher input",
                    value: u32::from(raw),
                });
            }
        })
    }
}

/// The neutral counterpart of the frame codec's `ShellNativeLauncher*`
/// message kinds: names which [`ShellNativeLauncherRecord`] variant a byte
/// body decodes into, without naming any frame message kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellNativeLauncherValueKind {
    Opening,
    AllocationRequest,
    CandidateBegin,
    CandidateChunk,
    Focus,
    FocusRevoked,
    Input,
    InputAck,
    Activate,
    ActivationOutcome,
    Closed,
}

/// The value kind a given record encodes as. The frame codec uses this to
/// pick the message kind a frame carries it under.
pub fn shell_native_launcher_value_kind(
    record: &ShellNativeLauncherRecord,
) -> ShellNativeLauncherValueKind {
    use ShellNativeLauncherValueKind as V;
    match record {
        ShellNativeLauncherRecord::Opening(_) => V::Opening,
        ShellNativeLauncherRecord::AllocationRequest(_) => V::AllocationRequest,
        ShellNativeLauncherRecord::CandidateBegin(_) => V::CandidateBegin,
        ShellNativeLauncherRecord::CandidateChunk(_) => V::CandidateChunk,
        ShellNativeLauncherRecord::Focus(_) => V::Focus,
        ShellNativeLauncherRecord::FocusRevoked(_) => V::FocusRevoked,
        ShellNativeLauncherRecord::Input(_) => V::Input,
        ShellNativeLauncherRecord::InputAck(_) => V::InputAck,
        ShellNativeLauncherRecord::Activate(_) => V::Activate,
        ShellNativeLauncherRecord::ActivationOutcome(_) => V::ActivationOutcome,
        ShellNativeLauncherRecord::Closed(_) => V::Closed,
    }
}

/// Encodes one record's value body. Validates first, exactly as the IPC
/// frame codec did before this split.
pub fn encode_shell_native_launcher_value(
    record: &ShellNativeLauncherRecord,
) -> Result<Vec<u8>, ValueError> {
    crate::shell::native_launcher::validation::validate(record)?;
    let mut bytes = Vec::new();
    match record {
        ShellNativeLauncherRecord::Opening(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::AllocationRequest(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::CandidateBegin(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::CandidateChunk(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::Focus(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::FocusRevoked(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::Input(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::InputAck(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::Activate(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::ActivationOutcome(v) => v.put(&mut bytes),
        ShellNativeLauncherRecord::Closed(v) => v.put(&mut bytes),
    }
    Ok(bytes)
}

/// Encodes the whole native-launcher candidate as one native file layout:
/// the base content-candidate header fields, the launcher's own opening,
/// catalog generation, state revision and selection, four row counts, then
/// the surface/placement/target rows and the displayed catalog-slot rows.
/// No chunk ordinals, no repeated identities. Every part the owner will
/// receive (`CandidateBegin`, `CandidateChunk`, the shared `CandidateEnd`)
/// is validated first, exactly as [`crate::encode_content_candidate`] does
/// for the base profile.
pub fn encode_native_content_candidate(
    value: &NativeContentCandidate,
) -> Result<Vec<u8>, ValueError> {
    let (begin, chunk, end) = value.parts();
    crate::shell::native_launcher::validation::validate(&begin)?;
    crate::shell::native_launcher::validation::validate(&chunk)?;
    crate::shell::content::validation::validate(&end)?;
    let candidate = &value.candidate;
    let mut bytes = Vec::new();
    candidate.grant.put(&mut bytes);
    candidate.candidate_generation.put(&mut bytes);
    candidate.output.put(&mut bytes);
    candidate.facts_generation.put(&mut bytes);
    candidate.pacing_permit.put(&mut bytes);
    candidate.interaction_generation.put(&mut bytes);
    value.opening.put(&mut bytes);
    value.catalog_generation.put(&mut bytes);
    value.state_revision.put(&mut bytes);
    value.selected.put(&mut bytes);
    (candidate.surfaces.len() as u16).put(&mut bytes);
    (candidate.placements.len() as u16).put(&mut bytes);
    (candidate.targets.len() as u16).put(&mut bytes);
    (value.rows.len() as u16).put(&mut bytes);
    0u16.put(&mut bytes);
    for row in &candidate.surfaces {
        row.put(&mut bytes);
    }
    for row in &candidate.placements {
        row.put(&mut bytes);
    }
    for row in &candidate.targets {
        row.put(&mut bytes);
    }
    for slot in &value.rows {
        slot.put(&mut bytes);
    }
    Ok(bytes)
}

pub fn decode_native_content_candidate(bytes: &[u8]) -> Result<NativeContentCandidate, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let grant = ContentGrant::take(&mut cursor)?;
    let candidate_generation = u64::take(&mut cursor)?;
    let output = ContentOutputId::take(&mut cursor)?;
    let facts_generation = u64::take(&mut cursor)?;
    let pacing_permit = u64::take(&mut cursor)?;
    let interaction_generation = u64::take(&mut cursor)?;
    let opening = u64::take(&mut cursor)?;
    let catalog_generation = u64::take(&mut cursor)?;
    let state_revision = u64::take(&mut cursor)?;
    let selected = u16::take(&mut cursor)?;
    let surface_count = table_count(&mut cursor, 1)?;
    let placement_count = table_count(&mut cursor, 32)?;
    let target_count = table_count(&mut cursor, SOPHIA_SHELL_MAX_LAUNCHER_ROWS)?;
    let row_count = table_count(&mut cursor, SOPHIA_SHELL_MAX_LAUNCHER_ROWS)?;
    reserved::<u16>(&mut cursor)?;
    let surfaces = rows(&mut cursor, surface_count)?;
    let placements = rows(&mut cursor, placement_count)?;
    let targets = rows(&mut cursor, target_count)?;
    let launcher_rows = (0..row_count)
        .map(|_| u16::take(&mut cursor))
        .collect::<Result<Vec<_>, _>>()?;
    cursor.finish()?;
    let value = NativeContentCandidate {
        candidate: ContentCandidate {
            grant,
            candidate_generation,
            output,
            facts_generation,
            pacing_permit,
            interaction_generation,
            surfaces,
            placements,
            targets,
        },
        opening,
        catalog_generation,
        state_revision,
        selected,
        rows: launcher_rows,
    };
    let (begin, chunk, end) = value.parts();
    crate::shell::native_launcher::validation::validate(&begin)?;
    crate::shell::native_launcher::validation::validate(&chunk)?;
    crate::shell::content::validation::validate(&end)?;
    Ok(value)
}

/// The file wire's own layout for [`NativeLauncherInput`]: identical event
/// fields, but `text` rides a fixed, zero-padded 256-byte field
/// ([`super::put_text_padded`]) instead of a variable-length tail, so the
/// whole `NativeInput` event is one fixed-size body like every other native
/// launcher event. This is a fresh native layout, not the IPC frame's
/// variable-length encoding (which stays exactly as it is; see
/// [`Wire for NativeLauncherInput`](struct@NativeLauncherInput) above).
pub fn encode_native_launcher_input_padded(
    value: &NativeLauncherInput,
) -> Result<Vec<u8>, ValueError> {
    crate::shell::native_launcher::validation::validate(&ShellNativeLauncherRecord::Input(
        value.clone(),
    ))?;
    let mut bytes = Vec::new();
    value.event.put(&mut bytes);
    value.issued_mono_usec.put(&mut bytes);
    (value.kind as u16).put(&mut bytes);
    super::put_text_padded(
        &mut bytes,
        &value.text,
        SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES,
    );
    Ok(bytes)
}

pub fn decode_native_launcher_input_padded(
    bytes: &[u8],
) -> Result<NativeLauncherInput, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let event = NativeLauncherEvent::take(&mut cursor)?;
    let issued_mono_usec = u64::take(&mut cursor)?;
    let kind = NativeLauncherInputKind::try_from(u16::take(&mut cursor)?)?;
    let text = super::take_text_padded(&mut cursor, SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES)?;
    cursor.finish()?;
    let value = NativeLauncherInput {
        event,
        issued_mono_usec,
        kind,
        text,
    };
    crate::shell::native_launcher::validation::validate(&ShellNativeLauncherRecord::Input(
        value.clone(),
    ))?;
    Ok(value)
}

/// Decodes one record's value body for the given kind. Rejects trailing
/// bytes and then validates, exactly as the IPC frame codec did before this
/// split.
pub fn decode_shell_native_launcher_value(
    kind: ShellNativeLauncherValueKind,
    payload: &[u8],
) -> Result<ShellNativeLauncherRecord, ValueError> {
    use ShellNativeLauncherRecord as R;
    use ShellNativeLauncherValueKind as V;
    let mut cursor = Cursor::new(payload);
    let record = match kind {
        V::Opening => R::Opening(NativeLauncherOpening::take(&mut cursor)?),
        V::AllocationRequest => {
            R::AllocationRequest(NativeLauncherAllocationRequest::take(&mut cursor)?)
        }
        V::CandidateBegin => R::CandidateBegin(NativeLauncherCandidateBegin::take(&mut cursor)?),
        V::CandidateChunk => R::CandidateChunk(ContentCandidateChunk::take(&mut cursor)?),
        V::Focus => R::Focus(NativeLauncherBinding::take(&mut cursor)?),
        V::FocusRevoked => R::FocusRevoked(NativeLauncherFocusRevoked::take(&mut cursor)?),
        V::Input => R::Input(NativeLauncherInput::take(&mut cursor)?),
        V::InputAck => R::InputAck(NativeLauncherInputAck::take(&mut cursor)?),
        V::Activate => R::Activate(NativeLauncherActivation::take(&mut cursor)?),
        V::ActivationOutcome => {
            R::ActivationOutcome(NativeLauncherActivationOutcome::take(&mut cursor)?)
        }
        V::Closed => R::Closed(NativeLauncherClosed::take(&mut cursor)?),
    };
    cursor.finish()?;
    crate::shell::native_launcher::validation::validate(&record)?;
    Ok(record)
}

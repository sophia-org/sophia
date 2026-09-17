use super::SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES;
use super::records::*;
use crate::ipc::cursor::Cursor;
use crate::ipc::shell_content::fields::{Wire, reserved};
use crate::{
    ContentAllocationId, ContentCandidateBegin, ContentGrant, ContentMargins, ContentOutputId,
    IpcCodecError, SOPHIA_SHELL_MAX_LAUNCHER_ROWS,
};

macro_rules! fields {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        impl Wire for $name {
            fn put(&self, bytes: &mut Vec<u8>) { $(self.$field.put(bytes);)* }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
                Ok(Self { $($field: <$ty>::take(cursor)?),* })
            }
        }
    };
}
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
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
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
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let content = ContentCandidateBegin::take(cursor)?;
        let opening = cursor.u64()?;
        let catalog_generation = cursor.u64()?;
        let state_revision = cursor.u64()?;
        let selected = cursor.u16()?;
        let count = usize::from(cursor.u16()?);
        if count > SOPHIA_SHELL_MAX_LAUNCHER_ROWS {
            return Err(IpcCodecError::CountTooLarge {
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
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let event = NativeLauncherEvent::take(cursor)?;
        let issued_mono_usec = cursor.u64()?;
        let kind = NativeLauncherInputKind::try_from(cursor.u16()?)?;
        let count = usize::from(cursor.u16()?);
        if count > SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES {
            return Err(IpcCodecError::CountTooLarge {
                count,
                max: SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES,
            });
        }
        let text = std::str::from_utf8(cursor.slice(count)?)
            .map_err(|_| IpcCodecError::InvalidRecord("native launcher UTF-8"))?
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
    type Error = IpcCodecError;
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
                return Err(IpcCodecError::InvalidEnum {
                    field: "native launcher input",
                    value: u32::from(raw),
                });
            }
        })
    }
}

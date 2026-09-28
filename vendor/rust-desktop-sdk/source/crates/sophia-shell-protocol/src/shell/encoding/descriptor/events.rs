//! Native event and acknowledgement values. Tags are explicit wire values;
//! Rust enum declaration order does not determine them.
use super::*;
use crate::shell::encoding::fields;

macro_rules! word_enum {
    ($name:ident { $($variant:ident = $word:literal),+ $(,)? }) => {
        impl Wire for $name {
            fn put(&self, bytes: &mut Vec<u8>) {
                let word: u16 = match self { $(Self::$variant => $word),+ };
                word.put(bytes);
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
                match u16::take(cursor)? {
                    $($word => Ok(Self::$variant),)+
                    value => Err(ValueError::InvalidEnum {field: stringify!($name), value: u32::from(value)}),
                }
            }
        }
    };
}
word_enum!(ShellV1CandidateOutcomeKind { Prepared=1, Presented=2, Rejected=3, Superseded=4 });
word_enum!(ShellV1ActivationDisposition { Consumed=1, RejectedStale=2 });
word_enum!(ShellReferenceOperation { Startup=0, Toggle=1, Next=2, Previous=3, Dismiss=4 });
word_enum!(ShellLauncherOperation { Open=0, Query=1, Next=2, Previous=3, Dismiss=4 });
word_enum!(ShellLaunchStatus { Started=1, Rejected=2, Failed=3 });

macro_rules! reserved_tail {
    ($name:ident { $($field:ident : $ty:ty),+ $(,)? }) => {
        impl Wire for $name {
            fn put(&self, bytes: &mut Vec<u8>) {
                $(self.$field.put(bytes);)+
                0u16.put(bytes);
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
                let value = Self { $($field: <$ty>::take(cursor)?),+ };
                reserved::<u16>(cursor)?;
                Ok(value)
            }
        }
    };
}
reserved_tail!(ShellV1CandidateOutcome {
    connection_epoch: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    kind: ShellV1CandidateOutcomeKind,
});
reserved_tail!(ShellV1ActivationAck {
    connection_epoch: u64,
    activation: u64,
    disposition: ShellV1ActivationDisposition,
});
reserved_tail!(ShellReferenceRequest {
    connection_epoch: u64,
    catalog_generation: u64,
    request_generation: u64,
    output: OutputId,
    output_generation: u64,
    presentation_epoch: u64,
    operation: ShellReferenceOperation,
});
reserved_tail!(ShellReferenceOutcome {
    connection_epoch: u64,
    catalog_generation: u64,
    request_generation: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    page: u16,
    pages: u16,
    kind: ShellV1CandidateOutcomeKind,
});
reserved_tail!(ShellLauncherOutcome {
    connection_epoch: u64,
    request_generation: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    kind: ShellV1CandidateOutcomeKind,
});
fields!(ShellV1Activation {
    connection_epoch: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    activation: u64,
    action: ToplevelActionCapabilityRef,
});

// The fifty-byte activation identity is shared by three records. Its last
// word is reserved in the event, consumed in the ack, and status in the outcome.
fields!(ShellLauncherActivation {
    connection_epoch: u64,
    catalog_generation: u64,
    request_generation: u64,
    candidate_generation: u64,
    presentation_epoch: u64,
    activation: u64,
    slot: u16,
});
fields!(ShellLaunchOutcome {
    activation: ShellLauncherActivation,
    status: ShellLaunchStatus
});

impl Wire for ShellLauncherActivationAck {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.activation.put(bytes);
        u16::from(self.consumed).put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        Ok(Self {
            activation: ShellLauncherActivation::take(cursor)?,
            consumed: boolean(cursor, "launcher ack consumed")?,
        })
    }
}

impl Wire for ShellLauncherRequest {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.catalog_generation.put(bytes);
        self.request_generation.put(bytes);
        self.output.put(bytes);
        self.output_generation.put(bytes);
        self.presentation_epoch.put(bytes);
        self.operation.put(bytes);
        0u16.put(bytes);
        put_text_padded(bytes, &self.query, SOPHIA_SHELL_MAX_QUERY_BYTES);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let catalog_generation = u64::take(cursor)?;
        let request_generation = u64::take(cursor)?;
        let output = OutputId::take(cursor)?;
        let output_generation = u64::take(cursor)?;
        let presentation_epoch = u64::take(cursor)?;
        let operation = ShellLauncherOperation::take(cursor)?;
        reserved::<u16>(cursor)?;
        let query = take_text_padded(cursor, SOPHIA_SHELL_MAX_QUERY_BYTES)?;
        Ok(Self {
            connection_epoch,
            catalog_generation,
            request_generation,
            output,
            output_generation,
            presentation_epoch,
            operation,
            query,
        })
    }
}

value_codec!(
    encode_shell_descriptor_outcome_value,
    decode_shell_descriptor_outcome_value,
    ShellV1CandidateOutcome,
    |v: &ShellV1CandidateOutcome| validate_shell_descriptor_outcome(*v)
);
value_codec!(
    encode_shell_descriptor_activation_value,
    decode_shell_descriptor_activation_value,
    ShellV1Activation,
    |v: &ShellV1Activation| validate_shell_descriptor_activation(*v)
);
value_codec!(
    encode_shell_descriptor_ack_value,
    decode_shell_descriptor_ack_value,
    ShellV1ActivationAck,
    |v: &ShellV1ActivationAck| validate_shell_descriptor_activation_ack(*v)
);
value_codec!(
    encode_shell_reference_request_value,
    decode_shell_reference_request_value,
    ShellReferenceRequest,
    |v: &ShellReferenceRequest| validate_shell_reference_request(*v)
);
value_codec!(
    encode_shell_reference_outcome_value,
    decode_shell_reference_outcome_value,
    ShellReferenceOutcome,
    |v: &ShellReferenceOutcome| validate_shell_reference_outcome(*v)
);
value_codec!(
    encode_shell_launcher_request_value,
    decode_shell_launcher_request_value,
    ShellLauncherRequest,
    validate_shell_launcher_request
);
value_codec!(
    encode_shell_launcher_outcome_value,
    decode_shell_launcher_outcome_value,
    ShellLauncherOutcome,
    |v: &ShellLauncherOutcome| validate_shell_launcher_outcome(*v)
);
value_codec!(
    encode_shell_launcher_ack_value,
    decode_shell_launcher_ack_value,
    ShellLauncherActivationAck,
    |v: &ShellLauncherActivationAck| validate_shell_launcher_activation_ack(*v)
);
value_codec!(
    encode_shell_launch_outcome_value,
    decode_shell_launch_outcome_value,
    ShellLaunchOutcome,
    |v: &ShellLaunchOutcome| validate_shell_launch_outcome(*v)
);

pub fn encode_shell_launcher_activation_value(
    value: &ShellLauncherActivation,
) -> Result<Vec<u8>, ValueError> {
    validate_shell_launcher_activation(*value)?;
    let mut bytes = Vec::with_capacity(52);
    value.put(&mut bytes);
    0u16.put(&mut bytes);
    Ok(bytes)
}
pub fn decode_shell_launcher_activation_value(
    bytes: &[u8],
) -> Result<ShellLauncherActivation, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let value = ShellLauncherActivation::take(&mut cursor)?;
    reserved::<u16>(&mut cursor)?;
    cursor.finish()?;
    validate_shell_launcher_activation(value)?;
    Ok(value)
}

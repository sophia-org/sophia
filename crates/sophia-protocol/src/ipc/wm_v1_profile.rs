use crate::{
    IpcCodecError, TransactionId, WmV1ProfileActivate, WmV1ProfileActive, WmV1ProfilePrepare,
    WmV1ProfilePrepared, WmV1ProfileRollback, WmV1ProfileRolledBack,
    decode_wm_v1_profile_activate_frame, decode_wm_v1_profile_active_frame,
    decode_wm_v1_profile_prepare_frame, decode_wm_v1_profile_prepared_frame,
    decode_wm_v1_profile_rollback_frame, decode_wm_v1_profile_rolled_back_frame,
    encode_wm_v1_profile_activate_frame, encode_wm_v1_profile_active_frame,
    encode_wm_v1_profile_prepare_frame, encode_wm_v1_profile_prepared_frame,
    encode_wm_v1_profile_rollback_frame, encode_wm_v1_profile_rolled_back_frame,
};

// Compatibility names retain the same passive types, constructors and errors.
pub use crate::{
    POLICY_PROFILE_DIGEST_BYTES as WM_V1_PROFILE_DIGEST_BYTES,
    PolicyProfileCommand as WmV1ProfileCommand, PolicyProfileCompletion as WmV1ProfileCompletion,
    PolicyProfileIdentity as WmV1ProfileIdentity, PolicyProfileOutcome as WmV1ProfileOutcome,
};

fn command(
    transaction: TransactionId,
    connection_epoch: u64,
    profile_generation: u64,
    profile_digest: [u8; WM_V1_PROFILE_DIGEST_BYTES],
) -> Result<WmV1ProfileCommand, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(transaction.raw()));
    }
    Ok(WmV1ProfileCommand {
        transaction,
        identity: WmV1ProfileIdentity::new(connection_epoch, profile_generation, profile_digest)?,
    })
}

fn completion(
    transaction: TransactionId,
    connection_epoch: u64,
    profile_generation: u64,
    profile_digest: [u8; WM_V1_PROFILE_DIGEST_BYTES],
    outcome: u16,
) -> Result<WmV1ProfileCompletion, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(transaction.raw()));
    }
    Ok(WmV1ProfileCompletion {
        transaction,
        identity: WmV1ProfileIdentity::new(connection_epoch, profile_generation, profile_digest)?,
        outcome: outcome.try_into()?,
    })
}

macro_rules! profile_command_codec {
    ($decode:ident, $encode:ident, $decode_frame:ident, $encode_frame:ident, $wire:ident) => {
        pub fn $decode(frame: &[u8]) -> Result<WmV1ProfileCommand, IpcCodecError> {
            let (transaction, wire) = $decode_frame(frame)?;
            command(
                transaction,
                wire.connection_epoch,
                wire.profile_generation,
                wire.profile_digest,
            )
        }

        pub fn $encode(command: WmV1ProfileCommand) -> Result<Vec<u8>, IpcCodecError> {
            $encode_frame(
                command.transaction,
                &$wire {
                    connection_epoch: command.identity.connection_epoch,
                    profile_generation: command.identity.profile_generation,
                    profile_digest: command.identity.profile_digest,
                },
            )
        }
    };
}

macro_rules! profile_completion_codec {
    ($decode:ident, $encode:ident, $decode_frame:ident, $encode_frame:ident, $wire:ident) => {
        pub fn $decode(frame: &[u8]) -> Result<WmV1ProfileCompletion, IpcCodecError> {
            let (transaction, wire) = $decode_frame(frame)?;
            completion(
                transaction,
                wire.connection_epoch,
                wire.profile_generation,
                wire.profile_digest,
                wire.outcome,
            )
        }

        pub fn $encode(completion: WmV1ProfileCompletion) -> Result<Vec<u8>, IpcCodecError> {
            $encode_frame(
                completion.transaction,
                &$wire {
                    connection_epoch: completion.identity.connection_epoch,
                    profile_generation: completion.identity.profile_generation,
                    profile_digest: completion.identity.profile_digest,
                    outcome: completion.outcome as u16,
                },
            )
        }
    };
}

profile_command_codec!(
    decode_wm_v1_profile_prepare,
    encode_wm_v1_profile_prepare,
    decode_wm_v1_profile_prepare_frame,
    encode_wm_v1_profile_prepare_frame,
    WmV1ProfilePrepare
);
profile_completion_codec!(
    decode_wm_v1_profile_prepared,
    encode_wm_v1_profile_prepared,
    decode_wm_v1_profile_prepared_frame,
    encode_wm_v1_profile_prepared_frame,
    WmV1ProfilePrepared
);
profile_command_codec!(
    decode_wm_v1_profile_activate,
    encode_wm_v1_profile_activate,
    decode_wm_v1_profile_activate_frame,
    encode_wm_v1_profile_activate_frame,
    WmV1ProfileActivate
);
profile_completion_codec!(
    decode_wm_v1_profile_active,
    encode_wm_v1_profile_active,
    decode_wm_v1_profile_active_frame,
    encode_wm_v1_profile_active_frame,
    WmV1ProfileActive
);
profile_command_codec!(
    decode_wm_v1_profile_rollback,
    encode_wm_v1_profile_rollback,
    decode_wm_v1_profile_rollback_frame,
    encode_wm_v1_profile_rollback_frame,
    WmV1ProfileRollback
);
profile_completion_codec!(
    decode_wm_v1_profile_rolled_back,
    encode_wm_v1_profile_rolled_back,
    decode_wm_v1_profile_rolled_back_frame,
    encode_wm_v1_profile_rolled_back_frame,
    WmV1ProfileRolledBack
);

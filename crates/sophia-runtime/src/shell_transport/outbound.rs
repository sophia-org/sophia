//! Session-to-client obligations as typed values. The shared FIFO holds these,
//! never wire bytes: each wire encodes a record only when it takes custody.
//!
//! A record's queue charge is its body in the surviving native encoding
//! (`sophia-shell-files-v1.kdl`), without any wire header. Both wires and the
//! content registry charge that one unit, so no owner sizes a record in a
//! particular wire's framing.
use sophia_protocol::shell_files::{
    ShellFileCatalogActionRecord, ShellFileIndicatorActivationOutcome, ShellFileKind,
    ShellFileNativeLauncherRecord, ShellFileTransactionRecord,
    encode_shell_file_allocation_result_body, encode_shell_file_catalog_action_body,
    encode_shell_file_indicator_activation_outcome_body, encode_shell_file_native_input_body,
    encode_shell_file_native_launcher_transaction_body, encode_shell_file_outputs_body,
    encode_shell_file_resource_released_body, encode_shell_file_resource_status_body,
    encode_shell_file_transaction_body, shell_file_transaction_kind,
};
use sophia_protocol::{
    ShellCatalogActionRecord, ShellContentRecord, ShellIndicatorActivationOutcome,
    ShellNativeLauncherRecord, TransactionId,
};

use super::ShellTransportError;

/// Body prefix and per-output row of the `Outputs` object
/// (`body-prefix "Outputs" size=40`, one 40-byte row per output).
const OUTPUT_FACTS_PREFIX_BYTES: usize = 40;
const OUTPUT_FACT_BYTES: usize = 40;

/// One Session-to-client record owed to the component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum OutboundRecord {
    Content(TransactionId, ShellContentRecord),
    NativeLauncher(TransactionId, ShellNativeLauncherRecord),
    CatalogAction(TransactionId, ShellCatalogActionRecord),
    IndicatorOutcome(TransactionId, ShellIndicatorActivationOutcome),
}

/// A record whose encoding and wire bounds were checked before any owner
/// changed state. Only `ShellComponentTransport::admit_record` creates one, and
/// moving it into the FIFO is the one infallible custody transfer.
#[derive(Debug)]
pub(super) struct Admitted {
    pub(super) record: OutboundRecord,
    pub(super) control: bool,
    pub(super) charge: usize,
}

impl OutboundRecord {
    /// The surviving native encoding: the file event or object kind, and its
    /// body. A record the file contract does not carry fails closed here; it
    /// never falls back to another wire's framing.
    pub(super) fn native(&self) -> Result<(ShellFileKind, Vec<u8>), ShellTransportError> {
        let refused = |_| ShellTransportError::WrongContentRecord;
        match self {
            Self::Content(transaction, record) => content_body(*transaction, record),
            Self::NativeLauncher(transaction, record) => {
                let value = ShellFileNativeLauncherRecord {
                    transaction: *transaction,
                    record: record.clone(),
                };
                if matches!(record, ShellNativeLauncherRecord::Input(_)) {
                    let body = encode_shell_file_native_input_body(&value).map_err(refused)?;
                    Ok((ShellFileKind::NativeInput, body))
                } else {
                    encode_shell_file_native_launcher_transaction_body(&value).map_err(refused)
                }
            }
            Self::CatalogAction(transaction, record) => {
                encode_shell_file_catalog_action_body(&ShellFileCatalogActionRecord {
                    transaction: *transaction,
                    record: record.clone(),
                })
                .map_err(refused)
            }
            Self::IndicatorOutcome(transaction, outcome) => {
                let body = encode_shell_file_indicator_activation_outcome_body(
                    &ShellFileIndicatorActivationOutcome {
                        transaction: *transaction,
                        outcome: *outcome,
                    },
                )
                .map_err(refused)?;
                Ok((ShellFileKind::IndicatorActivationOutcome, body))
            }
        }
    }
}

/// The queue charge of an `OutputFacts` record with `outputs` rows, which the
/// content registry holds until the record moves into the FIFO. It equals the
/// native body length `OutboundRecord::native` produces.
pub(crate) const fn output_facts_charge(outputs: usize) -> usize {
    OUTPUT_FACTS_PREFIX_BYTES + OUTPUT_FACT_BYTES * outputs
}

fn content_body(
    transaction: TransactionId,
    record: &ShellContentRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellTransportError> {
    let refused = |_| ShellTransportError::WrongContentRecord;
    let value = ShellFileTransactionRecord {
        transaction,
        record: record.clone(),
    };
    match record {
        ShellContentRecord::ResourceStatus(_) => Ok((
            ShellFileKind::ResourceStatus,
            encode_shell_file_resource_status_body(&value).map_err(refused)?,
        )),
        ShellContentRecord::ResourceReleased(_) => Ok((
            ShellFileKind::ResourceReleased,
            encode_shell_file_resource_released_body(&value).map_err(refused)?,
        )),
        ShellContentRecord::OutputFacts(_) => Ok((
            ShellFileKind::Outputs,
            encode_shell_file_outputs_body(&value).map_err(refused)?,
        )),
        ShellContentRecord::AllocationResult(_) => Ok((
            ShellFileKind::AllocationResult,
            encode_shell_file_allocation_result_body(&value).map_err(refused)?,
        )),
        // Candidate outcomes, frame permits and actions: the caller admits
        // only server records.
        _ if shell_file_transaction_kind(record).is_some() => {
            encode_shell_file_transaction_body(&value).map_err(refused)
        }
        _ => Err(ShellTransportError::WrongContentRecord),
    }
}

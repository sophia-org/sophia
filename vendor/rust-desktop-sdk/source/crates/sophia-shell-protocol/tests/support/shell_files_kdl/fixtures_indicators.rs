//! Distinctly-valued typed fixtures for the t252 B5 view indicator (r6)
//! kinds: the whole `Indicators` object, the indicator activation-outcome
//! event and the indicator activate candidate.

use super::fmap;
use sophia_shell_protocol::*;
use std::collections::BTreeMap;

/// The whole indicator snapshot: one output status, two indicators, an
/// active output. Its transaction, the prefix's field-value table and each
/// row table's field-value tables in row order (labels excluded: `type="text"`
/// fields are checked separately via [`indicator_row_text`] and
/// [`super::checks::verify_text_field`]).
#[allow(clippy::type_complexity)]
pub fn indicators() -> (
    u64,
    ShellIndicatorSnapshot,
    BTreeMap<&'static str, i128>,
    Vec<BTreeMap<&'static str, i128>>,
    Vec<BTreeMap<&'static str, i128>>,
) {
    let transaction = 5001u64;
    let connection_epoch = 201u64;
    let generation = 202u64;
    let active_output = OutputId::from_raw(9);
    let active_output_id = active_output.raw() as i128;
    let active_output_present = 1u64;
    let status_count = 1u64;
    let indicator_count = 2u64;

    let status_output = OutputId::from_raw(9);
    let focus_bits = 3u16;
    let status = ShellOutputStatus {
        output: status_output,
        focus_bits,
        layout: "grid".to_owned(),
    };
    let status_expected = {
        let output_id = status_output.raw();
        fmap!(output_id, focus_bits)
    };

    let indicator0 = ShellIndicator {
        output: OutputId::from_raw(9),
        indicator: 11,
        action: 12,
        slot: 0,
        state_bits: 1,
        label: "A".to_owned(),
    };
    let indicator0_expected = {
        let output_id = indicator0.output.raw();
        let indicator = indicator0.indicator;
        let action = indicator0.action;
        let slot = indicator0.slot;
        let state_bits = indicator0.state_bits;
        fmap!(output_id, indicator, action, slot, state_bits)
    };
    let indicator1 = ShellIndicator {
        output: OutputId::from_raw(10),
        indicator: 21,
        action: 22,
        slot: 1,
        state_bits: 2,
        label: "overview".to_owned(),
    };
    let indicator1_expected = {
        let output_id = indicator1.output.raw();
        let indicator = indicator1.indicator;
        let action = indicator1.action;
        let slot = indicator1.slot;
        let state_bits = indicator1.state_bits;
        fmap!(output_id, indicator, action, slot, state_bits)
    };

    let snapshot = ShellIndicatorSnapshot {
        connection_epoch,
        generation,
        active_output: Some(active_output),
        statuses: vec![status],
        indicators: vec![indicator0, indicator1],
    };
    let expected_prefix = fmap!(
        transaction,
        connection_epoch,
        generation,
        active_output_id,
        active_output_present,
        status_count,
        indicator_count
    );
    (
        transaction,
        snapshot,
        expected_prefix,
        vec![status_expected],
        vec![indicator0_expected, indicator1_expected],
    )
}

/// The layout/label text `indicators` carries per row, in row order.
pub fn indicators_status_text() -> Vec<&'static str> {
    vec!["grid"]
}
pub fn indicators_entry_text() -> Vec<&'static str> {
    vec!["A", "overview"]
}

pub fn indicator_activation_outcome() -> (
    u64,
    ShellIndicatorActivationOutcome,
    BTreeMap<&'static str, i128>,
) {
    let transaction = 5002u64;
    let connection_epoch = 211u64;
    let snapshot_generation = 212u64;
    let event_id = 213u64;
    let value = ShellIndicatorActivationOutcome {
        connection_epoch,
        snapshot_generation,
        event_id,
        status: ShellIndicatorActivationStatus::Stale,
        reason: 4,
    };
    let status = value.status as i128;
    let reason = value.reason;
    let expected = fmap!(
        transaction,
        connection_epoch,
        snapshot_generation,
        event_id,
        status,
        reason
    );
    (transaction, value, expected)
}

pub fn indicator_activate() -> (u64, ShellIndicatorActivation, BTreeMap<&'static str, i128>) {
    let transaction = 5003u64;
    let connection_epoch = 221u64;
    let snapshot_generation = 222u64;
    let output = OutputId::from_raw(31);
    let indicator = 32u64;
    let action = 33u64;
    let event_id = 34u64;
    let value = ShellIndicatorActivation {
        connection_epoch,
        snapshot_generation,
        output,
        indicator,
        action,
        event_id,
    };
    let output_id = output.raw();
    let expected = fmap!(
        transaction,
        connection_epoch,
        snapshot_generation,
        output_id,
        indicator,
        action,
        event_id
    );
    (transaction, value, expected)
}

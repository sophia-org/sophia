//! Durable capture of output resolution (t310): each admitted record keeps
//! exactly its bounded fields, and everything else is dropped whole or field
//! by field, never copied.

use sophia_config::DesktopOutputAdjustmentReason as Reason;
use sophia_session::diagnostics::reduced_record;

const ALL_REASONS: &[Reason] = &[
    Reason::Mode,
    Reason::Scale,
    Reason::Transform,
    Reason::Vrr,
    Reason::Position,
    Reason::Unavailable,
    Reason::MirrorUnavailable,
    Reason::Fallback,
];

fn kept(record: &str) {
    assert_eq!(reduced_record(record).as_deref(), Some(record), "{record}");
}

#[test]
fn every_resolution_status_and_reason_keeps_its_fields() {
    for status in [
        "waiting",
        "resolved",
        "committed",
        "refused",
        "construction_refused",
        "uncommitted",
    ] {
        for phase in ["startup", "runtime"] {
            for reason in [
                "presented_settings_differ",
                "profile_changed",
                "hardware",
                "unavailable",
                "stale",
                "none",
            ] {
                kept(&format!(
                    "sophia_live_output_resolution schema=1 phase={phase} status={status} reason={reason} generation=3 transition=2 notice=5 owner=4 outputs=2 adjustments=1 attempt=1"
                ));
            }
        }
    }
}

#[test]
fn every_adjustment_reason_is_admitted_by_its_debug_name() {
    for reason in ALL_REASONS {
        kept(&format!(
            "sophia_live_output_adjustment schema=1 phase=runtime reason={reason:?} head=3 output=1"
        ));
        kept(&format!(
            "sophia_live_output_adjustment schema=1 phase=startup reason={reason:?}"
        ));
    }
}

#[test]
fn free_form_and_malformed_values_never_cross() {
    for (record, reduced) in [
        (
            "sophia_live_output_resolution schema=1 phase=startup status=refused attempt=2 error=private connector=DP-1 path=/home/x profile=desktop reason=hardware",
            "sophia_live_output_resolution schema=1 phase=startup status=refused attempt=2 reason=hardware",
        ),
        (
            "sophia_live_output_resolution schema=1 phase=runtime status=waiting generation=-1 transition=0x2 notice=1.5 owner=18446744073709551616 outputs=2",
            "sophia_live_output_resolution schema=1 phase=runtime status=waiting outputs=2",
        ),
        (
            "sophia_live_output_resolution schema=1 phase=runtime status=committed reason=Hardware",
            "sophia_live_output_resolution schema=1 phase=runtime status=committed",
        ),
        (
            "sophia_live_output_resolution schema=2 phase=runtime status=committed owner=4",
            "sophia_live_output_resolution phase=runtime status=committed owner=4",
        ),
        (
            "sophia_live_output_adjustment schema=1 phase=runtime reason=Mode connector=HDMI-A-1 head=x output=2 status=waiting",
            "sophia_live_output_adjustment schema=1 phase=runtime reason=Mode output=2",
        ),
        // The first occurrence of a key decides; a later one cannot replace it.
        (
            "sophia_live_output_resolution schema=1 phase=runtime status=resolved owner=4 status=refused owner=9 phase=startup",
            "sophia_live_output_resolution schema=1 phase=runtime status=resolved owner=4",
        ),
    ] {
        assert_eq!(reduced_record(record).as_deref(), Some(reduced), "{record}");
    }
}

#[test]
fn chatter_typos_and_missing_identities_are_refused_whole() {
    for record in [
        "sophia_live_output_resolution schema=1 phase=startup status=probing attempt=1",
        "sophia_live_output_resolution schema=1 phase=startup status=Waiting attempt=1",
        "sophia_live_output_resolution schema=1 phase=startup status=wait attempt=1",
        "sophia_live_output_resolution schema=1 phase=startup attempt=1",
        "sophia_live_output_resolution schema=1 status=waiting attempt=1",
        "sophia_live_output_resolution schema=1 phase=shutdown status=waiting",
        "sophia_live_output_resolution schema=1 phase=startup status=",
        "sophia_live_output_adjustment schema=1 phase=runtime reason=mode head=1",
        "sophia_live_output_adjustment schema=1 phase=runtime reason=Some(Mode)",
        "sophia_live_output_adjustment schema=1 phase=runtime head=1 output=1",
        "sophia_live_output_adjustment schema=1 reason=Mode head=1",
    ] {
        assert_eq!(reduced_record(record), None, "{record}");
    }
}

#[test]
fn only_the_first_sixteen_fields_are_read() {
    let mut record =
        String::from("sophia_live_output_resolution schema=1 phase=runtime status=waiting");
    for index in 0..13 {
        record.push_str(&format!(" filler{index}=1"));
    }
    record.push_str(" generation=3 transition=2 attempt=1");
    assert_eq!(
        reduced_record(&record).as_deref(),
        Some("sophia_live_output_resolution schema=1 phase=runtime status=waiting")
    );
}

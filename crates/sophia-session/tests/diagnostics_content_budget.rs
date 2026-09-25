use sophia_session::diagnostics::reduced_record;

#[test]
fn component_budget_evidence_keeps_exact_bounded_host_numbers() {
    for status in ["admission_refused", "admitted_reduced"] {
        let record = format!(
            "sophia_shell_component schema=1 status={status} cause=content_budget slot=2 role=dock connection_epoch=17 content_grant_epoch=18 source_capacity_bytes=67108864 backing_capacity_bytes=67108864 nominal_bytes=20971520 own_retired_bytes=4 own_retired_epochs=1 reserved_bytes=67108860 reserved_backing_bytes=50000000 available_bytes=4 available_backing_bytes=12000000 required_bytes=16777216 required_backing_bytes=12582912 active_epochs=2 retired_epochs=1 staging_bytes=0 resident_bytes=0 retiring_bytes=0"
        );
        assert_eq!(reduced_record(&record), Some(record.clone()));
        assert_eq!(
            reduced_record(&format!(
                "{record} title=private path=/private reason=unbounded"
            )),
            Some(record)
        );
    }
}

#[test]
fn budget_evidence_drops_out_of_range_or_unapproved_values() {
    let record = reduced_record("sophia_shell_component schema=1 status=private cause=arbitrary slot=3 source_capacity_bytes=67108865 own_retired_bytes=-1 own_retired_epochs=17 reserved_bytes=18446744073709551616 available_bytes=NaN required_bytes=0 retired_epochs=16").unwrap();
    assert_eq!(
        record,
        "sophia_shell_component schema=1 required_bytes=0 retired_epochs=16"
    );
}

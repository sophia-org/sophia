use sophia_session::diagnostics::reduced_record;

#[test]
fn native_stall_keeps_completion_facts_without_arbitrary_payloads() {
    let facts = "sophia_native_stall schema=1 head=2 output=1 age_ms=501 retirements=551 ledger_pending=false completion_fence=Signaled completion_mode=OutFenceAuthoritative out_fence_retirements=3 late_events=4 poller_pending=0 decoded=555 rejected=0";
    assert_eq!(reduced_record(facts).as_deref(), Some(facts));
    assert_eq!(
        reduced_record(&format!(
            "{facts} path=/secret head=9 completion_fence=private arbitrary=17"
        ))
        .as_deref(),
        Some(facts)
    );
    assert!(reduced_record("sophia_native_stall schema=2 head=2").is_none());
    assert_eq!(reduced_record("sophia_native_stall schema=1 decoded=18446744073709551616 ledger_pending=maybe completion_fence=private").as_deref(), Some("sophia_native_stall schema=1"));
}

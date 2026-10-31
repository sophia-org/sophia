use sophia_session::diagnostics::reduced_record;

#[test]
fn stage_cost_records_keep_every_field_without_accepting_payloads() {
    let common = "sophia_shell_work_cost schema=1 role=owner tid=7 thread_sequence=1 snapshot=3 final=0 observed_monotonic_usec=19 cpu_first_nsec=1 cpu_now_nsec=12 clock_reads=9 monotonic_reads=3 clock_regressions=0 saturations=0 valid=1";
    for suffix in [
        "kind=record_class class=resource_retire direction=incoming decoded=2 admitted=1 service_records=2 service_turns=1 flush_records=0 flush_turns=0",
        "kind=stage stage=copy parent=validate inclusive=1 calls=2 errors=1 empty=0 aborted=0 cpu_nsec=8 max_nsec=5 bytes=16 ok_bytes=8",
        "kind=boundary boundary=before_turn export_polls=2 file_checks=0 custody_checks=2 image_process_checks=0",
        "kind=summary service_turns=1 service_empty=0 service_records=2 flush_turns=1 flush_empty=1 flush_records=0 staged_attempts=1 staged_errors=0 ring_attempts=1 ring_errors=1 ack_attempts=1 ack_errors=0 staged_writes=1 staged_refused=0 ring_writes=1 ring_refused=1 ack_writes=1 ack_refused=0 jobs=0 job_errors=0 job_bytes=0 server_frame_permits=1",
    ] {
        let row = format!("{common} {suffix}");
        assert_eq!(reduced_record(&row), Some(row.clone()));
        assert_eq!(reduced_record(&format!("{row} payload=secret")), None);
        assert_eq!(reduced_record(&format!("{row} tid=8")), None);
        // An invalid measurement must remain visible to the downstream gate.
        let invalid = row
            .replace("clock_regressions=0", "clock_regressions=1")
            .replace("valid=1", "valid=0");
        assert_eq!(reduced_record(&invalid), Some(invalid));
    }
}

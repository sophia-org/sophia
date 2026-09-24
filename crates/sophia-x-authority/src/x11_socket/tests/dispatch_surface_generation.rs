// The dispatcher's surface-generation tests, moved out of the production
// file (t026); `super` here is the socket tests module, which globs the
// x11_socket namespace.

mod surface_generation_tests {
    use super::*;

    #[test]
    fn rejected_candidate_is_reusable_but_admitted_xid_recreation_advances() {
        let mut ledger = X11SurfaceGenerationLedger::default();

        let first = ledger.candidate(0x220001).unwrap();
        assert_eq!(first, SurfaceId::new(0x220001, 1));
        assert_eq!(ledger.candidate(0x220001).unwrap(), first);

        ledger.admit(first).unwrap();
        let replacement = ledger.candidate(0x220001).unwrap();
        assert_eq!(replacement, SurfaceId::new(0x220001, 2));
        assert_ne!(replacement, first);
        ledger.admit(replacement).unwrap();
        assert_eq!(
            ledger.candidate(0x220001).unwrap(),
            SurfaceId::new(0x220001, 3)
        );
    }

    #[test]
    fn ledger_rejects_stale_or_skipped_admission() {
        let mut ledger = X11SurfaceGenerationLedger::default();

        assert!(ledger.admit(SurfaceId::new(7, 2)).is_err());
        ledger.admit(SurfaceId::new(7, 1)).unwrap();
        assert!(ledger.admit(SurfaceId::new(7, 1)).is_err());
        assert!(ledger.admit(SurfaceId::new(7, 3)).is_err());
    }

    #[test]
    fn generation_exhaustion_fails_closed() {
        let mut ledger = X11SurfaceGenerationLedger::default();
        ledger.admitted.insert(9, u32::MAX);

        assert!(ledger.candidate(9).is_err());
    }
}

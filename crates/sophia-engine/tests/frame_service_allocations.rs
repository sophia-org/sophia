use sophia_engine::{
    OutputFrameServiceObservation, OutputFrameServiceReducer, OutputFrameServiceRequest,
    OutputNativeFramePhase,
};
use sophia_protocol::OutputId;

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn stable_service_and_reordered_observations_allocate_nothing() {
    for count in [1, 2, 16] {
        let mut request = OutputFrameServiceRequest {
            outputs: (1..=count)
                .map(|id| OutputFrameServiceObservation {
                    output: OutputId::from_raw(id),
                    primary: id == 1,
                    native_phase: OutputNativeFramePhase::InFlight,
                    pending_frame: true,
                })
                .collect(),
            ..Default::default()
        };
        let before = ALLOCATOR.stats();
        for _ in 0..1000 {
            let mut reducer = OutputFrameServiceReducer::begin(&request).unwrap();
            request.outputs.reverse();
            for _ in 0..count {
                assert!(reducer.next_effect(&request).unwrap().is_some());
            }
            assert!(reducer.next_effect(&request).unwrap().is_none());
            for output in &mut request.outputs {
                output.native_phase = OutputNativeFramePhase::Idle;
            }
            for _ in 0..count {
                assert!(reducer.next_effect(&request).unwrap().is_some());
            }
            assert!(reducer.next_effect(&request).unwrap().is_none());
            for output in &mut request.outputs {
                output.native_phase = OutputNativeFramePhase::InFlight;
            }
            std::hint::black_box(reducer);
        }
        let measured = ALLOCATOR.stats() - before;
        eprintln!(
            "{count} outputs, 1000 service turns: {} allocations, {} reallocations",
            measured.allocations, measured.reallocations
        );
        assert_eq!(measured.allocations + measured.reallocations, 0);
    }
}

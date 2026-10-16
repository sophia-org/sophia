mod support;
use sophia_9p::{Limits, unix::Server};

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn repeated_borrowed_readiness_does_not_allocate() {
    let (_peer, stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut server = Server::new(support::StaticExport::new(), Limits::default()).unwrap();
    server.adopt(stream).map_err(|e| e.error).unwrap();
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        let mut count = 0;
        server.visit_poll_fds(|fd| {
            std::hint::black_box(fd);
            count += 1;
        });
        assert_eq!(count, 2);
    }
    let after = ALLOCATOR.stats();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.reallocations, before.reallocations);
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        std::hint::black_box(server.poll_fds());
    }
    let after = ALLOCATOR.stats();
    assert_eq!(after.allocations - before.allocations, 1000);
    eprintln!("readiness: 1000 owned lists = 1000 allocations; 1000 borrowed visits = 0");
}

//! Generic conformance peer. The owner supplies stdin or an explicit socket path;
//! neither mode supplies authentication or chooses a fallback after failure.
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_broker_client::{
    Error as StateError,
    protocol::{RequestKind, ResponseRow, RowKind},
    session::{Error, Session},
    startup::Opening,
};
use std::{
    os::fd::AsFd,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};
fn main() {
    let stream = match std::env::var_os("SOPHIA_BROKER_9P_SOCKET") {
        Some(path) => UnixStream::connect(path).unwrap(),
        None => UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap()),
    };
    let mut wire =
        Pipeline::over(stream, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let (tag, root) = wire.attach(b"", b"").unwrap();
    assert!(matches!(
        wire.wait(tag, Instant::now() + Duration::from_secs(2))
            .unwrap(),
        Reply::Attach(_)
    ));
    let start = Instant::now();
    let mut opening = Opening::from_attached(wire, root, 0, 8000).unwrap();
    while !opening.ready() {
        assert!(start.elapsed() < Duration::from_secs(8));
        opening
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::yield_now();
    }
    let mut s = Session::new(opening.into_opened().unwrap(), 2, 2, 8000).unwrap();
    let mut sent = false;
    loop {
        assert!(start.elapsed() < Duration::from_secs(8));
        let now = start.elapsed().as_millis() as u64;
        s.dispatch(now, 65536).unwrap();
        while s.event().is_some() {
            s.consume(now).unwrap();
        }
        match s.acknowledge(now) {
            Ok(()) | Err(Error::State(StateError::Busy)) => (),
            Err(e) => panic!("{e:?}"),
        }
        if !sent
            && s.transaction_ready()
            && let Some(request) = s.request()
        {
            let mut rows = vec![ResponseRow {
                kind: RowKind::RetireSurface,
                surface_index: request.surface_index,
                surface_generation: request.surface_generation,
                ..Default::default()
            }];
            if request.kind == RequestKind::SetDisclosure {
                rows[0] = ResponseRow {
                    kind: RowKind::PublishRule,
                    surface_index: request.surface_index,
                    surface_generation: request.surface_generation,
                    disclosure: request.value,
                    trust: 1,
                    generation: 8,
                    ..Default::default()
                };
                rows.push(ResponseRow {
                    kind: RowKind::EmitDescriptor,
                    surface_index: request.surface_index,
                    surface_generation: request.surface_generation,
                    trust: 1,
                    attention: 2,
                    generation: 8,
                    grant_token: 12,
                    grant_revocation_epoch: 1,
                    grant_target_generation: 8,
                    ..Default::default()
                });
            } else {
                assert_eq!(request.kind, RequestKind::SurfaceRemoved);
            }
            s.respond(request.id, &rows, now).unwrap();
            sent = true;
        }
        if sent && s.transaction_ready() {
            break;
        }
        std::thread::yield_now();
    }
    s.close();
}

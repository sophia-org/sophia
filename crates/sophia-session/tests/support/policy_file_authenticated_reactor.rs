//! Construction refusal is not permission to fall back to raw WM attach.
#![cfg(test)]
use super::*;
use sophia_runtime::authenticated_files::FileAuthentication;
use std::io::Read;
use std::sync::atomic::AtomicUsize;

struct Refuse(Arc<AtomicUsize>);
impl FileAuthenticationFactory for Refuse {
    fn create(
        &mut self,
        _connection: sophia_9p::ConnectionId,
        _now: Instant,
        _wake: Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Errno::EACCES)
    }
}

fn refuse(stopped: bool) {
    let (mut peer, stream) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let owner = WmFiles::awaiting_negotiation(
        1,
        WmFileLimits {
            capability_ceiling: 0,
            profile_required: false,
        },
        WmQids::new(),
        typed_codec::TypedFileCodec,
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cancellation = NinePCancellation::new();
    if stopped {
        cancellation.handle().stop();
    }
    let result = NinePReactor::adopt_with_authentication(
        stream,
        owner,
        cancellation,
        Some(Box::new(Refuse(calls.clone()))),
    );
    let error = match result {
        Ok(_) => panic!("refused WM authentication must not produce a raw reactor"),
        Err(error) => error,
    };
    assert_eq!(calls.load(Ordering::SeqCst), usize::from(!stopped));
    assert!(error.contains(if stopped {
        "stopped before adoption"
    } else {
        "authentication admission"
    }));
    assert_eq!(
        peer.read(&mut [0; 1]).unwrap(),
        0,
        "failed authenticated construction closes the role stream"
    );
}

#[test]
fn wm_authentication_factory_refusal_closes_without_fallback() {
    refuse(false);
}

#[test]
fn cancelled_wm_adoption_never_reserves_authentication() {
    refuse(true);
}

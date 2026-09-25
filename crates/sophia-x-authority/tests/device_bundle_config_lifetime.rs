#![cfg(unix)]
//! t069 D1: the device bundle a frontend is configured with is its first
//! generation, installed once. The frontend's retained configuration must not
//! keep that generation alive after it is superseded and no client or backing
//! holds it, or a lost startup device stays open for the session and occupies
//! one of the sixteen generation slots. Real frontend and configuration
//! construction; the provider's lifetime is witnessed through a weak pointer.

use sophia_protocol::{DRM_FORMAT_XRGB8888, NamespaceId};
use sophia_x_authority::*;
use std::{
    fs::File,
    io::{Read, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Provider {
    generation: u64,
}

impl XServerFrontendRenderDeviceProvider for Provider {
    fn open_render_device_fd(&self) -> Result<OwnedFd, XServerFrontendRenderDeviceError> {
        File::open("/dev/zero")
            .map(OwnedFd::from)
            .map_err(|_| XServerFrontendRenderDeviceError::OpenFailed)
    }
    fn dma_buf_import_formats(&self) -> Vec<XServerFrontendDmaBufImportFormat> {
        vec![XServerFrontendDmaBufImportFormat {
            format: DRM_FORMAT_XRGB8888,
            modifiers: vec![self.generation],
        }]
    }
}

/// A bundle whose provider only the bundle owns, and a weak witness to it.
fn bundle(generation: u64) -> (Arc<XServerFrontendDeviceBundle>, Weak<Provider>) {
    let provider = Arc::new(Provider { generation });
    let witness = Arc::downgrade(&provider);
    let bundle = XServerFrontendDeviceBundle::new(generation, provider, None).unwrap();
    (Arc::new(bundle), witness)
}

struct Socket(PathBuf);

impl Socket {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "sophia-bundle-lifetime-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn frontend(path: &Path, first: Arc<XServerFrontendDeviceBundle>) -> XServerFrontend {
    let config = XServerFrontendConfig::new(path, NamespaceId::from_raw(69))
        .unwrap()
        .with_device_bundle(first);
    XServerFrontend::bind(config).unwrap()
}

/// Accept one client on a worker. The connection pins the current generation
/// when it is accepted, before its setup completes.
fn client(frontend: &mut XServerFrontend, path: &Path) -> UnixStream {
    let mut stream = UnixStream::connect(path).unwrap();
    frontend.serve_next_concurrently().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    let mut header = [0; 8];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 1, "setup accepted");
    let mut body = vec![0; usize::from(u16::from_le_bytes([header[6], header[7]])) * 4];
    stream.read_exact(&mut body).unwrap();
    stream
}

/// DRI3 GetSupportedModifiers at depth 24: the generation this connection pinned.
fn pinned_generation(stream: &mut UnixStream) -> Vec<u64> {
    let mut request = vec![
        X_DRI3_MAJOR_OPCODE,
        X_DRI3_GET_SUPPORTED_MODIFIERS_MINOR_OPCODE,
        3,
        0,
    ];
    request.extend_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    request.extend_from_slice(&[24, 32, 0, 0]);
    stream.write_all(&request).unwrap();
    let mut header = [0; 32];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 1, "modifier reply");
    let count = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    let mut body = vec![0; count * 8];
    stream.read_exact(&mut body).unwrap();
    body.chunks_exact(8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

/// Wait, bounded, until every client worker has finished and been collected.
fn settle(frontend: &mut XServerFrontend, clients: usize) {
    let start = Instant::now();
    loop {
        frontend.poll_client_workers().unwrap();
        if frontend.active_client_worker_count() == clients {
            return;
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "client workers did not settle to {clients} after {elapsed:?}: {} remain",
            frontend.active_client_worker_count()
        );
        std::thread::yield_now();
    }
}

#[test]
fn a_configured_first_generation_is_released_once_superseded_and_unleased() {
    let socket = Socket::new();
    let (first, first_provider) = bundle(1);
    let mut frontend = frontend(&socket.0, first);
    let mut old = client(&mut frontend, &socket.0);
    assert_eq!(pinned_generation(&mut old), [1]);

    // Loss and a successor: the connection's lease keeps its own generation.
    frontend.mark_device_generation_unavailable(1).unwrap();
    let (second, _) = bundle(2);
    frontend.install_device_bundle(second).unwrap();
    assert!(
        first_provider.upgrade().is_some(),
        "a connection still pinned to generation 1 keeps its provider"
    );
    assert_eq!(pinned_generation(&mut old), [1], "the lease is unchanged");
    let mut new = client(&mut frontend, &socket.0);
    assert_eq!(pinned_generation(&mut new), [2]);

    // The last lease ends; the next install prunes every unleased generation.
    drop(old);
    settle(&mut frontend, 1);
    let (third, _) = bundle(3);
    frontend.install_device_bundle(third).unwrap();
    assert!(
        first_provider.upgrade().is_none(),
        "the frontend's configuration must not keep a superseded, unleased generation 1 alive"
    );
    assert_eq!(
        pinned_generation(&mut new),
        [2],
        "a live lease is untouched"
    );
    drop(new);
    settle(&mut frontend, 0);
}

#[test]
fn a_configured_first_generation_does_not_hold_one_of_the_sixteen_slots() {
    let socket = Socket::new();
    let (first, first_provider) = bundle(1);
    let mut frontend = frontend(&socket.0, first);
    let mut leases = Vec::new();
    // Generations 2..=17 each pinned by a live connection: sixteen leased
    // generations once generation 1, unleased, has been pruned.
    for generation in 2..=X_SERVER_FRONTEND_DEVICE_BUNDLE_CAPACITY as u64 + 1 {
        let (next, _) = bundle(generation);
        frontend.install_device_bundle(next).unwrap_or_else(|error| {
            panic!(
                "generation {generation} with {} leased generations and generation 1 alive={}: {error:?}",
                leases.len(),
                first_provider.upgrade().is_some()
            )
        });
        let mut lease = client(&mut frontend, &socket.0);
        assert_eq!(pinned_generation(&mut lease), [generation]);
        leases.push(lease);
    }
    assert!(first_provider.upgrade().is_none());
    // The bound itself is unchanged: a seventeenth leased generation refuses.
    let next = X_SERVER_FRONTEND_DEVICE_BUNDLE_CAPACITY as u64 + 2;
    let (refused, _) = bundle(next);
    assert_eq!(
        frontend.install_device_bundle(refused),
        Err(XServerFrontendDeviceBundleError::Capacity)
    );
    drop(leases);
    settle(&mut frontend, 0);
}

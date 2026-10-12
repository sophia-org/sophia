#![cfg(test)]
use super::*;

fn x_candidate(revision: u64, identity: u64) -> (CompiledNamespace, NamespaceDiscovery) {
    let (mut view, mut discovery) = candidate(revision, identity);
    for endpoint in &mut view.endpoints {
        endpoint.endpoint.service.protocol = EndpointProtocol::X11;
        endpoint.endpoint.service.interface = "x11".into();
    }
    for endpoint in &mut discovery.endpoints {
        endpoint.service.protocol = EndpointProtocol::X11;
        endpoint.service.interface = "x11".into();
    }
    (view, discovery)
}

#[test]
fn x11_aliases_capture_the_frontend_inode_without_a_proxy_or_new_listener() {
    let f = Fixture::new();
    let source = f.0.join("X77");
    let listener = UnixListener::bind(&source).unwrap();
    listener.set_nonblocking(true).unwrap();
    let metadata = fs::metadata(&source).unwrap();
    let (view, discovery) = x_candidate(1, 10);
    let capture =
        CapturedX11Socket::capture(view.endpoints[0].endpoint.service.clone(), &source).unwrap();
    // Replacing the original pathname cannot retarget this captured inode.
    fs::rename(&source, f.0.join("original")).unwrap();
    let replacement = UnixListener::bind(&source).unwrap();
    replacement.set_nonblocking(true).unwrap();
    let mut publisher = f.publisher();
    let prepared = publisher
        .prepare_with_x11(view, discovery, &[capture])
        .unwrap();
    let published = publisher.publish(prepared).unwrap();
    assert!(
        published.listeners.is_empty(),
        "X11 aliases must not introduce a proxy listener"
    );
    for endpoint in &published.lease.view().endpoints {
        let path = published.lease.directory().join(&endpoint.path);
        let actual = fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (actual.dev(), actual.ino(), actual.mode()),
            (metadata.dev(), metadata.ino(), metadata.mode())
        );
        let _peer = UnixStream::connect(path).unwrap();
        assert!(
            listener.accept().is_ok(),
            "X11 alias must reach the captured frontend"
        );
        assert_eq!(
            replacement.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
    drop(published);
    drop(publisher);
    assert!(source.exists());
    assert!(f.0.join("original").exists());
}

#[test]
fn missing_wrong_and_retired_x11_sources_refuse_without_partial_publication() {
    let f = Fixture::new();
    let mut publisher = f.publisher();
    let (view, discovery) = x_candidate(1, 10);
    assert!(publisher.prepare(view.clone(), discovery.clone()).is_err());
    assert!(publisher.current().is_none());
    let source = f.0.join("X77");
    let _listener = UnixListener::bind(&source).unwrap();
    let mut foreign = view.endpoints[0].endpoint.service.clone();
    foreign.generation += 1;
    let capture = CapturedX11Socket::capture(foreign, &source).unwrap();
    assert!(
        publisher
            .prepare_with_x11(view.clone(), discovery.clone(), &[capture])
            .is_err()
    );
    let capture =
        CapturedX11Socket::capture(view.endpoints[0].endpoint.service.clone(), &source).unwrap();
    fs::remove_file(&source).unwrap();
    assert!(
        publisher
            .prepare_with_x11(view, discovery, &[capture])
            .is_err()
    );
    assert!(publisher.current().is_none());
    assert!(
        fs::read_dir(publisher.directory())
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn x11_capture_refuses_symlinks_regular_files_and_nonprivate_groups() {
    let f = Fixture::new();
    let source = f.0.join("X77");
    let _listener = UnixListener::bind(&source).unwrap();
    let service = x_candidate(1, 10).0.endpoints[0].endpoint.service.clone();
    let alias = f.0.join("link");
    symlink(&source, &alias).unwrap();
    assert!(CapturedX11Socket::capture(service.clone(), &alias).is_err());
    fs::write(f.0.join("file"), b"not a socket").unwrap();
    assert!(CapturedX11Socket::capture(service.clone(), &f.0.join("file")).is_err());
    fs::set_permissions(&f.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(CapturedX11Socket::capture(service, &source).is_err());
}

#[test]
fn failure_after_linking_x11_reclaims_only_candidate_names() {
    let f = Fixture::new();
    let source = f.0.join("X77");
    let listener = UnixListener::bind(&source).unwrap();
    listener.set_nonblocking(true).unwrap();
    let before = fs::metadata(&source).unwrap();
    let mut publisher = f.publisher();
    let (old_view, old_discovery) = candidate(1, 20);
    let prepared = publisher.prepare(old_view, old_discovery).unwrap();
    let old = publisher.publish(prepared).unwrap();
    let (view, discovery) = x_candidate(2, 10);
    let capture =
        CapturedX11Socket::capture(view.endpoints[0].endpoint.service.clone(), &source).unwrap();
    assert!(
        publisher
            .prepare_inner(view, discovery, &[capture], |_| {
                Err(io::Error::other("injected failure after X11 alias"))
            })
            .is_err()
    );
    assert!(!publisher.directory().join("v2").exists());
    assert_eq!(
        publisher.current().unwrap().directory(),
        old.lease.directory()
    );
    let after = fs::metadata(&source).unwrap();
    assert_eq!(
        (after.dev(), after.ino(), after.nlink(), after.mode()),
        (before.dev(), before.ino(), before.nlink(), before.mode()),
        "failed publication must not alter the frontend socket"
    );
    let _peer = UnixStream::connect(&source).unwrap();
    assert!(listener.accept().is_ok());
}

#![cfg(test)]

use super::*;
use crate::namespace_recipe::{Input, compile};
use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-pub-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn publisher(&self) -> NamespacePublisher {
        NamespacePublisher::new(&self.0, NamespaceId::from_raw(1), 7).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn candidate(revision: u64, service: u64) -> (CompiledNamespace, NamespaceDiscovery) {
    let endpoint = GrantedEndpoint {
        service: ServiceReference {
            identity: service,
            generation: 3,
            protocol: EndpointProtocol::NineP2000L,
            interface: "inspection".into(),
            version: 1,
        },
        scope: EndpointGrantScope {
            namespace: NamespaceId::from_raw(1),
            principal: 7,
            grant: service,
            generation: 1,
        },
    };
    let view = compile(Input {
        revision,
        namespace: NamespaceId::from_raw(1),
        principal: 7,
        source: "namespace-recipe 1\nmount a roles/inspection\nbind roles/inspection roles/status",
        fragments: &BTreeMap::new(),
        services: &BTreeMap::from([("a".into(), endpoint)]),
    })
    .unwrap();
    let discovery = NamespaceDiscovery {
        revision,
        endpoints: view
            .endpoints
            .iter()
            .map(|e| DiscoveredEndpoint {
                path: e.path.clone(),
                service: e.endpoint.service.clone(),
                availability: ServiceAvailability::Available,
            })
            .collect(),
    };
    (view, discovery)
}

#[test]
fn new_view_changes_names_without_retargeting_retained_listeners() {
    let f = Fixture::new();
    let mut p = f.publisher();
    let (v, d) = candidate(1, 10);
    let prepared = p.prepare(v, d).unwrap();
    assert!(!p.directory().join("current").exists());
    let old = p.publish(prepared).unwrap();
    let (v, d) = candidate(2, 20);
    let prepared = p.prepare(v, d).unwrap();
    assert_eq!(
        fs::canonicalize(p.directory().join("current")).unwrap(),
        old.lease.directory()
    );
    let new = p.publish(prepared).unwrap();
    assert_eq!(old.lease.view().endpoints[0].endpoint.service.identity, 10);
    assert_eq!(new.lease.view().endpoints[0].endpoint.service.identity, 20);
    let _old_peer = UnixStream::connect(old.lease.directory().join("roles/inspection")).unwrap();
    assert!(old.listeners[0].listener.accept().is_ok());
    assert_eq!(
        new.listeners[0].listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        fs::canonicalize(p.directory().join("current")).unwrap(),
        new.lease.directory()
    );
}

#[test]
fn failed_materialization_and_failed_publication_preserve_the_old_view() {
    let f = Fixture::new();
    let mut p = f.publisher();
    let (v, d) = candidate(1, 10);
    let prepared = p.prepare(v, d).unwrap();
    let old = p.publish(prepared).unwrap();
    let (v, d) = candidate(2, 20);
    assert!(
        p.prepare_inner(v, d, |n| if n == 1 {
            Err(io::Error::other("injected endpoint failure"))
        } else {
            Ok(())
        })
        .is_err()
    );
    assert!(!p.directory().join("v2").exists());
    assert!(!p.directory().join(".socket").exists());
    let (v, d) = candidate(3, 30);
    let prepared = p.prepare(v, d).unwrap();
    let failed = prepared.lease.directory().to_owned();
    fs::create_dir(p.directory().join(".current-next")).unwrap();
    assert!(p.publish(prepared).is_err());
    assert!(!failed.exists());
    assert_eq!(p.current().unwrap().directory(), old.lease.directory());
    assert_eq!(
        fs::canonicalize(p.directory().join("current")).unwrap(),
        old.lease.directory()
    );
    fs::remove_dir(p.directory().join(".current-next")).unwrap();
}

#[test]
fn last_lease_reclaims_old_view_even_after_publisher_exit() {
    let f = Fixture::new();
    let mut p = f.publisher();
    let root = p.directory().to_owned();
    let (v, d) = candidate(1, 10);
    let prepared = p.prepare(v, d).unwrap();
    let published = p.publish(prepared).unwrap();
    let path = published.lease.directory().to_owned();
    drop(p);
    assert!(path.exists());
    drop(published);
    assert!(!path.exists());
    assert!(!root.exists());
}

#[test]
fn discovery_cannot_substitute_a_service_or_scope_and_stale_candidates_refuse() {
    let f = Fixture::new();
    let mut p = f.publisher();
    let (v, mut d) = candidate(1, 10);
    d.endpoints[0].service.identity = 20;
    assert!(p.prepare(v, d).is_err());
    let (mut v, d) = candidate(1, 10);
    v.endpoints[0].endpoint.scope.principal = 8;
    assert!(p.prepare(v, d).is_err());
    let (v, d) = candidate(1, 10);
    let old = p.prepare(v, d).unwrap();
    let (v, d) = candidate(2, 20);
    let new = p.prepare(v, d).unwrap();
    p.publish(new).unwrap();
    assert!(p.publish(old).is_err());
}

#[test]
fn long_logical_names_connect_via_pinned_socket_identity() {
    let f = Fixture::new();
    let mut p = f.publisher();
    let (mut v, mut d) = candidate(1, 10);
    let name = format!("{}/{}", "x".repeat(200), "y".repeat(200));
    v.endpoints.truncate(1);
    d.endpoints.truncate(1);
    v.endpoints[0].path = name.clone();
    d.endpoints[0].path = name.clone();
    let prepared = p.prepare(v, d).unwrap();
    let published = p.publish(prepared).unwrap();
    let node = rustix::fs::open(
        published.lease.directory().join(name),
        rustix::fs::OFlags::PATH | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .unwrap();
    let _stream = UnixStream::connect(format!("/proc/self/fd/{}", node.as_raw_fd())).unwrap();
    assert!(published.listeners[0].listener.accept().is_ok());
}

#[test]
fn retained_view_capacity_is_reclaimed_without_reusing_generations() {
    let f = Fixture::new();
    let mut p = f.publisher();
    let mut candidates = Vec::new();
    for revision in 1..=MAX_RETAINED_VIEWS as u64 {
        let (v, d) = candidate(revision, 10);
        candidates.push(p.prepare(v, d).unwrap());
    }
    let (v, d) = candidate(100, 10);
    assert!(
        p.prepare(v, d).is_err(),
        "retained candidates must have a finite bound"
    );
    let removed = candidates.pop().unwrap();
    let old_path = removed.lease.directory().to_owned();
    drop(removed);
    assert!(!old_path.exists());
    let (v, d) = candidate(100, 10);
    let next = p.prepare(v, d).unwrap();
    assert_ne!(next.lease.directory(), old_path);
    assert!(p.current().is_none());
}

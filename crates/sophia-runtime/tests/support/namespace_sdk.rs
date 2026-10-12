use super::*;
use sophia_namespace_client::Namespace;
use std::io::{Read, Write};
use std::process::Command;

struct CPeer(PathBuf);
impl CPeer {
    fn build() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let sdk = root.join("vendor/c-desktop-sdk/source/src");
        let target =
            PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").expect("cargo-slot required"));
        let path = target.join(format!("namespace-c-peer-{}", std::process::id()));
        assert!(
            Command::new("/usr/bin/cc")
                .args(["-std=c99", "-Wall", "-Wextra", "-Werror", "-pedantic"])
                .arg("-I")
                .arg(&sdk)
                .arg(sdk.join("namespace/records.c"))
                .arg(sdk.join("namespace/directory.c"))
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/support/namespace_discovery_peer.c")
                )
                .arg("-o")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        Self(path)
    }
    fn connect(&self, view: &PublishedNamespace, index: usize, marker: &str) {
        let endpoint = &view.lease.view().endpoints[index];
        assert!(
            Command::new(&self.0)
                .arg(view.lease.directory())
                .arg(&endpoint.path)
                .arg(endpoint.endpoint.service.identity.to_string())
                .arg(marker)
                .status()
                .unwrap()
                .success(),
            "C SDK must consume the production discovery and endpoint"
        );
        received(&view.listeners[index].listener, marker);
    }
}
impl Drop for CPeer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn received(listener: &UnixListener, marker: &str) {
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut bytes = vec![0; marker.len()];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(
        bytes,
        marker.as_bytes(),
        "SDK connection must reach the captured listener"
    );
}

#[test]
fn independent_sdks_consume_production_views_and_keep_old_service_identity() {
    let f = Fixture::new();
    let c = CPeer::build();
    let mut publisher = f.publisher();
    let (mut v, mut d) = candidate(1, 10);
    // The exact same long logical name is rebound to another existing service.
    let long = format!("{}/{}", "a".repeat(180), "b".repeat(180));
    v.endpoints[0].path = long.clone();
    d.endpoints[0].path = long.clone();
    let prepared = publisher.prepare(v, d).unwrap();
    let old = publisher.publish(prepared).unwrap();
    let old_reader = Namespace::open(old.lease.directory()).unwrap();
    let (mut v, mut d) = candidate(2, 20);
    v.endpoints[0].path = long.clone();
    d.endpoints[0].path = long.clone();
    let prepared = publisher.prepare(v, d).unwrap();
    let next = publisher.publish(prepared).unwrap();
    let next_reader = Namespace::open(next.lease.directory()).unwrap();
    for (reader, view, identity, marker) in [
        (&old_reader, &old, 10, "old"),
        (&next_reader, &next, 20, "new"),
    ] {
        for index in 0..2 {
            let name = &view.lease.view().endpoints[index].path;
            let endpoint = reader.discovery().endpoint(name).unwrap();
            assert_eq!((endpoint.service, endpoint.generation), (identity, 3));
            let mut stream = reader.connect(name).unwrap();
            stream.write_all(marker.as_bytes()).unwrap();
            received(&view.listeners[index].listener, marker);
            c.connect(view, index, marker);
        }
    }
}

//! Real SysV segments, created and removed by util-linux so the authority tests
//! keep the workspace's unsafe-code prohibition.
use sophia_protocol::{NamespaceId, Size};
use sophia_x_authority::{XAuthorityRuntime, XResourceId, XWireClientResourceRange};
use std::{process::Command, sync::Arc};

struct Segment(u32);

impl Segment {
    fn new(value: u8) -> Self {
        let output = Command::new("ipcmk")
            .args(["-M", "4096"])
            .env("LC_ALL", "C")
            .output()
            .expect("util-linux ipcmk creates the SysV fixture");
        assert!(output.status.success(), "{output:?}");
        let id = String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .last()
            .unwrap()
            .parse()
            .unwrap();
        let segment = Self(id);
        sophia_sysv_shm::write_bytes(id, 0, &[value; 16]).unwrap();
        segment
    }
}

impl Drop for Segment {
    fn drop(&mut self) {
        let _ = Command::new("ipcrm")
            .args(["-m", &self.0.to_string()])
            .status();
    }
}

const NS: NamespaceId = NamespaceId::from_raw(1);
const SEG: XResourceId = XResourceId::new(0x200001, 1);

#[test]
fn attached_sysv_mapping_survives_between_requests_and_ends_at_detach() {
    let segment = Segment::new(17);
    let mut runtime = XAuthorityRuntime::new();
    runtime
        .attach_shm_segment(NS, SEG, segment.0, false, 1)
        .unwrap();
    let mapping = runtime.shm_segment_mapping(NS, SEG).unwrap();
    let weak = Arc::downgrade(&mapping);
    drop(mapping);
    assert!(
        weak.upgrade().is_some(),
        "attached segment must retain its mapping between uploads"
    );
    for _ in 0..16 {
        let mapping = runtime.shm_segment_mapping(NS, SEG).unwrap();
        assert!(Arc::ptr_eq(&mapping, &weak.upgrade().unwrap()));
        assert_eq!(mapping.copy_bytes(0, 16).unwrap(), [17; 16]);
    }
    assert!(
        runtime
            .shm_segment_mapping(NamespaceId::from_raw(2), SEG)
            .is_err()
    );
    assert!(
        runtime
            .detach_shm_segment(NamespaceId::from_raw(2), SEG)
            .is_err()
    );
    assert!(weak.upgrade().is_some());
    runtime.detach_shm_segment(NS, SEG).unwrap();
    assert!(
        weak.upgrade().is_none(),
        "detach must unmap an unreferenced segment"
    );
}

#[test]
fn sysv_alias_and_pixmap_own_the_mapping_independently() {
    let segment = Segment::new(23);
    let alias = XResourceId::new(0x200002, 1);
    let pixmap = XResourceId::new(0x200003, 1);
    let mut runtime = XAuthorityRuntime::new();
    for id in [SEG, alias] {
        runtime
            .attach_shm_segment(NS, id, segment.0, false, 1)
            .unwrap();
    }
    let mapping = runtime.shm_segment_mapping(NS, SEG).unwrap();
    let weak = Arc::downgrade(&mapping);
    assert!(Arc::ptr_eq(
        &mapping,
        &runtime.shm_segment_mapping(NS, alias).unwrap()
    ));
    drop(mapping);
    runtime.detach_shm_segment(NS, SEG).unwrap();
    assert!(
        weak.upgrade().is_some(),
        "second attachment must retain the mapping"
    );
    runtime
        .create_shm_pixmap(
            NS,
            pixmap,
            Size {
                width: 2,
                height: 2,
            },
            32,
            1,
            alias,
            0,
        )
        .unwrap();
    runtime.detach_shm_segment(NS, alias).unwrap();
    assert_eq!(weak.upgrade().unwrap().copy_bytes(0, 16).unwrap(), [23; 16]);
    runtime.free_pixmap(NS, pixmap).unwrap();
    assert!(weak.upgrade().is_none(), "last pixmap release must unmap");
}

#[test]
fn disconnect_and_segment_replacement_do_not_keep_old_mappings() {
    let first = Segment::new(31);
    let second = Segment::new(47);
    let mut runtime = XAuthorityRuntime::new();
    runtime
        .attach_shm_segment(NS, SEG, first.0, false, 1)
        .unwrap();
    let old = Arc::downgrade(&runtime.shm_segment_mapping(NS, SEG).unwrap());
    assert!(old.upgrade().is_some());
    // The runtime API can replace a record; no cached old mapping may leak
    // through it, even when the caller reuses the exact resource identity.
    runtime
        .attach_shm_segment(NS, SEG, second.0, false, 2)
        .unwrap();
    assert!(old.upgrade().is_none());
    let mapping = runtime.shm_segment_mapping(NS, SEG).unwrap();
    assert_eq!(mapping.copy_bytes(0, 16).unwrap(), [47; 16]);
    let new = Arc::downgrade(&mapping);
    drop(mapping);
    runtime
        .release_client_resource_range(
            NS,
            XWireClientResourceRange {
                base: 0x200000,
                mask: 0xfffff,
            },
        )
        .unwrap();
    assert_eq!(runtime.shm_segment_count(), 0);
    assert!(new.upgrade().is_none());
    for generation in 3..35 {
        runtime
            .attach_shm_segment(NS, SEG, first.0, false, generation)
            .unwrap();
        let weak = Arc::downgrade(&runtime.shm_segment_mapping(NS, SEG).unwrap());
        assert!(weak.upgrade().is_some());
        runtime.detach_shm_segment(NS, SEG).unwrap();
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn invalid_sysv_attachment_stays_lazy_and_does_not_reuse_replaced_descriptor() {
    let mut runtime = XAuthorityRuntime::new();
    let (mapping, _fd) = sophia_sysv_shm::DescriptorMapping::create_sealed(4096).unwrap();
    let mapping = Arc::new(sophia_sysv_shm::ClientMapping::Descriptor(mapping));
    let weak = Arc::downgrade(&mapping);
    runtime
        .attach_shm_descriptor_segment(NS, SEG, mapping, false, 1)
        .unwrap();
    runtime
        .attach_shm_segment(NS, SEG, u32::MAX, false, 2)
        .unwrap();
    assert!(
        weak.upgrade().is_none(),
        "replacement must release the previous backing"
    );
    assert!(runtime.shm_segment_mapping(NS, SEG).is_err());
    runtime.detach_shm_segment(NS, SEG).unwrap();
}

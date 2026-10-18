//! Real sealed mappings for store tests; no transport or worker-pool claim.
use super::{ContentResourceStore, IsbPixelValidation, ValidatedIsbPixels};
use sophia_protocol::{ContentResourceBegin, TransactionId};
use std::io::Write;

pub fn validated(bytes: &[u8]) -> ValidatedIsbPixels {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "content-store-test",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(bytes).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    let mapping = sophia_linux_peer::SealedMapping::map(file.into(), bytes.len()).unwrap();
    let mut work = IsbPixelValidation::new(mapping).unwrap();
    while !work.step().unwrap() {}
    work.finish().unwrap()
}

pub fn accept(
    store: &mut ContentResourceStore,
    tx: TransactionId,
    description: ContentResourceBegin,
    bytes: &[u8],
    now: u64,
) {
    store.begin_isb(tx, description.clone(), now).unwrap();
    store
        .complete_isb(
            description.grant,
            description.resource,
            &mut Some(Ok(validated(bytes))),
            now,
        )
        .unwrap();
}

#![cfg(test)]
//! Read-only test observation of logical allocator custody, not wire fid proof.
use super::super::PolicyFilesystemQids;
use std::sync::Arc;

pub(in crate::live_session) fn same_owner(
    left: &PolicyFilesystemQids,
    right: &PolicyFilesystemQids,
) -> bool {
    Arc::ptr_eq(&left.0.0, &right.0.0)
}

pub(in crate::live_session) fn watermark(owner: &PolicyFilesystemQids) -> u64 {
    *owner.0.0.lock().expect("logical WM Qid allocator")
}

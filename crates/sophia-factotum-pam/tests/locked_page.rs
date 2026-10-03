//! A locked page holds what is written to it and gives back zeroes once
//! zeroed; a page that cannot be mapped is refused, never handed out.
use sophia_factotum_pam::LockedPage;

#[test]
fn a_page_holds_its_bytes_until_zeroed() {
    let mut page = LockedPage::new(4096).expect("one page within the memlock limit");
    assert_eq!(page.as_slice().len(), 4096);
    assert!(
        page.as_slice().iter().all(|byte| *byte == 0),
        "fresh pages are zero"
    );
    page.as_mut_slice()[..6].copy_from_slice(b"secret");
    page.as_mut_slice()[4095] = 7;
    assert_eq!(&page.as_slice()[..6], b"secret");
    page.zero();
    assert!(page.as_slice().iter().all(|byte| *byte == 0));
}

#[test]
fn an_empty_page_is_refused() {
    assert!(LockedPage::new(0).is_none());
}

use sophia_renderer_native_egl::NativeRendererImageCustody;
use std::{cell::Cell, rc::Rc};

struct Allocation(Rc<Cell<usize>>);
impl Drop for Allocation {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn promotion_keeps_the_exact_capture_and_pool_custody_until_eviction() {
    let released = Rc::new(Cell::new(0));
    let allocation = Rc::new(Allocation(released.clone()));
    let retained = Rc::downgrade(&allocation);
    let pool = Rc::new(Allocation(released.clone()));
    let pool_retained = Rc::downgrade(&pool);
    let mut image = NativeRendererImageCustody::staged(allocation, Some(pool), 4096);
    assert!(!image.is_promoted());
    let address = Rc::as_ptr(image.buffer());
    assert!(image.promote(), "staged capture must advance at retirement");
    assert!(
        image.is_promoted(),
        "promotion must persist on the captured allocation"
    );
    assert!(
        !image.promote(),
        "duplicate retirement cannot promote twice"
    );
    assert_eq!(Rc::as_ptr(image.buffer()), address);
    assert!(Rc::ptr_eq(
        image.pooled().unwrap(),
        &pool_retained.upgrade().unwrap()
    ));
    assert_eq!(image.bytes(), 4096);
    assert_eq!(
        released.get(),
        0,
        "promotion must retain both resource owners"
    );
    drop(image);
    assert!(retained.upgrade().is_none() && pool_retained.upgrade().is_none());
    assert_eq!(released.get(), 2);
}

#[test]
fn discarding_a_staged_capture_releases_its_storage_without_promotion() {
    let released = Rc::new(Cell::new(0));
    let image = NativeRendererImageCustody::<_, ()>::staged(Allocation(released.clone()), None, 64);
    assert!(!image.is_promoted());
    drop(image);
    assert_eq!(released.get(), 1);
}

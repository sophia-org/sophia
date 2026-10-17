use sophia_portal_client::read_window::*;
#[test]
fn reversed_short_reads_cover_each_byte_once_with_fixed_window() {
    let mut w = ReadWindow::new(1003, 512, 256, 60000).unwrap();
    let mut seen = [false; 1003];
    while !w.payload_complete() {
        let requests: Vec<_> = (0..4).filter_map(|_| w.reserve().unwrap()).collect();
        assert!(!requests.is_empty());
        assert_eq!(w.reserve(), Ok(None));
        for r in requests.into_iter().rev() {
            assert!(r.count <= 256);
            let n = if r.count > 1 { r.count as usize / 2 } else { 1 };
            assert_eq!(w.accept(r.token, n), Ok(r.offset));
            for byte in &mut seen[r.offset as usize..r.offset as usize + n] {
                assert!(!*byte);
                *byte = true;
            }
        }
    }
    assert!(seen.into_iter().all(|b| b));
    assert_eq!(w.received(), 1003);
    assert_eq!(w.reserve(), Ok(None));
}
#[test]
fn bounds_terminal_errors_and_full_image_range() {
    for (length, msize, chunk) in [
        (0, 512, 1),
        (67108865, 512, 1),
        (1, 511, 1),
        (1, 65537, 1),
        (1, 512, 0),
    ] {
        assert!(ReadWindow::new(length, msize, 0, chunk).is_err());
    }
    for invalid in [0, 502] {
        let mut w = ReadWindow::new(1000, 512, 0, u32::MAX).unwrap();
        let r = w.reserve().unwrap().unwrap();
        assert_eq!(r.count, 501);
        assert_eq!(w.accept(r.token, invalid), Err(Error::Invalid));
        assert_eq!(w.reserve(), Err(Error::Invalid));
        assert!(!w.payload_complete());
    }
    let mut w = ReadWindow::new(1, 512, 0, 1).unwrap();
    let r = w.reserve().unwrap().unwrap();
    w.accept(r.token, 1).unwrap();
    assert_eq!(w.accept(r.token, 1), Err(Error::Invalid));
    assert!(!w.payload_complete());
    let mut w = ReadWindow::new(67108864, 65536, 0, 65536).unwrap();
    while let Some(r) = w.reserve().unwrap() {
        assert!(r.count <= 65525);
        w.accept(r.token, r.count as usize).unwrap();
    }
    assert!(w.payload_complete());
    w.close();
    assert_eq!(w.reserve(), Err(Error::Closed));
}

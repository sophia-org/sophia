use super::*;

fn pipeline() -> Pipeline {
    let (stream, _peer) = UnixStream::pair().unwrap();
    Pipeline {
        stream,
        limits: PipelineLimits::default(),
        msize: PipelineLimits::default().msize,
        next_tag: 0,
        next_fid: 0,
        fids: BTreeSet::new(),
        in_flight: BTreeMap::new(),
        flushed: BTreeSet::new(),
        completed: VecDeque::new(),
        completed_tags: BTreeSet::new(),
        reserved_tags: 0,
        reserved_reply_bytes: 0,
        out_buf: Vec::new(),
        out_start: 0,
        scrub_output: false,
        in_buf: Vec::new(),
        poisoned: false,
    }
}

#[test]
fn ordinary_partial_sends_preserve_storage_and_exact_unsent_bytes() {
    let mut p = pipeline();
    p.write(Fid(1), 0, &[0x5a; 1024]).unwrap();
    let original = p.out_buf.clone();
    let address = p.out_buf.as_ptr();
    let mut sent = 0;
    for count in [1, 5, 1, 16, 500, 100] {
        p.record_output_write(count);
        sent += count;
        assert_eq!(p.out_buf, original, "partial sends must not shift the tail");
        assert_eq!(p.out_buf.as_ptr(), address);
        assert_eq!(p.output_pending(), original.len() - sent);
        assert_eq!(&p.out_buf[p.out_start..], &original[sent..]);
    }
    p.record_output_write(p.output_pending());
    assert_eq!(p.out_start, 0);
    assert!(p.out_buf.is_empty());
    assert_eq!(
        p.out_buf.as_ptr(),
        address,
        "retain storage for the next frame"
    );
}

#[test]
fn consumed_bytes_do_not_spend_output_budget_and_append_compacts_when_needed() {
    let mut p = pipeline();
    p.limits.max_buffered_output = 64;
    p.out_buf = Vec::with_capacity(64);
    p.write(Fid(1), 0, &[1; 40]).unwrap();
    p.record_output_write(40);
    let tail = p.out_buf[p.out_start..].to_vec();
    let address = p.out_buf.as_ptr();
    p.write(Fid(1), 40, &[2; 18]).unwrap();
    assert_eq!(p.output_pending(), 64);
    assert_eq!(p.out_start, 0);
    assert_eq!(&p.out_buf[..tail.len()], tail);
    assert_eq!(p.out_buf.as_ptr(), address);
    assert!(matches!(
        p.write(Fid(1), 58, &[3]),
        Err(PipelineError::Limit("output buffer full"))
    ));
}

#[test]
fn a_secret_after_partial_plain_output_normalizes_before_reserving() {
    let mut p = pipeline();
    p.write(Fid(1), 0, &[0x5a; 32]).unwrap();
    p.record_output_write(25);
    let tail = p.out_buf[p.out_start..].to_vec();
    p.write_secret(Fid(2), 0, b"secret").unwrap();
    assert_eq!(p.out_start, 0);
    assert_eq!(&p.out_buf[..tail.len()], tail);
    let capacity = p.output_capacity();
    p.record_output_write(tail.len() + 23 + 1);
    assert_eq!(&p.out_buf, b"ecret");
    assert!(p.scrubbing_output());
    p.write(Fid(3), 0, b"later").unwrap();
    assert_eq!(p.output_capacity(), capacity);
    p.record_output_write(p.output_pending());
    assert!(!p.scrubbing_output());
    assert_eq!(p.out_start, 0);
}

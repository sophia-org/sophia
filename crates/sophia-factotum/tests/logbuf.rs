//! The `log` ring against 9front's `log.c`, with the one recorded change:
//! a full ring drops its oldest message.

use sophia_factotum::logbuf::{LogBuf, LogReadTooShort};

#[test]
fn one_message_per_read_with_a_newline() {
    let mut log = LogBuf::new();
    assert_eq!(log.read(64), Ok(None), "an empty log waits");
    log.append("first".into());
    log.append("second".into());
    assert_eq!(log.read(64), Ok(Some(b"first\n".to_vec())));
    assert_eq!(log.read(64), Ok(Some(b"second\n".to_vec())));
    assert_eq!(log.read(4), Err(LogReadTooShort));
}

#[test]
fn a_message_too_long_for_the_read_is_cut_like_9front_cuts_it() {
    let mut log = LogBuf::new();
    // count 10: copy 5 bytes, then drop back to the start of the last one,
    // which is itself dropped.
    log.append("abcdefghijklmnop".into());
    assert_eq!(log.read(10), Ok(Some(b"abcd...\n".to_vec())));
    // A two-byte character straddling the cut is dropped whole.
    log.append("abcéfghijklmnop".into());
    assert_eq!(log.read(9), Ok(Some(b"abc...\n".to_vec())));
}

#[test]
fn a_full_ring_drops_its_oldest_message() {
    let mut log = LogBuf::new();
    for index in 0..130 {
        log.append(format!("{index}"));
    }
    assert_eq!(log.read(64), Ok(Some(b"2\n".to_vec())));
}

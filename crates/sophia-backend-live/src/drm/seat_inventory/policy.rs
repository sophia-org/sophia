use std::ffi::OsStr;

pub(crate) fn valid_seat(seat: &str) -> bool {
    !seat.is_empty() && seat.len() <= 64 && seat.is_ascii() && seat.bytes().all(|byte| byte > b' ')
}

pub(crate) fn is_node_name(name: &OsStr, prefix: &str) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(prefix))
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(crate) fn seat_matches(seat: &str, initialized: bool, assigned: Option<&OsStr>) -> bool {
    // Missing ID_SEAT denotes seat0 only after udev has initialized the record.
    initialized && assigned.unwrap_or_else(|| OsStr::new("seat0")) == seat
}

use sophia_admin_protocol::{Outcome, Status, decode_status};

#[test]
fn literal_status_and_all_outcomes() {
    assert_eq!(
        decode_status(b"v1 ready 7\n"),
        Ok(Status::Ready { connection: 7 })
    );
    assert_eq!(
        decode_status(b"v1 pending 7 9 12\n"),
        Ok(Status::Pending {
            connection: 7,
            request: 9,
            generation: 12
        })
    );
    for (name, outcome) in [
        ("committed", Outcome::Committed),
        ("completed", Outcome::Completed),
        ("unchanged", Outcome::Unchanged),
        ("rejected", Outcome::Rejected),
        ("stale", Outcome::Stale),
        ("denied", Outcome::Denied),
        ("unavailable", Outcome::Unavailable),
        ("overloaded", Outcome::Overloaded),
        ("timed-out", Outcome::TimedOut),
        ("indeterminate", Outcome::Indeterminate),
    ] {
        let wire = format!("v1 outcome 7 9 12 {name} 3:é!\n");
        let status = decode_status(wire.as_bytes()).unwrap();
        assert_eq!(
            status,
            Status::Outcome {
                connection: 7,
                request: 9,
                generation: 12,
                outcome,
                detail: "é!"
            }
        );
        if let Status::Outcome { detail, .. } = status {
            assert_eq!(detail.as_ptr(), wire.as_bytes()[wire.len() - 4..].as_ptr());
        }
        for end in 0..wire.len() {
            assert!(decode_status(&wire.as_bytes()[..end]).is_err());
        }
    }
    assert!(decode_status(b"v1 outcome 7 9 12 committed 0:\n").is_ok());
    for size in [256, 257] {
        let wire = format!("v1 outcome 7 9 12 rejected {size}:{}\n", "a".repeat(size));
        assert_eq!(decode_status(wire.as_bytes()).is_ok(), size == 256);
    }
}

#[test]
fn malformed_status() {
    for wire in [
        "v1 ready 0\n",
        "v1 ready 7 \n",
        "v1 pending 7 9 0\n",
        "v1 pending 7 9 12 \n",
        "v1 outcome 7 9 12 unknown 0:\n",
        "v1 outcome 7 9 12 committed 0\n",
        "v1 outcome 7 9 12 committed 00:\n",
        "v1 outcome 7 9 12 committed 1:é\n",
        "v1 outcome 7 9 12 committed 2:\u{85}\n",
        "v1 outcome 7 9 12 committed 1:\t\n",
        "v1 outcome 7 9 12 committed 1:\0\n",
        "v1 pending 7 09 12\n",
    ] {
        assert!(decode_status(wire.as_bytes()).is_err(), "{wire:?}");
    }
    assert!(decode_status(b"v1 outcome 7 9 12 committed 2:\xc0\xaf\n").is_err());
}

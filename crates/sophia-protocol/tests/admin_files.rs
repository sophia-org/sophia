use sophia_protocol::admin_files::*;
use sophia_protocol::{ControlCommand, ControlOutcome, ControlOwner};

#[test]
fn literal_records_preserve_command_spaces_and_utf8_byte_lengths() {
    let invocation = AdminInvocation {
        connection: 7,
        request: 9,
        generation: 12,
        command: ControlCommand {
            owner: ControlOwner::Policy,
            name: "next tab".into(),
        },
    };
    let wire = b"v1 invoke 7 9 12 policy 8:next tab\n";
    assert_eq!(decode_admin_invocation(wire).unwrap(), invocation);
    assert_eq!(encode_admin_invocation(&invocation).unwrap(), wire);
    let outcome = AdminOutcome {
        connection: 7,
        request: 9,
        generation: 12,
        outcome: ControlOutcome::Rejected,
        detail: "caf\u{e9}".into(),
    };
    let wire = "v1 outcome 7 9 12 rejected 5:caf\u{e9}\n";
    assert_eq!(decode_admin_outcome(wire.as_bytes()).unwrap(), outcome);
    assert_eq!(encode_admin_outcome(&outcome).unwrap(), wire.as_bytes());
    assert!(decode_admin_outcome("v1 outcome 7 9 12 rejected 4:caf\u{e9}\n".as_bytes()).is_err());
}

#[test]
fn malformed_records_cannot_smuggle_a_second_command_or_ambiguous_identity() {
    for wire in [
        "v2 invoke 7 9 12 policy 8:next tab\n",
        "v1 invoke 0 9 12 policy 8:next tab\n",
        "v1 invoke 7 0 12 policy 8:next tab\n",
        "v1 invoke 7 9 0 policy 8:next tab\n",
        "v1 invoke 07 9 12 policy 8:next tab\n",
        "v1 invoke +7 9 12 policy 8:next tab\n",
        "v1 invoke 18446744073709551616 9 12 policy 8:next tab\n",
        "v1 invoke 7 9 12 root 8:next tab\n",
        "v1 invoke 7 9 12 policy 08:next tab\n",
        "v1 invoke 7 9 12 policy 7:next tab\n",
        "v1 invoke 7 9 12 policy 0:\n",
        "v1 invoke 7 9 12 policy 8:next tab",
        "v1 invoke 7 9 12 policy 8:next tab\r\n",
        "v1 invoke 7 9 12 policy 8:next\ttab\n",
        "v1 invoke 7 9 12 policy 8:next tab\nnext\n",
        "v1 invoke 7 9 12 policy 9:next tab \n",
    ] {
        assert!(
            decode_admin_invocation(wire.as_bytes()).is_err(),
            "{wire:?}"
        );
    }
    assert!(decode_admin_invocation(&[0xff, b'\n']).is_err());
    assert!(decode_admin_outcome(b"v1 outcome 7 9 12 success 0:\n").is_err());
    assert!(decode_admin_outcome(b"v1 outcome 7 9 12 completed 1:\0\n").is_err());
    assert!(decode_admin_outcome(&vec![b' '; ADMIN_LINE_MAX_BYTES + 1]).is_err());
}

#[test]
fn encoders_enforce_bounds_before_formatting_and_preserve_uncertain_outcomes() {
    let mut invocation = AdminInvocation {
        connection: u64::MAX,
        request: u64::MAX,
        generation: u64::MAX,
        command: ControlCommand {
            owner: ControlOwner::Session,
            name: "x".repeat(128),
        },
    };
    assert!(encode_admin_invocation(&invocation).unwrap().len() <= ADMIN_LINE_MAX_BYTES);
    invocation.command.name.push('x');
    assert!(encode_admin_invocation(&invocation).is_err());
    for outcome in [
        ControlOutcome::Committed,
        ControlOutcome::Completed,
        ControlOutcome::Unchanged,
        ControlOutcome::Rejected,
        ControlOutcome::Stale,
        ControlOutcome::Denied,
        ControlOutcome::Unavailable,
        ControlOutcome::Overloaded,
        ControlOutcome::TimedOut,
        ControlOutcome::Indeterminate,
    ] {
        let mut value = AdminOutcome {
            connection: u64::MAX,
            request: u64::MAX,
            generation: u64::MAX,
            outcome,
            detail: "x".repeat(ADMIN_DETAIL_MAX_BYTES),
        };
        let wire = encode_admin_outcome(&value).unwrap();
        assert!(wire.len() <= ADMIN_LINE_MAX_BYTES);
        assert_eq!(decode_admin_outcome(&wire).unwrap(), value);
        value.detail.push('x');
        assert!(encode_admin_outcome(&value).is_err());
        value.detail.clear();
        assert_eq!(
            decode_admin_outcome(&encode_admin_outcome(&value).unwrap()).unwrap(),
            value
        );
        value.request = 0;
        assert!(encode_admin_outcome(&value).is_err());
    }
}

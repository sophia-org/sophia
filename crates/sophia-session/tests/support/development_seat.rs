#![cfg(test)]
use super::*;

fn observation() -> Observation {
    Observation {
        owner: ":1.42".into(),
        path: "/org/freedesktop/login1/session/_88".into(),
        id: "88".into(),
        uid: 1000,
        seat: "seat-sophia-dev".into(),
        seat_path: "/org/freedesktop/login1/seat/seat_2dsophia_2ddev".into(),
        active: true,
        remote: false,
        kind: "wayland".into(),
        class: "user".into(),
        tty: String::new(),
        vt: 0,
        seat_has_vts: false,
    }
}

fn environment() -> Environment {
    Environment {
        backend: Some("logind".into()),
        session: Some("88".into()),
        ..Environment::default()
    }
}

fn seat() -> DevelopmentSeat {
    DevelopmentSeat::parse("seat-sophia-dev".into()).unwrap()
}

#[test]
fn secondary_mode_requires_inputless_native_and_an_elapsed_time_bound() {
    let bound = Some(Duration::from_secs(60));
    seat().validate_arguments(true, true, false, bound).unwrap();
    for (native, no_input, proof, bound) in [
        (false, true, false, bound),
        (true, false, false, bound),
        (true, true, true, bound),
        (true, true, false, None),
        (true, true, false, Some(Duration::ZERO)),
        (true, true, false, Some(Duration::from_millis(300_001))),
    ] {
        assert!(
            seat()
                .validate_arguments(native, no_input, proof, bound)
                .is_err()
        );
    }
    for name in ["seat0", "", "../seat1", "seat/1", "seat"] {
        assert!(DevelopmentSeat::parse(name.into()).is_err(), "{name}");
    }
}

#[test]
fn two_server_observations_and_the_opened_seat_must_agree() {
    let mut calls = 0;
    let granted = admit(&seat(), 1000, &environment(), || {
        calls += 1;
        Ok(observation())
    })
    .unwrap();
    assert_eq!(calls, 2);
    granted
        .check_opened_seat("seat-sophia-dev", &granted)
        .unwrap();
    assert!(granted.check_opened_seat("seat0", &granted).is_err());
    let mut changed = observation();
    changed.owner = ":1.43".into();
    assert!(
        granted
            .check_opened_seat("seat-sophia-dev", &Admission(changed))
            .is_err()
    );
}

#[test]
fn another_users_inactive_remote_text_or_vt_login_is_never_admitted() {
    let changes: [fn(&mut Observation); 9] = [
        |r| r.uid = 1001,
        |r| r.active = false,
        |r| r.remote = true,
        |r| r.kind = "tty".into(),
        |r| r.class = "greeter".into(),
        |r| r.vt = 7,
        |r| r.tty = "tty7".into(),
        |r| r.seat = "seat0".into(),
        |r| r.seat_has_vts = true,
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut row = observation();
        change(&mut row);
        assert!(
            admit(&seat(), 1000, &environment(), || Ok(row.clone())).is_err(),
            "field {index}"
        );
    }
}

#[test]
fn no_display_fallback_or_bus_override_is_allowed() {
    let changes: [fn(&mut Environment); 7] = [
        |e| e.backend = None,
        |e| e.backend = Some("noop".into()),
        |e| e.bus = Some("unix:path=/tmp/fake".into()),
        |e| e.session = None,
        |e| e.session = Some("76".into()),
        |e| e.seat = Some("seat0".into()),
        |e| e.vt = Some("7".into()),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut env = environment();
        change(&mut env);
        assert!(
            admit(&seat(), 1000, &env, || Ok(observation())).is_err(),
            "environment {index}"
        );
    }
    let mut env = environment();
    env.kind = Some("tty".into());
    assert!(admit(&seat(), 1000, &env, || Ok(observation())).is_err());
}

#[test]
fn changing_session_owner_or_seat_and_query_errors_refuse() {
    let changes: [fn(&mut Observation); 4] = [
        |r| r.owner = ":1.43".into(),
        |r| r.id = "89".into(),
        |r| r.path = "/different".into(),
        |r| r.seat_path = "/otherseat".into(),
    ];
    for change in changes {
        let mut later = observation();
        change(&mut later);
        let mut rows = [observation(), later].into_iter();
        assert!(admit(&seat(), 1000, &environment(), || Ok(rows.next().unwrap())).is_err());
    }
    for failing_call in [1, 2] {
        let mut calls = 0;
        assert!(
            admit(&seat(), 1000, &environment(), || {
                calls += 1;
                if calls == failing_call {
                    Err("query failed".into())
                } else {
                    Ok(observation())
                }
            })
            .is_err()
        );
        assert_eq!(calls, failing_call);
    }
}

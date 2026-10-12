use sophia_protocol::developer_launch::*;

fn request() -> DeveloperLaunchRequest {
    DeveloperLaunchRequest {
        version: 1,
        request: 17,
        recipe: "observer".into(),
        executable: "/usr/bin/example".into(),
        arguments: vec!["".into(), "a b".into(), "雪".into()],
    }
}

#[test]
fn request_preserves_argv_without_shell_interpretation() {
    let value = request();
    let bytes = encode_developer_launch_request(&value).unwrap();
    assert_eq!(decode_developer_launch_request(&bytes).unwrap(), value);
    for end in 0..bytes.len() {
        assert!(decode_developer_launch_request(&bytes[..end]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(b'x');
    assert!(decode_developer_launch_request(&trailing).is_err());
    let mut extra: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for key in ["role", "principal", "environment", "working_directory"] {
        extra[key] = serde_json::json!("administrator");
        assert!(
            decode_developer_launch_request(&serde_json::to_vec(&extra).unwrap()).is_err(),
            "unknown authority-bearing field {key}"
        );
        extra.as_object_mut().unwrap().remove(key);
    }
    assert!(decode_developer_launch_request(br#"{"version":1,"request":1,"request":2,"recipe":"x","executable":"/x","arguments":[]}"#).is_err());
}

#[test]
fn request_bounds_and_required_identity_are_symmetric() {
    let valid = request();
    for mutate in [
        |v: &mut DeveloperLaunchRequest| v.version = 2,
        |v: &mut DeveloperLaunchRequest| v.request = 0,
        |v: &mut DeveloperLaunchRequest| v.recipe.clear(),
        |v: &mut DeveloperLaunchRequest| v.recipe = "../admin".into(),
        |v: &mut DeveloperLaunchRequest| v.recipe = "x".repeat(65),
        |v: &mut DeveloperLaunchRequest| v.executable = "relative".into(),
        |v: &mut DeveloperLaunchRequest| v.executable.push('\0'),
        |v: &mut DeveloperLaunchRequest| v.arguments = vec![String::new(); 129],
        |v: &mut DeveloperLaunchRequest| v.arguments = vec!["x".repeat(4097)],
        |v: &mut DeveloperLaunchRequest| v.arguments = vec!["\0".into()],
    ] {
        let mut v = valid.clone();
        mutate(&mut v);
        assert!(encode_developer_launch_request(&v).is_err());
        assert!(decode_developer_launch_request(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    let mut v = valid;
    v.recipe = "x".repeat(64);
    v.arguments = vec![String::new(); 128];
    assert!(encode_developer_launch_request(&v).is_ok());
    v.arguments = vec!["x".repeat(4096)];
    assert!(encode_developer_launch_request(&v).is_ok());
    v.arguments = vec!["x".repeat(4096); 8];
    assert!(encode_developer_launch_request(&v).is_err());
    assert!(
        decode_developer_launch_request(&vec![b' '; DEVELOPER_LAUNCH_MAX_REQUEST + 1]).is_err()
    );
}

#[test]
fn results_distinguish_process_readiness_from_refusal_and_uncertainty() {
    for status in [
        DeveloperLaunchStatus::Started,
        DeveloperLaunchStatus::Denied,
        DeveloperLaunchStatus::Unavailable,
        DeveloperLaunchStatus::Failed,
        DeveloperLaunchStatus::Indeterminate,
    ] {
        let value = DeveloperLaunchResult {
            version: 1,
            request: 17,
            status,
            process: (status == DeveloperLaunchStatus::Started).then_some(123),
        };
        let bytes = encode_developer_launch_result(&value).unwrap();
        assert_eq!(decode_developer_launch_result(&bytes).unwrap(), value);
        let mut missing: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        missing.as_object_mut().unwrap().remove("process");
        assert!(decode_developer_launch_result(&serde_json::to_vec(&missing).unwrap()).is_err());
        let mut invalid = value;
        invalid.process = if value.process.is_some() {
            None
        } else {
            Some(123)
        };
        assert!(encode_developer_launch_result(&invalid).is_err());
        assert!(decode_developer_launch_result(&serde_json::to_vec(&invalid).unwrap()).is_err());
        for end in 0..bytes.len() {
            assert!(decode_developer_launch_result(&bytes[..end]).is_err());
        }
    }
}

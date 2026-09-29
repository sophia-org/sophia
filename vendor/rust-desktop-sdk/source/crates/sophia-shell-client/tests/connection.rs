use sophia_shell_client::*;

// `connect_from_env`'s selection rule as pure logic: no process-global env
// mutation is needed (or, under this workspace's forbidden `unsafe_code`
// lint, possible: `std::env::set_var`/`remove_var` are `unsafe` since
// edition 2024).

#[test]
fn neither_or_both_env_wire_is_refused_outright() {
    assert!(matches!(
        select_env_wire(None, None),
        Err(ShellClientError::Environment(_))
    ));
    assert!(matches!(
        select_env_wire(Some("a".into()), Some("b".into())),
        Err(ShellClientError::Environment(_))
    ));
}

#[test]
fn retired_env_socket_is_refused_even_when_empty() {
    for socket in ["/tmp/shell.sock", ""] {
        assert!(matches!(
            select_env_wire(Some(socket.into()), None),
            Err(ShellClientError::Environment(_))
        ));
        assert!(matches!(
            select_env_wire(Some(socket.into()), Some("/tmp/files".into())),
            Err(ShellClientError::Environment(_))
        ));
    }
}

#[test]
fn env_files_alone_selects_the_file_wire() {
    let selection = select_env_wire(None, Some("/tmp/shell.9p".into())).unwrap();
    assert_eq!(
        selection,
        EnvWireSelection::Files {
            socket: "/tmp/shell.9p".into(),
        }
    );
}

// `api`'s one line, parsed strictly: exact key set and order, a version
// match and a nonzero epoch, nothing else.

fn api_line(line: &str) -> Vec<u8> {
    line.as_bytes().to_vec()
}

#[test]
fn api_line_good_shape_yields_the_epoch() {
    let epoch = parse_shell_files_api_line(&api_line(
        "sophia-shell-files version=1 role=bar epoch=7 fd_transfer=none\n",
    ))
    .unwrap();
    assert_eq!(epoch, 7);
}

#[test]
fn api_line_missing_key_is_refused() {
    // No role.
    assert!(
        parse_shell_files_api_line(&api_line(
            "sophia-shell-files version=1 epoch=7 fd_transfer=none\n"
        ))
        .is_err()
    );
    // No fd_transfer.
    assert!(
        parse_shell_files_api_line(&api_line("sophia-shell-files version=1 role=bar epoch=7\n"))
            .is_err()
    );
}

#[test]
fn api_line_extra_key_is_refused() {
    assert!(
        parse_shell_files_api_line(&api_line(
            "sophia-shell-files version=1 role=bar epoch=7 fd_transfer=none extra=1\n"
        ))
        .is_err()
    );
}

#[test]
fn api_line_reordered_keys_are_refused() {
    assert!(
        parse_shell_files_api_line(&api_line(
            "sophia-shell-files role=bar version=1 epoch=7 fd_transfer=none\n"
        ))
        .is_err()
    );
    assert!(
        parse_shell_files_api_line(&api_line(
            "sophia-shell-files version=1 epoch=7 role=bar fd_transfer=none\n"
        ))
        .is_err()
    );
}

#[test]
fn api_line_zero_epoch_is_refused() {
    assert!(
        parse_shell_files_api_line(&api_line(
            "sophia-shell-files version=1 role=bar epoch=0 fd_transfer=none\n"
        ))
        .is_err()
    );
}

#[test]
fn api_line_wrong_version_is_refused() {
    assert!(
        parse_shell_files_api_line(&api_line(
            "sophia-shell-files version=2 role=bar epoch=7 fd_transfer=none\n"
        ))
        .is_err()
    );
}

#[test]
fn api_line_oversize_is_refused() {
    let role = "x".repeat(SHELL_FILES_API_LINE_MAX_BYTES as usize);
    let line = format!("sophia-shell-files version=1 role={role} epoch=7 fd_transfer=none\n");
    assert!(parse_shell_files_api_line(line.as_bytes()).is_err());
}

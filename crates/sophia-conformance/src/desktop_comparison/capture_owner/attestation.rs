fn validate_session(
    run: &Path,
    scheduled: &ScheduledSample,
    attestation: &SessionAttestation,
) -> Result<(), String> {
    let candidate = source_commit(run)?;
    let expected_version = match scheduled.stack.as_str() {
        "sophia" => candidate.as_str(),
        "xlibre-xmonad" => XLIBRE_COMMIT,
        "niri" => NIRI_VERSION,
        _ => return Err("prepared schedule contains an unknown stack".to_owned()),
    };
    if attestation.stack != scheduled.stack
        || attestation.stack_version != expected_version
        || attestation.topology != TOPOLOGY
    {
        return Err(
            "active session attestation does not match the exact next schedule row".to_owned(),
        );
    }
    if attestation.native_timing != "not_exposed" {
        return Err(
            "local capture currently requires native_timing=not_exposed; kernel DRM remains authoritative"
                .to_owned(),
        );
    }
    validate_supervisor(attestation)
}

fn validate_active_profile(
    repo: &Path,
    run: &Path,
    attestation: &SessionAttestation,
) -> Result<(), String> {
    let expected = expected_profile(repo, &attestation.stack)?;
    match attestation.stack.as_str() {
        "sophia" => {
            let observed =
                process_environment(attestation.supervisor_pid, "SOPHIA_DESKTOP_PROFILE")
                    .ok_or("Sophia supervisor does not expose SOPHIA_DESKTOP_PROFILE")?;
            require_same_path(&observed, &expected, "Sophia desktop profile")?;
            require_descendant_executable(
                run,
                attestation.supervisor_pid,
                "hagia",
                "hagia_sha256",
            )?;
            require_descendant_executable(
                run,
                attestation.supervisor_pid,
                "narthex",
                "narthex_sha256",
            )
        }
        "niri" => {
            if let Some(observed) = process_environment(attestation.supervisor_pid, "NIRI_CONFIG") {
                return require_same_path(&observed, &expected, "niri profile");
            }
            let command = fs::read(format!("/proc/{}/cmdline", attestation.supervisor_pid))
                .map_err(|error| format!("could not inspect niri command line: {error}"))?;
            let expected = expected.to_string_lossy();
            if command
                .split(|byte| *byte == 0)
                .any(|argument| argument == expected.as_bytes())
            {
                Ok(())
            } else {
                Err("niri supervisor is not using the repository comparison profile".to_owned())
            }
        }
        "xlibre-xmonad" => stack_auxiliary_roots(repo, run, &attestation.stack).map(|_| ()),
        _ => Err("active session names an unknown comparison stack".to_owned()),
    }
}

fn require_descendant_executable(
    run: &Path,
    supervisor: u32,
    executable_name: &str,
    digest_field: &str,
) -> Result<(), String> {
    let expected = manifest_identity(run, digest_field)?;
    let mut observed = 0usize;
    for entry in fs::read_dir("/proc")
        .map_err(|error| format!("could not enumerate {executable_name} processes: {error}"))?
    {
        let entry = entry
            .map_err(|error| format!("could not inspect {executable_name} process: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(executable) = fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        if executable.file_name().and_then(|name| name.to_str()) != Some(executable_name)
            || !process_descends_from(pid, supervisor)?
        {
            continue;
        }
        require_executable_digest(&executable, &expected)?;
        observed = observed.saturating_add(1);
    }
    if observed == 1 {
        Ok(())
    } else {
        Err(format!(
            "comparison requires exactly one prepared {executable_name} descendant; found {observed}"
        ))
    }
}

fn required_stack_identities(
    repo: &Path,
    run: &Path,
    scheduled: &ScheduledSample,
    attestation: &SessionAttestation,
) -> Result<Vec<StackProcessIdentity>, String> {
    let mut identities = vec![stack_process_identity(
        "supervisor",
        attestation.supervisor_pid,
    )?];
    match scheduled.stack.as_str() {
        "sophia" => {
            identities.push(sole_descendant_identity(
                run,
                attestation.supervisor_pid,
                "hagia",
                "hagia_sha256",
            )?);
            identities.push(sole_descendant_identity(
                run,
                attestation.supervisor_pid,
                "narthex",
                "narthex_sha256",
            )?);
        }
        "xlibre-xmonad" => {
            let roots = stack_auxiliary_roots(repo, run, &scheduled.stack)?;
            identities.push(stack_process_identity("xmonad", roots[0])?);
        }
        "niri" => {}
        _ => return Err("prepared schedule contains an unknown stack".to_owned()),
    }
    Ok(identities)
}

fn sole_descendant_identity(
    run: &Path,
    supervisor: u32,
    executable_name: &'static str,
    digest_field: &str,
) -> Result<StackProcessIdentity, String> {
    let expected = manifest_identity(run, digest_field)?;
    let mut identities = Vec::new();
    for entry in fs::read_dir("/proc")
        .map_err(|error| format!("could not enumerate {executable_name} processes: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not inspect process: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(executable) = fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        if executable.file_name().and_then(|name| name.to_str()) == Some(executable_name)
            && process_descends_from(pid, supervisor)?
        {
            require_executable_digest(&executable, &expected)?;
            identities.push(stack_process_identity(executable_name, pid)?);
        }
    }
    if identities.len() != 1 {
        return Err(format!(
            "comparison requires exactly one prepared {executable_name} descendant; found {}",
            identities.len()
        ));
    }
    Ok(identities.remove(0))
}

fn stack_process_identity(label: &'static str, pid: u32) -> Result<StackProcessIdentity, String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|error| format!("could not read {label} process identity: {error}"))?;
    let executable = fs::read_link(format!("/proc/{pid}/exe"))
        .map_err(|error| format!("could not read {label} executable identity: {error}"))?;
    Ok(StackProcessIdentity {
        label,
        pid,
        start_ticks: parse_proc_stat(&stat)?.start_ticks,
        executable,
    })
}

fn validate_stack_identities(identities: &[StackProcessIdentity]) -> Result<(), String> {
    for expected in identities {
        let observed = stack_process_identity(expected.label, expected.pid)?;
        if observed.start_ticks != expected.start_ticks
            || observed.executable != expected.executable
        {
            return Err(format!(
                "comparison stack component {} changed identity during capture",
                expected.label
            ));
        }
    }
    Ok(())
}

fn expected_profile(repo: &Path, stack: &str) -> Result<PathBuf, String> {
    let relative = match stack {
        "sophia" => "validation/desktop-comparison/profiles/hagia.kdl",
        "xlibre-xmonad" => "validation/desktop-comparison/profiles/xmonad.hs",
        "niri" => "validation/desktop-comparison/profiles/niri.kdl",
        _ => return Err("unknown comparison stack profile".to_owned()),
    };
    fs::canonicalize(repo.join(relative))
        .map_err(|error| format!("could not resolve comparison profile {relative}: {error}"))
}

fn process_environment(pid: u32, name: &str) -> Option<PathBuf> {
    let environment = fs::read(format!("/proc/{pid}/environ")).ok()?;
    environment.split(|byte| *byte == 0).find_map(|entry| {
        let separator = entry.iter().position(|byte| *byte == b'=')?;
        let (key, value) = entry.split_at(separator);
        (key == name.as_bytes())
            .then(|| PathBuf::from(String::from_utf8_lossy(&value[1..]).into_owned()))
    })
}

fn require_same_path(observed: &Path, expected: &Path, name: &str) -> Result<(), String> {
    let observed = fs::canonicalize(observed)
        .map_err(|error| format!("could not resolve observed {name}: {error}"))?;
    if observed == expected {
        Ok(())
    } else {
        Err(format!(
            "{name} mismatch: expected {}, observed {}",
            expected.display(),
            observed.display()
        ))
    }
}

fn stack_auxiliary_roots(repo: &Path, run: &Path, stack: &str) -> Result<Vec<u32>, String> {
    if stack != "xlibre-xmonad" {
        return Ok(Vec::new());
    }
    let uid = current_uid()?;
    let mut roots = Vec::new();
    for entry in
        fs::read_dir("/proc").map_err(|error| format!("could not enumerate xmonad: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not inspect xmonad process: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(executable) = fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        if executable.file_name().and_then(|name| name.to_str()) != Some("xmonad") {
            continue;
        }
        let Ok(status) = fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        let process_uid = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .and_then(|rest| rest.split_ascii_whitespace().next())
            .and_then(|value| value.parse::<u32>().ok());
        if process_uid == Some(uid) {
            roots.push(pid);
        }
    }
    if roots.len() != 1 {
        return Err(format!(
            "XLibre comparison requires exactly one owned xmonad process; found {}",
            roots.len()
        ));
    }
    let executable = fs::read_link(format!("/proc/{}/exe", roots[0]))
        .map_err(|error| format!("could not identify xmonad executable: {error}"))?;
    require_executable_digest(&executable, &manifest_identity(run, "xmonad_sha256")?)?;
    let prefix = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("xmonad executable does not live under a versioned prefix")?;
    let identity = prefix.join("share/sophia-desktop-comparison");
    let version = fs::read_to_string(identity.join("xmonad-version"))
        .map_err(|error| format!("xmonad version identity is missing: {error}"))?;
    if version.trim() != XMONAD_VERSION {
        return Err(format!(
            "xmonad version identity does not match {XMONAD_VERSION}"
        ));
    }
    let contrib = fs::read_to_string(identity.join("xmonad-contrib-version"))
        .map_err(|error| format!("xmonad-contrib version identity is missing: {error}"))?;
    if contrib.trim() != XMONAD_CONTRIB_VERSION {
        return Err(format!(
            "xmonad-contrib version identity does not match {XMONAD_CONTRIB_VERSION}"
        ));
    }
    let expected_profile = expected_profile(repo, stack)?;
    let expected_digest = format!(
        "{:x}",
        sha2::Sha256::digest(
            fs::read(&expected_profile)
                .map_err(|error| format!("could not hash xmonad profile: {error}"))?
        )
    );
    let observed_digest = fs::read_to_string(identity.join("xmonad-profile-sha256"))
        .map_err(|error| format!("xmonad profile identity is missing: {error}"))?;
    if observed_digest.trim() != expected_digest {
        return Err(
            "running xmonad was not built from the repository comparison profile".to_owned(),
        );
    }
    Ok(roots)
}

fn manifest_identity(run: &Path, name: &str) -> Result<String, String> {
    let source = fs::read_to_string(run.join("manifest.kdl"))
        .map_err(|error| format!("comparison manifest is missing: {error}"))?;
    let first = source
        .lines()
        .next()
        .ok_or("comparison manifest is empty")?;
    let value = record_fields(first)?
        .get(name)
        .cloned()
        .ok_or_else(|| format!("comparison manifest lacks {name}"))?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "comparison manifest {name} is not lowercase SHA-256"
        ));
    }
    Ok(value)
}

fn require_executable_digest(executable: &Path, expected: &str) -> Result<(), String> {
    let bytes = fs::read(executable).map_err(|error| {
        format!(
            "could not hash comparison executable {}: {error}",
            executable.display()
        )
    })?;
    let observed = format!("{:x}", sha2::Sha256::digest(bytes));
    if observed == expected {
        Ok(())
    } else {
        Err(format!(
            "comparison executable digest mismatch: {}",
            executable.display()
        ))
    }
}

fn validate_supervisor(attestation: &SessionAttestation) -> Result<(), String> {
    let source = fs::read_to_string(format!("/proc/{}/stat", attestation.supervisor_pid))
        .map_err(|error| format!("attested session supervisor is not alive: {error}"))?;
    let observed = parse_proc_stat(&source)?;
    if observed.start_ticks != attestation.supervisor_start_ticks {
        return Err("attested session supervisor PID was reused".to_owned());
    }
    Ok(())
}

fn supervisor_identity_is_live(attestation: &SessionAttestation) -> Result<bool, String> {
    match fs::read_to_string(format!("/proc/{}/stat", attestation.supervisor_pid)) {
        Ok(source) => {
            Ok(parse_proc_stat(&source)?.start_ticks == attestation.supervisor_start_ticks)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("could not inspect torn-down supervisor: {error}")),
    }
}

fn session_attestation_path() -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or("XDG_RUNTIME_DIR is unset; no active comparison session can be admitted")?;
    Ok(PathBuf::from(runtime)
        .join("sophia-desktop-comparison")
        .join("session.kdl"))
}

fn read_attestation(path: &Path) -> Result<SessionAttestation, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("comparison session attestation is missing: {error}"))?;
    if !metadata.is_file() || metadata.uid() != current_uid()? || metadata.mode() & 0o077 != 0 {
        return Err("comparison session attestation must be an owner-only regular file".to_owned());
    }
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read comparison session attestation: {error}"))?;
    let line = one_record(&source, SESSION_PREFIX)?;
    let fields = record_fields(line)?;
    let required = |name: &str| {
        fields
            .get(name)
            .cloned()
            .ok_or_else(|| format!("comparison session attestation lacks {name}"))
    };
    let number = |name: &str| {
        required(name)?
            .parse::<u64>()
            .map_err(|_| format!("comparison session {name} is not an integer"))
    };
    let native_timing = required("native_timing")?;
    let native_source = required("native_source")?;
    require_token("native_source", &native_source)?;
    Ok(SessionAttestation {
        stack: required("stack")?,
        stack_version: required("stack_version")?,
        topology: required("topology")?,
        supervisor_pid: u32::try_from(number("supervisor_pid")?)
            .map_err(|_| "comparison supervisor PID is too large")?,
        supervisor_start_ticks: number("supervisor_start_ticks")?,
        crtc: number("crtc")?,
        native_timing,
        native_source,
    })
}

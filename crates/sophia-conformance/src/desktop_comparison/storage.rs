fn verify_prepared_inputs(repo: &Path, run: &Path) -> Result<(), String> {
    verify_private_run_storage(run)?;
    let manifest = fs::read_to_string(run.join("manifest.kdl"))
        .map_err(|error| format!("comparison manifest is missing: {error}"))?;
    let acquisition = manifest.lines().next().unwrap_or_default();
    let expected_cursor_digest = format!(
        "cursor_sha256={}",
        sophia_engine::x11_core_left_ptr_cursor(1).digest()
    );
    if !acquisition.starts_with("desktop_comparison_manifest schema=4 status=prepared ")
        || !acquisition
            .split_ascii_whitespace()
            .any(|field| field == "acquisition=terminal_free_visible")
        || !acquisition
            .split_ascii_whitespace()
            .any(|field| field == "optional_soak=separate")
        || !acquisition
            .split_ascii_whitespace()
            .any(|field| field == "cursor_theme=sophia-x11-core")
        || !acquisition
            .split_ascii_whitespace()
            .any(|field| field == "cursor_size=16")
        || !acquisition
            .split_ascii_whitespace()
            .any(|field| field == "cursor_shape=left_ptr")
        || !acquisition
            .split_ascii_whitespace()
            .any(|field| field == expected_cursor_digest)
    {
        return Err(
            "comparison run predates the terminal-free visibility, pinned-cursor, and optional-soak contract"
                .to_owned(),
        );
    }
    let schedule_file = fs::read_to_string(run.join("schedule.kdl"))
        .map_err(|error| format!("comparison schedule is missing: {error}"))?;
    let expected_schedule = schedule_for_run(run)?
        .iter()
        .map(|item| format!(
            "desktop_comparison_schedule schema=2 order={} stack={} workload={} repetition={} backend=native\n",
            item.order, item.stack, item.workload, item.repetition
        ))
        .collect::<String>();
    if schedule_file != expected_schedule {
        return Err("comparison schedule differs from the typed matrix".to_owned());
    }
    for config in CONFIGS {
        let expected = format!(
            "desktop_comparison_input schema=2 path={config} sha256={}",
            digest_file(&repo.join(config))?
        );
        if !manifest.lines().any(|line| line == expected) {
            return Err(format!(
                "comparison input changed after preparation: {config}"
            ));
        }
    }
    Ok(())
}

pub(crate) fn source_commit(run: &Path) -> Result<String, String> {
    let manifest = fs::read_to_string(run.join("manifest.kdl"))
        .map_err(|error| format!("comparison manifest is missing: {error}"))?;
    let first = manifest
        .lines()
        .next()
        .ok_or("comparison manifest is empty")?;
    fields(first)?
        .get("source_commit")
        .map(|value| (*value).to_owned())
        .ok_or_else(|| "comparison manifest lacks source_commit".to_owned())
}

fn sample_paths(run: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let root = run.join("samples");
    for stack in fs::read_dir(&root).map_err(|error| format!("sample root is missing: {error}"))? {
        let stack = stack.map_err(|error| format!("could not read sample stack: {error}"))?;
        if !stack.path().is_dir() {
            return Err("sample root contains a non-directory entry".to_owned());
        }
        for sample in fs::read_dir(stack.path())
            .map_err(|error| format!("could not read sample directory: {error}"))?
        {
            let sample = sample.map_err(|error| format!("could not read sample entry: {error}"))?;
            if sample.path().extension().and_then(|value| value.to_str()) != Some("log") {
                return Err("sample directory contains a non-log entry".to_owned());
            }
            paths.push(sample.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn rewrite_checksums(run: &Path, extra: &[PathBuf]) -> Result<(), String> {
    let mut paths = vec![PathBuf::from("manifest.kdl"), PathBuf::from("schedule.kdl")];
    paths.extend_from_slice(extra);
    let mut output = String::new();
    for relative in paths {
        output.push_str(&format!(
            "{}  {}\n",
            digest_file(&run.join(&relative))?,
            relative.display()
        ));
    }
    write_private(&run.join("checksums.sha256"), output.as_bytes())
}

fn append_checksum(run: &Path, relative: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(run.join("checksums.sha256"))
        .map_err(|error| format!("could not open comparison checksums: {error}"))?;
    writeln!(
        file,
        "{}  {}",
        digest_file(&run.join(relative))?,
        relative.display()
    )
    .map_err(|error| format!("could not append comparison checksum: {error}"))
}

pub(crate) fn verify_checksums(run: &Path) -> Result<(), String> {
    let checksums = fs::read_to_string(run.join("checksums.sha256"))
        .map_err(|error| format!("comparison checksums are missing: {error}"))?;
    let mut paths = BTreeSet::new();
    for line in checksums.lines() {
        let (expected, path) = line
            .split_once("  ")
            .ok_or("comparison checksum line is malformed")?;
        if !paths.insert(path.to_owned()) {
            return Err(format!("duplicate comparison checksum for {path}"));
        }
        if digest_file(&run.join(path))? != expected {
            return Err(format!("comparison checksum mismatch: {path}"));
        }
    }
    let expected_count = sample_paths(run)?.len().saturating_add(2);
    if paths.len() != expected_count {
        return Err("comparison checksum set does not cover every raw artifact".to_owned());
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(OWNER_FILE_MODE)
        .open(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))
        .and_then(|()| file.write_all(bytes))
        .map_err(|error| format!("could not protect or write {}: {error}", path.display()))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(OWNER_FILE_MODE)
        .open(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))
        .and_then(|()| file.write_all(bytes))
        .map_err(|error| format!("could not protect or write {}: {error}", path.display()))
}

fn create_private_run_storage(run: &Path) -> Result<(), String> {
    create_private_directory(run, true, "comparison run")?;
    create_private_directory(&run.join("samples"), false, "comparison sample root")?;
    create_private_directory(&run.join("attempts"), false, "comparison attempt root")
}

fn create_private_directory(path: &Path, recursive: bool, name: &str) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive).mode(OWNER_DIRECTORY_MODE);
    builder
        .create(path)
        .map_err(|error| format!("could not create {name}: {error}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_DIRECTORY_MODE))
        .map_err(|error| format!("could not protect {name}: {error}"))
}

fn verify_private_run_storage(run: &Path) -> Result<(), String> {
    for path in [run.to_path_buf(), run.join("samples"), run.join("attempts")] {
        verify_private_path(&path, true, OWNER_DIRECTORY_MODE)?;
    }
    for path in [
        run.join("manifest.kdl"),
        run.join("schedule.kdl"),
        run.join("checksums.sha256"),
    ] {
        verify_private_path(&path, false, OWNER_FILE_MODE)?;
    }
    Ok(())
}

fn verify_private_path(path: &Path, directory: bool, expected_mode: u32) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "comparison storage metadata failed for {}: {error}",
            path.display()
        )
    })?;
    let expected_kind = if directory {
        "directory"
    } else {
        "regular file"
    };
    let right_kind = if directory {
        metadata.file_type().is_dir()
    } else {
        metadata.file_type().is_file()
    };
    if !right_kind || metadata.uid() != current_uid()? || metadata.mode() & 0o777 != expected_mode {
        return Err(format!(
            "comparison storage must be an owner-only {expected_kind} with mode {expected_mode:04o}: {}",
            path.display()
        ));
    }
    Ok(())
}

fn digest_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn require_token(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('=') {
        return Err(format!(
            "{name} identity must be one nonempty key-value-safe token"
        ));
    }
    Ok(())
}

fn require_clean_worktree(status: &str) -> Result<(), String> {
    if status.is_empty() {
        Ok(())
    } else {
        Err("desktop comparison requires a clean Sophia worktree".to_owned())
    }
}

fn current_uid() -> Result<u32, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|error| format!("could not read current process identity: {error}"))?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_ascii_whitespace().next())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "current process identity lacks a numeric UID".to_owned())
}

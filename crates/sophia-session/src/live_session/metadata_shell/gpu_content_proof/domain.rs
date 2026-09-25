//! Proof-only observation immediately before exec in the admitted namespace.

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::Path;

use super::super::gpu;

const INVENTORY_BOUND: usize = 64;
pub(super) const OBSERVATION_ENV: &str = "SOPHIA_GPU_PROOF_OBSERVATION_ID";

/// This replaces the probe with Lom: its PID, namespace and grant stay intact.
pub fn exec_client(client: &Path) -> Result<(), Box<dyn std::error::Error>> {
    super::validate_input(client, "shell client")?;
    let observation = std::env::var(OBSERVATION_ENV)?;
    if observation.len() != 32
        || !observation
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("GPU proof child lacks its bounded observation identity".into());
    }
    if std::env::var(gpu::GPU_MODE_ENV).as_deref() != Ok("direct") {
        return Err("GPU proof child requires the direct grant".into());
    }
    let epoch = number(gpu::GPU_GRANT_EPOCH_ENV)?;
    let major = u32::try_from(number(gpu::GPU_DEVICE_MAJOR_ENV)?)?;
    let minor = u32::try_from(number(gpu::GPU_DEVICE_MINOR_ENV)?)?;
    if epoch == 0 {
        return Err("GPU proof child requires a nonzero grant epoch".into());
    }
    let node = format!("/dev/dri/renderD{minor}");
    if std::env::var(gpu::GPU_RENDER_NODE_ENV).as_deref() != Ok(node.as_str()) {
        return Err("GPU proof child render path disagrees with grant".into());
    }
    inspect(Path::new("/"), &node, major, minor)?;
    for key in ["DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"] {
        if std::env::var_os(key).is_some() {
            return Err("GPU proof child inherited display credentials".into());
        }
    }
    inspect_fds(Path::new("/proc/self/fd"))?;
    crate::session_println!(
        "sophia_shell_gpu_domain schema=1 status=observed observation_id={} grant_epoch={} device_major={} device_minor={} dri_entries=1 device_inventory=bounded input_absent=true x11_socket_dir_absent=true user_runtime_dir_absent=true display_environment_absent=true inherited_devices=none inherited_sockets=none",
        observation,
        epoch,
        major,
        minor,
    );
    std::io::stdout().flush()?;
    Err(std::process::Command::new(client)
        .arg("--serve")
        .exec()
        .into())
}

fn number(key: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let raw = std::env::var(key)?;
    let value: u64 = raw.parse()?;
    if value.to_string() != raw {
        return Err("GPU proof identity is not canonical".into());
    }
    Ok(value)
}

fn inspect(root: &Path, node: &str, major: u32, minor: u32) -> std::io::Result<()> {
    let node = root.join(node.trim_start_matches('/'));
    let metadata = fs::symlink_metadata(&node)?;
    if !metadata.file_type().is_char_device()
        || rustix::fs::major(metadata.rdev()) != major
        || rustix::fs::minor(metadata.rdev()) != minor
    {
        return Err(refusal("GPU proof selected device identity changed"));
    }
    let entries = fs::read_dir(root.join("dev/dri"))?
        .take(2)
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() != 1 || entries[0].path() != node {
        return Err(refusal("GPU proof exposes extra DRM entries"));
    }
    for path in ["dev/input", "tmp/.X11-unix", "run/user"] {
        match fs::symlink_metadata(root.join(path)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => return Err(refusal("GPU proof exposes input or display paths")),
        }
    }
    let mut remaining = INVENTORY_BOUND;
    inspect_devices(&root.join("dev"), &node, &mut remaining)
}

fn inspect_devices(path: &Path, selected: &Path, remaining: &mut usize) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| refusal("GPU proof device inventory exceeded bound"))?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let kind = metadata.file_type();
        if kind.is_dir() {
            inspect_devices(&entry.path(), selected, remaining)?;
        } else if kind.is_block_device()
            || (kind.is_char_device()
                && entry.path() != selected
                && !generic_device(metadata.rdev()))
        {
            return Err(refusal("GPU proof exposes an unrelated device"));
        }
    }
    Ok(())
}

fn inspect_fds(path: &Path) -> std::io::Result<()> {
    for (index, entry) in fs::read_dir(path)?.enumerate() {
        if index >= INVENTORY_BOUND {
            return Err(refusal("GPU proof descriptor inventory exceeded bound"));
        }
        let metadata = fs::metadata(entry?.path())?;
        let kind = metadata.file_type();
        if kind.is_socket()
            || kind.is_block_device()
            || (kind.is_char_device()
                && !matches!(
                    (
                        rustix::fs::major(metadata.rdev()),
                        rustix::fs::minor(metadata.rdev())
                    ),
                    (1, 3 | 5 | 7 | 8 | 9)
                ))
        {
            return Err(refusal("GPU proof inherited a socket or unrelated device"));
        }
    }
    Ok(())
}

fn generic_device(device: u64) -> bool {
    matches!(
        (rustix::fs::major(device), rustix::fs::minor(device)),
        (1, 3 | 5 | 7 | 8 | 9) | (5, 0 | 2)
    )
}

fn refusal(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, message)
}

#[path = "../../../../tests/support/gpu_proof_domain.rs"]
mod tests;

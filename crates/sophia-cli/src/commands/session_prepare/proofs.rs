use super::{BTreeMap, Result, Write, bounded, discovery, enabled, env, required};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const FIREFOX_PREFS: &str = "user_pref(\"browser.tabs.remote.autostart\", false);\nuser_pref(\"browser.tabs.remote.autostart.2\", false);\nuser_pref(\"fission.autostart\", false);\nuser_pref(\"middlemouse.paste\", true);\nuser_pref(\"middlemouse.contentLoadURL\", false);\n";

struct ProofDirectory(Option<PathBuf>);
impl Drop for ProofDirectory {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    rustix::rand::getrandom(&mut bytes, rustix::rand::GetRandomFlags::empty())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub(super) fn private_state(options: &BTreeMap<String, String>) -> Result<PathBuf> {
    let path = PathBuf::from(required(options, "state-dir")?);
    let meta = fs::symlink_metadata(&path)?;
    if !path.is_absolute()
        || !meta.is_dir()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err("state-dir must be an absolute private owner directory".into());
    }
    Ok(path)
}

fn install_private(source: &Path, destination: &Path) -> Result<()> {
    let temp = destination.with_extension(nonce()?);
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&fs::read(source)?)?;
        fs::rename(&temp, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub(super) fn run(options: &BTreeMap<String, String>, extra: &[String]) -> Result<()> {
    let state = private_state(options)?;
    let root = Path::new(required(options, "root")?);
    let mut proof = ProofDirectory(None);
    if discovery::firefox(extra) {
        // The adapter has already excluded a live wrapper and installed its
        // cleanup trap. Reclaim only directories in this private session root.
        for entry in fs::read_dir(&state)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("firefox-m10.")
                && entry.file_type()?.is_dir()
            {
                fs::remove_dir_all(entry.path())?;
            }
        }
        let path = state.join(format!("firefox-m10.{}", nonce()?));
        DirBuilder::new().mode(0o700).create(&path)?;
        proof.0 = Some(path.clone());
        let profile = path.join("firefox-profile");
        DirBuilder::new().mode(0o700).create(&profile)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(profile.join("user.js"))?;
        file.write_all(FIREFOX_PREFS.as_bytes())?;
    }
    if required(options, "profile")? == "standalone" {
        if enabled("SOPHIA_ENABLE_DIRECT_SCANOUT")? {
            for (source, target) in [
                ("direct_scanout_core.kdl", "standalone-core.kdl"),
                ("direct_scanout_desktop.kdl", "standalone-desktop.kdl"),
            ] {
                install_private(
                    &root.join("tools/fixtures").join(source),
                    &state.join(target),
                )?;
            }
        }
        if env("SOPHIA_STANDALONE_WORKLOAD", "vkcube")? == "kitty" {
            let executable = required(options, "standalone")?;
            let width = discovery::positive("SOPHIA_STANDALONE_WIDTH", "2560")?;
            let height = discovery::positive("SOPHIA_STANDALONE_HEIGHT", "1440")?;
            let log = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
                .open(state.join("kitty-override-check.log"))?;
            log.set_permissions(fs::Permissions::from_mode(0o600))?;
            let mut command = Command::new(executable);
            command.args(["+runpy", "import sys\nfrom kitty.config import parse_config\nfor spec in sys.argv[1:]:\n    parse_config([spec])\n"])
                .args(["linux_display_server x11".into(), "background_opacity 1".into(), "remember_window_size no".into(), format!("initial_window_width {width}"), format!("initial_window_height {height}"), "confirm_os_window_close 0".into()])
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
            bounded::check(&mut command, "Kitty override parser")?;
        }
    }
    let path = proof
        .0
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let profile = if path.is_empty() {
        String::new()
    } else {
        format!("{path}/firefox-profile")
    };
    let mut output = std::io::stdout().lock();
    output.write_all(b"sophia_session_proofs schema=1 status=prepared\0")?;
    for value in [path, profile] {
        output.write_all(value.as_bytes())?;
        output.write_all(b"\0")?;
    }
    output.flush()?;
    // Ownership transfers to the adapter only after the complete record is written.
    proof.0 = None;
    Ok(())
}

//! Fail-closed intake of the Bemenu artifact that
//! `cargo xtask prepare-bemenu-artifact` produced (crates/xtask/src/bemenu_artifact.rs).
//!
//! This checks content-to-revision BINDING only: the recorded raw commit hashes
//! to the expected revision, carries a signature header and names the recorded
//! tree; the preparation recorded status G; the SDK pin equals Sophia's own
//! vendored manifest byte for byte; and a private copy of the binary hashes to
//! the required SHA-256 before it is ever executed. Signer AUTHORIZATION
//! (`git verify-commit`) happened at preparation; no keyring is consulted here.
//! That the binary was built from that tree is the preparation's record, not
//! something this gate re-derives.
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const KEYS: [&str; 9] = [
    "schema",
    "binary",
    "binary_sha256",
    "source_commit",
    "source_tree",
    "signature_status",
    "signer_fingerprint",
    "sdk_revision",
    "sdk_manifest_sha256",
];
const TOOL_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Artifact {
    pub binary: PathBuf,
    pub commit: String,
    pub sha256: String,
    pub signer: String,
    pub sdk: String,
}

fn required(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!(
            "bemenu live gate: {name} is required. Prepare an artifact with \
             `cargo xtask prepare-bemenu-artifact <source-repo> <signed-commit> <new-output-dir>` \
             and pass SOPHIA_BEMENU_ARTIFACT=<that dir>, SOPHIA_BEMENU_SHA256=<binary sha256> \
             and SOPHIA_BEMENU_COMMIT=<signed commit>"
        ),
    }
}

/// Verify everything, then return a private 0500 copy that is the only file
/// the gate executes.
pub fn load(repo: &Path, private: &Path) -> Artifact {
    let dir = PathBuf::from(required("SOPHIA_BEMENU_ARTIFACT"));
    let expected_sha = required("SOPHIA_BEMENU_SHA256");
    let expected_commit = required("SOPHIA_BEMENU_COMMIT");
    assert!(
        dir.is_absolute(),
        "bemenu live gate: SOPHIA_BEMENU_ARTIFACT must be absolute"
    );
    assert!(
        hex(&expected_sha, 64) && hex(&expected_commit, 40),
        "bemenu live gate: SOPHIA_BEMENU_SHA256/COMMIT must be lowercase hex"
    );
    let manifest = String::from_utf8(read(&dir.join("bemenu-artifact.manifest")))
        .expect("bemenu live gate: manifest is not UTF-8");
    let values = manifest
        .lines()
        .map(|line| {
            line.split_once('=')
                .unwrap_or_else(|| panic!("bemenu live gate: manifest line {line:?}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        values.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        KEYS,
        "bemenu live gate: manifest keys"
    );
    let get = |key: &str| values.iter().find(|(k, _)| *k == key).unwrap().1;
    assert_eq!(get("schema"), "1", "bemenu live gate: manifest schema");
    assert_eq!(
        get("binary"),
        "bemenu-sophia",
        "bemenu live gate: binary name"
    );
    let commit = get("source_commit");
    let tree = get("source_tree");
    let signer = get("signer_fingerprint");
    let sdk = get("sdk_revision");
    assert!(hex(tree, 40) && hex(sdk, 40) && hex(get("sdk_manifest_sha256"), 64));
    assert_eq!(commit, expected_commit, "bemenu live gate: commit mismatch");
    assert_eq!(
        get("binary_sha256"),
        expected_sha,
        "bemenu live gate: manifest binary_sha256 differs from SOPHIA_BEMENU_SHA256"
    );
    assert_eq!(
        get("signature_status"),
        "G",
        "bemenu live gate: preparation did not record a good signature"
    );
    assert!(
        !signer.is_empty() && signer.bytes().all(|b| b.is_ascii_hexdigit()),
        "bemenu live gate: signer fingerprint {signer:?}"
    );

    // Binding: the raw signed object is exactly the named revision and tree.
    let raw = read(&dir.join("source.commit"));
    assert_eq!(
        tool("git", &["hash-object", "--stdin", "-t", "commit"], &raw).trim(),
        commit,
        "bemenu live gate: source.commit does not hash to the revision"
    );
    let text = String::from_utf8(raw).expect("bemenu live gate: commit object is not UTF-8");
    assert_eq!(
        text.lines().next(),
        Some(format!("tree {tree}").as_str()),
        "bemenu live gate: commit object tree"
    );
    assert!(
        text.lines().any(|line| line.starts_with("gpgsig ")),
        "bemenu live gate: commit object carries no signature"
    );

    // SDK pin: the same audited C SDK manifest Sophia vendors.
    let own = read(&repo.join("vendor/c-desktop-sdk/manifest.json"));
    assert_eq!(
        sha256(&own),
        get("sdk_manifest_sha256"),
        "bemenu live gate: Bemenu SDK manifest differs from Sophia's vendor/c-desktop-sdk"
    );
    let own = String::from_utf8(own).unwrap();
    let revision = own
        .split_once("\"revision\": \"")
        .and_then(|(_, rest)| rest.get(..40))
        .expect("bemenu live gate: Sophia SDK manifest revision");
    assert_eq!(revision, sdk, "bemenu live gate: SDK revision pin");

    // The executed file is a private copy hashed after copying.
    let source = dir.join("bemenu-sophia");
    assert!(
        std::fs::symlink_metadata(&source).is_ok_and(|m| m.is_file()),
        "bemenu live gate: {} is not a regular file",
        source.display()
    );
    std::fs::create_dir(private).unwrap();
    std::fs::set_permissions(private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let binary = private.join("bemenu-sophia");
    std::fs::copy(&source, &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o500)).unwrap();
    let digest = sha256(&read(&binary));
    assert_eq!(
        digest, expected_sha,
        "bemenu live gate: binary SHA-256 mismatch before exec"
    );
    Artifact {
        binary,
        commit: commit.to_owned(),
        sha256: digest,
        signer: signer.to_owned(),
        sdk: sdk.to_owned(),
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    let out = tool("sha256sum", &["-"], bytes);
    let digest = out.split_whitespace().next().unwrap_or("");
    assert!(hex(digest, 64), "sha256sum output {out:?}");
    digest.to_owned()
}

/// A bounded helper run: stdin supplied, stdout capped, KILL at the deadline.
fn tool(program: &str, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("bemenu live gate: {program}: {e}"));
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(4096).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + TOOL_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("bemenu live gate: {program} exceeded {TOOL_TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    writer.join().unwrap();
    let out = reader.join().unwrap();
    assert!(status.success(), "bemenu live gate: {program}: {status}");
    String::from_utf8(out).unwrap()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("bemenu live gate: {}: {e}", path.display()))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

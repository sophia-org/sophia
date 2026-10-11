// Confined group sockets: prepared before the frontend binds them, confirmed
// after it has, and both before any application can be launched into one.

/// Prepare each configured group directory for its listener.
///
/// The directory is made or confirmed owner-only and checked to hold nothing
/// but its own socket name. A socket left there is probed: one that answers
/// is a live listener and refuses startup; one refused with ECONNREFUSED is a
/// stale socket and is the only thing ever unlinked. Anything else found at
/// the socket path -- not a socket, or a probe failing any other way -- also
/// refuses. The frontend's bind stays exclusive, so a socket appearing
/// between this probe and that bind fails the bind rather than being taken.
fn prepare_confined_group_sockets(
    layout: &socket_directories::SocketDirectoryLayout,
    groups: &[u32],
) -> Result<Vec<(u32, std::path::PathBuf)>, Box<dyn std::error::Error>> {
    let mut prepared = Vec::with_capacity(groups.len());
    for id in groups {
        let group = socket_directories::ClientGroup::Confined(*id);
        let path = layout.prepare_group(group)?;
        layout.verify_group(group)?;
        clear_stale_group_socket(&path)?;
        prepared.push((*id, path));
    }
    Ok(prepared)
}

fn clear_stale_group_socket(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::FileTypeExt as _;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect {}: {error}", path.display()).into()),
    };
    if !metadata.file_type().is_socket() {
        return Err(format!("{} is not a socket", path.display()).into());
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(format!("confined X socket {} is already active", path.display()).into()),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            std::fs::remove_file(path)?;
            Ok(())
        }
        Err(error) => {
            Err(format!("cannot probe confined X socket {}: {error}", path.display()).into())
        }
    }
}

/// Wait for every group socket the frontend binds after its primary one, then
/// confirm each directory holds exactly that socket, before any launch.
fn verify_confined_group_sockets(
    layout: &socket_directories::SocketDirectoryLayout,
    groups: &[(u32, std::path::PathBuf)],
    server: &mut Option<
        std::thread::JoinHandle<Result<(), sophia_x_authority::X11SetupSocketError>>,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::FileTypeExt as _;
    for (id, path) in groups {
        wait_for_x_server_socket(path, server)?;
        layout.verify_group(socket_directories::ClientGroup::Confined(*id))?;
        if !std::fs::symlink_metadata(path)?.file_type().is_socket() {
            return Err(format!("{} is not the frontend's socket", path.display()).into());
        }
    }
    Ok(())
}

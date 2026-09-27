struct PolicySessionDirectory {
    path: std::path::PathBuf,
}

impl PolicySessionDirectory {
    fn create(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        std::fs::create_dir(&path)?;
        if let Err(error) = std::fs::set_permissions(
            &path,
            std::fs::Permissions::from_mode(0o700),
        ) {
            let _ = std::fs::remove_dir(&path);
            return Err(error);
        }
        let checkpoint_directory = path.join("checkpoint");
        if let Err(error) = std::fs::create_dir(&checkpoint_directory).and_then(|()| {
            std::fs::set_permissions(
                &checkpoint_directory,
                std::fs::Permissions::from_mode(0o700),
            )
        }) {
            let _ = std::fs::remove_dir(&checkpoint_directory);
            let _ = std::fs::remove_dir(&path);
            return Err(error);
        }
        Ok(Self { path })
    }

    fn endpoint_path(&self) -> std::path::PathBuf {
        self.path.join("endpoint")
    }

    fn checkpoint_path(&self) -> std::path::PathBuf {
        self.checkpoint_directory()
            .join("sophia-wm-policy.checkpoint")
    }

    fn checkpoint_directory(&self) -> std::path::PathBuf {
        self.path.join("checkpoint")
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for PolicySessionDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.checkpoint_path());
        let _ = std::fs::remove_dir(self.checkpoint_directory());
        let _ = std::fs::remove_dir(&self.path);
    }
}

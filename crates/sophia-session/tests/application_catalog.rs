use sophia_config::ApplicationCatalogConfig;
use sophia_session::application_catalog::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let p = std::env::temp_dir().join(format!(
            "sophia-catalog-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn entry(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).unwrap();
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn environment() -> ApplicationCatalogEnvironment {
    ApplicationCatalogEnvironment {
        namespace_policy: Default::default(),
        search_path: vec!["/usr/bin".into(), "/bin".into()],
        locale: "sr_RS.UTF-8@latin".into(),
        current_desktop: vec!["Sophia".into()],
    }
}
fn config(sources: Vec<PathBuf>) -> ApplicationCatalogConfig {
    ApplicationCatalogConfig {
        name: "installed".into(),
        sources,
        applications: vec![],
        terminal: None,
        terminal_arguments: vec![],
    }
}
fn wait(worker: &mut ApplicationCatalogWorker) -> ApplicationCatalogWorkerResult {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(result) = worker.poll() {
            return result;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
#[test]
fn exec_is_argv_and_field_codes_never_become_shell_evaluation() {
    let args = desktop_exec_arguments(
        r#"/bin/echo "hello world" %c %% %i %F"#,
        "Title;$(touch nope)",
        Some("editor"),
        Path::new("/x.desktop"),
    )
    .unwrap();
    assert_eq!(
        args,
        vec![
            "/bin/echo",
            "hello world",
            "Title;$(touch nope)",
            "%",
            "--icon",
            "editor"
        ]
    );
    for bad in [
        "/bin/echo ; touch nope",
        "/bin/echo $(touch nope)",
        "/bin/echo %Z",
        "/bin/echo %F %u",
        r#"/bin/echo "%c""#,
        "FOO=x /bin/echo",
    ] {
        assert!(
            desktop_exec_arguments(bad, "name", None, Path::new("/x.desktop")).is_none(),
            "{bad}"
        );
    }
}
#[test]
fn priority_visibility_locale_and_unsupported_activation_are_explicit() {
    let user = Directory::new();
    let system = Directory::new();
    system.entry(
        "hidden.desktop",
        "[Desktop Entry]\nType=Application\nName=Masked\nExec=/bin/true\n",
    );
    user.entry(
        "hidden.desktop",
        "[Desktop Entry]\nType=Application\nHidden=true\n",
    );
    user.entry("localized.desktop","[Desktop Entry]\nType=Application\nName=Base\nName[sr_RS@latin]=Lokalno\nExec=/bin/true\nOnlyShowIn=Sophia;\n");
    user.entry(
        "dbus.desktop",
        "[Desktop Entry]\nType=Application\nName=Bus\nExec=/bin/true\nDBusActivatable=true\n",
    );
    user.entry(
        "terminal.desktop",
        "[Desktop Entry]\nType=Application\nName=Terminal app\nExec=/bin/true\nTerminal=true\n",
    );
    user.entry("missing.desktop","[Desktop Entry]\nType=Application\nName=Missing\nExec=/bin/true\nTryExec=missing-sophia-test-executable\n");
    let catalog = build_application_catalog(
        &config(vec![user.0.clone(), system.0.clone()]),
        &[],
        &environment(),
    )
    .unwrap();
    assert_eq!(catalog.entries.len(), 3);
    assert!(
        catalog
            .entries
            .iter()
            .any(|e| e.descriptor.label == "Lokalno" && e.descriptor.available)
    );
    assert_eq!(
        catalog
            .entries
            .iter()
            .filter(|e| !e.descriptor.available)
            .count(),
        2
    );
}
#[test]
fn worker_rejects_changed_entry_and_new_priority_mask_at_dispatch() {
    let user = Directory::new();
    let system = Directory::new();
    system.entry(
        "app.desktop",
        "[Desktop Entry]\nType=Application\nName=App\nExec=/bin/true\n",
    );
    let mut worker = ApplicationCatalogWorker::start(
        config(vec![user.0.clone(), system.0.clone()]),
        vec![],
        environment(),
    )
    .unwrap();
    assert!(worker.refresh(1));
    assert!(!worker.refresh(2));
    let ApplicationCatalogWorkerResult::Built(1, Ok(catalog)) = wait(&mut worker) else {
        panic!("build failed");
    };
    let entry = catalog.entries[0].clone();
    assert!(worker.verify(2, entry.clone()));
    assert!(matches!(
        wait(&mut worker),
        ApplicationCatalogWorkerResult::Verified(2, Ok(_))
    ));
    user.entry(
        "app.desktop",
        "[Desktop Entry]\nType=Application\nHidden=true\n",
    );
    assert!(worker.verify(3, entry.clone()));
    assert!(matches!(
        wait(&mut worker),
        ApplicationCatalogWorkerResult::Verified(3, Err(_))
    ));
    std::fs::remove_file(user.0.join("app.desktop")).unwrap();
    system.entry(
        "app.desktop",
        "[Desktop Entry]\nType=Application\nName=App\nExec=/bin/false\n",
    );
    assert!(revalidate_catalog_entry(&entry).is_err());
}
#[test]
fn terminal_adapter_is_explicit_and_sources_cannot_escape_via_symlinks_or_fifos() {
    let source = Directory::new();
    let other = Directory::new();
    other.entry(
        "outside.desktop",
        "[Desktop Entry]\nType=Application\nName=Outside\nExec=/bin/true\n",
    );
    std::os::unix::fs::symlink(
        other.0.join("outside.desktop"),
        source.0.join("escape.desktop"),
    )
    .unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        source.0.join("fifo.desktop"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    source.entry(
        "terminal.desktop",
        "[Desktop Entry]\nType=Application\nName=Terminal\nExec=/bin/true\nTerminal=true\n",
    );
    let mut config = config(vec![source.0.clone()]);
    config.terminal = Some("terminal".into());
    config.terminal_arguments = vec!["--".into()];
    let registry = vec![RegisteredCatalogApplication {
        name: "terminal".into(),
        command: ApplicationLaunchCommand {
            executable: "/bin/echo".into(),
            arguments: vec!["prefix".into()],
            working_directory: None,
            namespace_recipe: None,
        },
    }];
    let catalog = build_application_catalog(&config, &registry, &environment()).unwrap();
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(
        catalog.entries[0].command.as_ref().unwrap().arguments,
        vec!["prefix", "--", "/bin/true"]
    );
}

#[test]
fn stable_identity_is_not_label_slot_or_locale_and_publication_is_atomic() {
    use sophia_protocol::*;
    let dir = Directory::new();
    dir.entry(
        "one.desktop",
        "[Desktop Entry]\nType=Application\nName=Same\nExec=/bin/true\n",
    );
    dir.entry(
        "two.desktop",
        "[Desktop Entry]\nType=Application\nName=Same\nExec=/bin/true\n",
    );
    let source =
        build_application_catalog(&config(vec![dir.0.clone()]), &[], &environment()).unwrap();
    assert_eq!(source.entries[0].identity, "desktop:one.desktop");
    assert_eq!(source.entries[1].identity, "desktop:two.desktop");
    assert_eq!(
        source.entries[0].descriptor.label,
        source.entries[1].descriptor.label
    );
    let published = PublishedApplicationCatalog::new(4, 5, source).unwrap();
    let tx = TransactionId::from_raw(9);
    use sophia_protocol::shell_files::*;
    let header = ShellFileHeader {
        kind: ShellFileKind::Catalog,
        connection_epoch: 4,
        submission_id: 0,
        sequence: 0,
    };
    let value = ShellFileCatalog {
        transaction: tx,
        catalog: published.value(true).unwrap(),
    };
    let bytes = encode_shell_file_catalog(header, &value).unwrap();
    let decoded = decode_shell_file_catalog(&bytes).unwrap();
    assert_eq!(decoded.transaction, tx);
    assert_eq!(&decoded.catalog.catalog, published.wire());
    assert_eq!(decoded.catalog.identities[&1], "desktop:one.desktop");
    assert_eq!(decoded.catalog.identities[&2], "desktop:two.desktop");
    assert_eq!(decoded, value);
    for end in 0..bytes.len() {
        assert!(
            decode_shell_file_catalog(&bytes[..end]).is_err(),
            "truncated at {end}"
        );
    }
    let plain = ShellFileCatalog {
        transaction: tx,
        catalog: published.value(false).unwrap(),
    };
    let decoded =
        decode_shell_file_catalog(&encode_shell_file_catalog(header, &plain).unwrap()).unwrap();
    assert_eq!(decoded.catalog.catalog, value.catalog.catalog);
    assert!(decoded.catalog.identities.is_empty());

    // File snapshots carry one whole identity bijection, without a separate
    // identity phase or per-entry epoch, generation and transaction headers.
    for identities in [
        std::collections::BTreeMap::from([(1, "desktop:one.desktop".into())]),
        std::collections::BTreeMap::from([
            (1, "desktop:one.desktop".into()),
            (2, "desktop:one.desktop".into()),
        ]),
        std::collections::BTreeMap::from([
            (1, "desktop:one.desktop".into()),
            (3, "desktop:two.desktop".into()),
        ]),
    ] {
        let mut bad = value.clone();
        bad.catalog.identities = identities;
        assert!(encode_shell_file_catalog(header, &bad).is_err());
    }
    let mut invalid_epoch = value.clone();
    invalid_epoch.catalog.catalog.connection_epoch = 0;
    assert!(encode_shell_file_catalog(header, &invalid_epoch).is_err());
    let mut no_transaction = value;
    no_transaction.transaction = TransactionId::from_raw(0);
    assert!(encode_shell_file_catalog(header, &no_transaction).is_err());
}

fn recipient_policy(identities: &[&str]) -> sophia_config::namespace_policy::NamespacePolicyConfig {
    use sophia_config::namespace_policy::*;
    NamespacePolicyConfig {
        recipes: std::collections::BTreeMap::from([(
            "private".into(),
            NamespaceRecipeConfig {
                name: "private".into(),
                profile: NamespaceRecipeProfile::ConfinedX { group: 7 },
                source: None,
                request: vec![],
                publish: vec![],
                developer_launch: false,
            },
        )]),
        applications: identities
            .iter()
            .map(|id| ((*id).into(), "private".into()))
            .collect(),
    }
}
#[test]
fn desktop_recipe_request_survives_verification_and_cannot_grant_policy() {
    let source = Directory::new();
    let body = "[Desktop Entry]\nType=Application\nName=Client\nExec=/bin/true\nX-Sophia-NamespaceRecipe=private\n";
    source.entry("client.desktop", body);
    let config = config(vec![source.0.clone()]);
    let mut env = environment();
    let unapproved = build_application_catalog(&config, &[], &env).unwrap();
    assert!(
        !unapproved.entries[0].descriptor.available,
        "metadata alone must not grant a recipe"
    );
    assert!(unapproved.entries[0].command.is_none());
    env.namespace_policy = recipient_policy(&["desktop:client.desktop"]);
    source.entry("other.desktop", body);
    let catalog = build_application_catalog(&config, &[], &env).unwrap();
    let entry = catalog
        .entries
        .iter()
        .find(|e| e.identity == "desktop:client.desktop")
        .unwrap();
    let command = revalidate_catalog_entry(entry).unwrap();
    assert_eq!(command.namespace_recipe.as_deref(), Some("private"));
    assert!(
        !catalog
            .entries
            .iter()
            .find(|e| e.identity == "desktop:other.desktop")
            .unwrap()
            .descriptor
            .available,
        "equal executable and metadata cannot borrow another identity's assignment"
    );
    source.entry("client.desktop", &body.replace("=private", "=shared"));
    assert!(
        revalidate_catalog_entry(entry).is_err(),
        "changing only recipe metadata must invalidate the retained command"
    );
}

#[test]
fn invalid_recipe_metadata_is_unavailable_and_terminal_wrapping_keeps_the_request() {
    let source = Directory::new();
    for invalid in [
        "".to_owned(),
        "Private".into(),
        "../private".into(),
        "x".repeat(65),
        "a\\sb".into(),
    ] {
        source.entry("client.desktop", &format!("[Desktop Entry]\nType=Application\nName=Client\nExec=/bin/true\nX-Sophia-NamespaceRecipe={invalid}\n"));
        let catalog =
            build_application_catalog(&config(vec![source.0.clone()]), &[], &environment())
                .unwrap();
        assert!(
            !catalog.entries[0].descriptor.available,
            "invalid request {invalid:?}"
        );
        assert!(catalog.entries[0].command.is_none());
    }
    source.entry("client.desktop", "[Desktop Entry]\nType=Application\nName=Client\nExec=/bin/true\nTerminal=true\nX-Sophia-NamespaceRecipe=private\n");
    let mut config = config(vec![source.0.clone()]);
    config.terminal = Some("terminal".into());
    let registry = [RegisteredCatalogApplication {
        name: "terminal".into(),
        command: ApplicationLaunchCommand {
            executable: "/bin/echo".into(),
            arguments: vec![],
            working_directory: None,
            namespace_recipe: None,
        },
    }];
    let mut env = environment();
    env.namespace_policy = recipient_policy(&["desktop:client.desktop"]);
    let catalog = build_application_catalog(&config, &registry, &env).unwrap();
    let command = revalidate_catalog_entry(&catalog.entries[0]).unwrap();
    assert_eq!(command.executable, Path::new("/bin/echo"));
    assert_eq!(
        command.namespace_recipe.as_deref(),
        Some("private"),
        "terminal wrapping cannot erase the application's request"
    );
    let result = spawn_catalog_process(
        &command,
        CatalogProcessEnvironment {
            display: ":unavailable",
            xauthority: Path::new("/nonexistent"),
            control_socket: None,
            inspection_socket: None,
            confinement: Default::default(),
        },
    );
    match result {
        Err(error) => assert!(error.to_string().contains("namespace launch owner")),
        Ok(mut child) => {
            let _ = child.wait();
            panic!("recipe metadata must not fall through to a host launch");
        }
    }
}

#[test]
fn assigned_recipes_are_automatic_and_worker_verification_keeps_exact_identity() {
    let source = Directory::new();
    source.entry(
        "client.desktop",
        "[Desktop Entry]\nType=Application\nName=Client\nExec=/bin/true\n",
    );
    let mut config = config(vec![source.0.clone()]);
    config.applications.push("client".into());
    let registered = vec![RegisteredCatalogApplication {
        name: "client".into(),
        command: ApplicationLaunchCommand {
            executable: "/bin/true".into(),
            arguments: vec![],
            working_directory: None,
            namespace_recipe: None,
        },
    }];
    let mut env = environment();
    env.namespace_policy = recipient_policy(&["registered:client", "desktop:client.desktop"]);
    let mut worker = ApplicationCatalogWorker::start(config, registered, env).unwrap();
    assert!(worker.refresh(1));
    let ApplicationCatalogWorkerResult::Built(1, Ok(catalog)) = wait(&mut worker) else {
        panic!("catalog publication");
    };
    assert_eq!(catalog.entries.len(), 2);
    for entry in &catalog.entries {
        assert!(entry.descriptor.available);
        assert_eq!(
            entry.command.as_ref().unwrap().namespace_recipe.as_deref(),
            Some("private"),
            "trusted assignment must select policy without application metadata"
        );
    }
    let mut forged = catalog.entries[0].clone();
    forged.identity = "registered:unassigned".into();
    assert!(worker.verify(2, forged));
    assert!(
        matches!(
            wait(&mut worker),
            ApplicationCatalogWorkerResult::Verified(2, Err(_))
        ),
        "worker must retain exact catalog identity"
    );
    assert!(worker.verify(3, catalog.entries[0].clone()));
    let ApplicationCatalogWorkerResult::Verified(3, Ok(command)) = wait(&mut worker) else {
        panic!("exact command must still verify");
    };
    assert_eq!(command.namespace_recipe.as_deref(), Some("private"));
    worker.request_shutdown();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !worker.poll_shutdown().unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

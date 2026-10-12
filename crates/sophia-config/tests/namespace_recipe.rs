use sophia_config::namespace_recipe::PinnedNamespaceRecipe;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-recipe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: &str) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pinned_source_survives_no_change_but_refuses_edits_and_inode_replacement() {
    let f = Fixture::new();
    f.write("main", "namespace-recipe 1\ninclude child");
    f.write("child", "mount inspection roles/inspection");
    let pinned = PinnedNamespaceRecipe::load(&f.0, "main").unwrap();
    assert_eq!(pinned.files().len(), 2);
    pinned.verify().unwrap();
    let second = PinnedNamespaceRecipe::load(&f.0, "main").unwrap();
    assert_eq!(pinned.digest(), second.digest());
    f.write("child", "mount inspection elsewhere");
    assert!(pinned.verify().is_err());
    let pinned = PinnedNamespaceRecipe::load(&f.0, "main").unwrap();
    f.write("replacement", pinned.files().get("child").unwrap());
    fs::rename(f.0.join("replacement"), f.0.join("child")).unwrap();
    assert!(pinned.verify().is_err());
}

#[test]
fn source_and_parent_symlinks_and_root_escape_are_refused() {
    let f = Fixture::new();
    let other = Fixture::new();
    other.write("secret", "mount inspection forbidden");
    f.write("main", "namespace-recipe 1\ninclude linked");
    symlink(other.0.join("secret"), f.0.join("linked")).unwrap();
    assert!(PinnedNamespaceRecipe::load(&f.0, "main").is_err());
    symlink(&other.0, f.0.join("dir")).unwrap();
    f.write("main", "namespace-recipe 1\ninclude dir/secret");
    assert!(PinnedNamespaceRecipe::load(&f.0, "main").is_err());
    assert!(PinnedNamespaceRecipe::load(&f.0.join("dir"), "secret").is_err());
    for path in ["../secret", "/secret"] {
        f.write("main", &format!("namespace-recipe 1\ninclude {path}"));
        assert!(PinnedNamespaceRecipe::load(&f.0, "main").is_err());
    }
}

#[test]
fn cycles_and_repeated_expansion_stop_before_publication() {
    let f = Fixture::new();
    f.write("main", "namespace-recipe 1\ninclude a");
    f.write("a", "include b");
    f.write("b", "include ./a");
    assert!(PinnedNamespaceRecipe::load(&f.0, "main").is_err());
    f.write("a", &"include b\n".repeat(100));
    f.write("b", &"# padding\n".repeat(100));
    assert!(PinnedNamespaceRecipe::load(&f.0, "main").is_err());
}

#[test]
fn root_replacement_and_missing_sources_are_refused() {
    let f = Fixture::new();
    f.write("main", "namespace-recipe 1\n");
    let pinned = PinnedNamespaceRecipe::load(&f.0, "main").unwrap();
    let old = f.0.with_extension("old");
    fs::rename(&f.0, &old).unwrap();
    fs::create_dir(&f.0).unwrap();
    f.write("main", pinned.source());
    assert!(pinned.verify().is_err());
    fs::remove_dir_all(old).unwrap();
    f.write("main", "namespace-recipe 1\ninclude absent");
    assert!(PinnedNamespaceRecipe::load(&f.0, "main").is_err());
}

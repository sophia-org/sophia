#[path = "../src/context.rs"]
mod context;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Notebook(PathBuf);

impl Notebook {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-context-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("todo.md"),
            "Do work id:t249 [details](note.md#current)\nOther id:t2490 [details](missing.md)\n",
        )
        .unwrap();
        std::fs::write(root.join("note.md"), "# Notebook\nIntro only.\n## Current\nExact candidate: abc123.\n```md\n## Not a section\n```\n### Limits\nPhysical acceptance remains open.\n## History\nOLD HISTORY\n## Current\nRepeated heading.\n").unwrap();
        Self(root)
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        context::run(
            &self.0,
            &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        )
        .map(|lines| lines.join("\n"))
    }
}

impl Drop for Notebook {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn task_retains_nested_limits_but_excludes_history_and_other_task() {
    let book = Notebook::new();
    let output = book.run(&["task", "t249"]).unwrap();
    assert!(output.contains("Exact candidate: abc123."));
    assert!(output.contains("Physical acceptance remains open."));
    assert!(output.contains("source: todo.md"));
    assert!(output.contains("source: note.md#current"));
    assert!(!output.contains("OLD HISTORY"));
    assert!(!output.contains("id:t2490"));
}

#[test]
fn missing_ambiguous_and_oversized_reads_fail_without_partial_context() {
    let book = Notebook::new();
    assert!(
        book.run(&["read", "note.md#missing"])
            .unwrap_err()
            .contains("not found")
    );
    assert!(
        book.run(&["read", "note.md#current", "--max-bytes=100"])
            .unwrap_err()
            .contains("smaller section")
    );
    std::fs::write(book.0.join("todo.md"), "One id:t249\nTwo id:t249\n").unwrap();
    assert!(book.run(&["task", "t249"]).unwrap_err().contains("found 2"));
    assert!(book.run(&["task", "t24"]).unwrap_err().contains("found 0"));
}

#[test]
fn outline_ignores_code_fences_and_resolves_duplicate_headings() {
    let book = Notebook::new();
    let output = book.run(&["outline", "note.md"]).unwrap();
    assert!(!output.contains("#not-a-section"));
    assert!(output.contains("#current-1"));
    assert!(
        book.run(&["read", "note.md#current-1"])
            .unwrap()
            .contains("Repeated heading.")
    );
    let intro = book.run(&["read", "note.md"]).unwrap();
    assert!(intro.contains("Intro only."));
    assert!(!intro.contains("Exact candidate"));
}

#[test]
fn source_hash_changes_with_content_and_symlinks_cannot_escape_checkout() {
    let book = Notebook::new();
    let before = book.run(&["read", "note.md"]).unwrap();
    std::fs::write(book.0.join("note.md"), "# Updated\n").unwrap();
    let after = book.run(&["read", "note.md"]).unwrap();
    assert_ne!(before.lines().nth(1), after.lines().nth(1));
    std::os::unix::fs::symlink("/etc", book.0.join("outside")).unwrap();
    assert!(
        book.run(&["read", "outside/passwd"])
            .unwrap_err()
            .contains("inside this checkout")
    );
}

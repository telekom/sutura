//! Actual classifier calls over real Git patch parsing; no patch is applied.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Categories, derive_from};

const ARTIFACT: &str = "devco/claim-mutations/a_catalog_rule.patch";
const DATAHUB: &str = "crates/sutura-catalog-datahub/src/lib.rs";
const BIGQUERY: &str = "crates/sutura-exec-bigquery/src/lib.rs";
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct Repo {
    root: PathBuf,
    base: String,
}

impl Repo {
    fn new(before: Option<&[u8]>) -> Self {
        let root = loop {
            let path = std::env::temp_dir().join(format!(
                "sutura-mutation-categories-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated classifier repo: {error}"),
            }
        };
        let mut repo = Self { root, base: String::new() };
        repo.git(&["init", "-q"]);
        repo.write("fixture", b"fixture\n");
        if let Some(patch) = before {
            repo.write(ARTIFACT, patch);
        }
        repo.git(&["add", "-A"]);
        repo.base = repo.git(&["write-tree"]).trim().to_owned();
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let mut command = Command::new("git");
        crate::repo::strip_git_env(&mut command);
        let output = command.current_dir(&self.root).args(args).output().expect("fixture git runs");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("fixture git output is UTF-8")
    }

    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        std::fs::write(path, bytes).expect("write fixture");
    }

    fn classify(&self, base: Option<&str>, paths: &[&str]) -> Categories {
        let paths = paths.iter().map(|path| (*path).to_owned()).collect::<Vec<_>>();
        let declared = BTreeSet::from(["catalog_datahub", "data_source_bigquery", "identity"].map(String::from));
        derive_from(&paths, Ok(declared), &self.root, base)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("remove classifier fixture: {error}");
        }
    }
}

fn patch(path: &str) -> String {
    format!("--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-before\n+after\n")
}

#[test]
fn a_new_adapter_mutation_and_its_derived_files_select_only_that_adapter() {
    let repo = Repo::new(None);
    let body = format!("diff --git a/{DATAHUB} b/{DATAHUB}\n{}", patch(DATAHUB));
    repo.write(ARTIFACT, body.as_bytes());
    let cats = repo.classify(Some(&repo.base), &[
        "crates/sutura-catalog-datahub/src/document.rs",
        DATAHUB,
        "crates/sutura-catalog-datahub/src/tests.rs",
        ARTIFACT,
        "docs/api/sutura-catalog-datahub.md",
    ]);
    assert!(!cats.core, "an adapter mutation is attributable: {cats:?}");
    assert_eq!(cats.selected, BTreeSet::from([String::from("catalog_datahub")]));
    assert!(!cats.needs("data_source_bigquery"));
    assert!(!cats.needs("identity"));
    assert_eq!(std::fs::read(repo.root.join(ARTIFACT)).unwrap(), body.as_bytes());
    assert!(!repo.root.join(DATAHUB).exists(), "classification must not apply the mutation");
}

#[test]
fn a_headerless_mutation_selects_its_adapter_without_applying() {
    let repo = Repo::new(None);
    repo.write(ARTIFACT, patch(DATAHUB).as_bytes());
    let cats = repo.classify(Some(&repo.base), &[ARTIFACT]);
    assert!(!cats.core, "Git accepts this headerless patch: {cats:?}");
    assert_eq!(cats.selected, BTreeSet::from([String::from("catalog_datahub")]));
    assert!(!repo.root.join(DATAHUB).exists());
}

#[test]
fn a_changed_mutation_selects_both_its_old_and_new_adapters() {
    let repo = Repo::new(Some(patch(DATAHUB).as_bytes()));
    repo.write(ARTIFACT, patch(BIGQUERY).as_bytes());
    let cats = repo.classify(Some(&repo.base), &[ARTIFACT]);
    assert!(!cats.core, "both revisions are attributable: {cats:?}");
    assert_eq!(cats.selected, BTreeSet::from(["catalog_datahub", "data_source_bigquery"].map(String::from)));
    assert!(!cats.needs("identity"));
}

#[test]
fn an_unreadable_mutation_revision_runs_every_category() {
    let repo = Repo::new(Some(patch(DATAHUB).as_bytes()));
    for base in [None, Some("refs/heads/absent")] {
        let cats = repo.classify(base, &[ARTIFACT]);
        assert!(cats.core, "an unknown base is not a proven addition: {cats:?}");
    }
    std::fs::remove_file(repo.root.join(ARTIFACT)).unwrap();
    let deleted = repo.classify(Some(&repo.base), &[ARTIFACT]);
    assert!(deleted.core, "deleted artifact has no readable head: {deleted:?}");

    let malformed_base = Repo::new(Some(b"not a patch\n"));
    malformed_base.write(ARTIFACT, patch(DATAHUB).as_bytes());
    let cats = malformed_base.classify(Some(&malformed_base.base), &[ARTIFACT]);
    assert!(cats.core, "a readable head cannot excuse an unreadable base patch: {cats:?}");

    let missing_blob = Repo::new(Some(patch(DATAHUB).as_bytes()));
    let object = missing_blob.git(&["rev-parse", &format!("{}:{ARTIFACT}", missing_blob.base)]);
    let (prefix, suffix) = object.trim().split_at(2);
    std::fs::remove_file(missing_blob.root.join(".git/objects").join(prefix).join(suffix)).unwrap();
    let cats = missing_blob.classify(Some(&missing_blob.base), &[ARTIFACT]);
    assert!(cats.core, "an existing unreadable blob is not an absent base path: {cats:?}");
}

#[test]
fn a_structural_or_noncanonical_mutation_runs_every_category() {
    let repo = Repo::new(None);
    let structural = [
        format!("diff --git a/{DATAHUB} b/{BIGQUERY}\nsimilarity index 100%\nrename from {DATAHUB}\nrename to {BIGQUERY}\n"),
        format!("diff --git a/{DATAHUB} b/{BIGQUERY}\nsimilarity index 100%\ncopy from {DATAHUB}\ncopy to {BIGQUERY}\n"),
        format!("diff --git a/{DATAHUB} b/{DATAHUB}\nold mode 100644\nnew mode 100755\n"),
        format!("diff --git a/{DATAHUB} b/{DATAHUB}\nnew file mode 100644\n--- /dev/null\n+++ b/{DATAHUB}\n@@ -0,0 +1 @@\n+new\n"),
        format!("diff --git a/{DATAHUB} b/{DATAHUB}\ndeleted file mode 100644\n--- a/{DATAHUB}\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n"),
        patch("crates/sutura-catalog-datahub/../sutura-app/src/lib.rs"),
        patch("../crates/sutura-catalog-datahub/src/lib.rs"),
        patch("/crates/sutura-catalog-datahub/src/lib.rs"),
        patch("crates/sutura-catalog-datahub/./src/lib.rs"),
        String::new(),
        String::from("not a patch\n"),
    ];
    for body in structural {
        repo.write(ARTIFACT, body.as_bytes());
        let cats = repo.classify(Some(&repo.base), &[ARTIFACT]);
        assert!(cats.core, "unsupported patch must retain core: {body:?}: {cats:?}");
    }
    repo.write(ARTIFACT, b"--- a/crates/sutura-catalog-datahub/\xff.rs\n+++ b/crates/sutura-catalog-datahub/\xff.rs\n@@ -1 +1 @@\n-a\n+b\n");
    let cats = repo.classify(Some(&repo.base), &[ARTIFACT]);
    assert!(cats.core, "invalid UTF-8 cannot be reinterpreted as a category: {cats:?}");
}

#[test]
fn a_mutation_targeting_shared_or_unknown_paths_runs_every_category() {
    let repo = Repo::new(None);
    for other in ["crates/sutura-domain/src/lib.rs", "unclassified", ARTIFACT] {
        let body = format!("{}{}", patch(DATAHUB), patch(other));
        repo.write(ARTIFACT, body.as_bytes());
        let cats = repo.classify(Some(&repo.base), &[ARTIFACT]);
        assert!(cats.core, "{other} keeps the mixed patch at core: {cats:?}");
        assert!(cats.needs("identity"));
        assert!(cats.needs("data_source_bigquery"));
    }
}

#[test]
fn a_lockfile_mutation_cannot_borrow_the_real_lock_diffs_adapter() {
    let mut repo = Repo::new(None);
    let before = "version = 4\n\n[[package]]\nname = \"sutura-catalog-datahub\"\nversion = \"0.1.0\"\ndependencies = [\n \"adapter-driver\",\n]\n\n[[package]]\nname = \"adapter-driver\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
    repo.write("Cargo.lock", before.as_bytes());
    repo.git(&["add", "-A"]);
    repo.base = repo.git(&["write-tree"]).trim().to_owned();
    repo.write("Cargo.lock", before.replace("1.0.0", "2.0.0").as_bytes());
    repo.write(ARTIFACT, patch("Cargo.lock").as_bytes());

    let control = repo.classify(Some(&repo.base), &["Cargo.lock"]);
    assert!(!control.core, "the actual lock comparison must succeed: {control:?}");
    assert_eq!(control.selected, BTreeSet::from([String::from("catalog_datahub")]));
    let mixed = repo.classify(Some(&repo.base), &["Cargo.lock", ARTIFACT]);
    assert!(mixed.core, "the artifact's target is a different lock comparison: {mixed:?}");
    assert!(mixed.needs("data_source_bigquery"));
}

//! Actual classifier calls over real Git patch parsing, and over the pages the real tree's tests read;
//! no patch is applied.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
            #[expect(
                clippy::create_dir,
                reason = "create_dir_all would succeed on a NAME COLLISION too, defeating the \
                          retry below that keeps advancing SEQUENCE past one; temp_dir() itself \
                          already exists"
            )]
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated classifier repo: {error}"),
            }
        };
        let mut repo = Self {
            root,
            base: String::new(),
        };
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
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
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
    let cats = repo.classify(
        Some(&repo.base),
        &[
            "crates/sutura-catalog-datahub/src/document.rs",
            DATAHUB,
            "crates/sutura-catalog-datahub/src/tests.rs",
            ARTIFACT,
        ],
    );
    assert!(!cats.core, "an adapter mutation is attributable: {cats:?}");
    assert_eq!(cats.selected, BTreeSet::from([String::from("catalog_datahub")]));
    assert!(!cats.needs("data_source_bigquery"));
    assert!(!cats.needs("identity"));
    assert_eq!(std::fs::read(repo.root.join(ARTIFACT)).unwrap(), body.as_bytes());
    assert!(
        !repo.root.join(DATAHUB).exists(),
        "classification must not apply the mutation"
    );
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
    assert_eq!(
        cats.selected,
        BTreeSet::from(["catalog_datahub", "data_source_bigquery"].map(String::from))
    );
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
        format!(
            "diff --git a/{DATAHUB} b/{DATAHUB}\nnew file mode 100644\n--- /dev/null\n+++ b/{DATAHUB}\n@@ -0,0 +1 @@\n+new\n"
        ),
        format!(
            "diff --git a/{DATAHUB} b/{DATAHUB}\ndeleted file mode 100644\n--- a/{DATAHUB}\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n"
        ),
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
    repo.write(
        ARTIFACT,
        b"--- a/crates/sutura-catalog-datahub/\xff.rs\n+++ b/crates/sutura-catalog-datahub/\xff.rs\n@@ -1 +1 @@\n-a\n+b\n",
    );
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

#[test]
fn a_symlinked_mutation_artifact_runs_every_category() {
    let repo = Repo::new(None);
    repo.write("elsewhere.patch", patch(DATAHUB).as_bytes());
    std::fs::create_dir_all(repo.root.join("devco/claim-mutations")).expect("the artifact directory");
    std::os::unix::fs::symlink(repo.root.join("elsewhere.patch"), repo.root.join(ARTIFACT)).expect("a symlinked artifact");
    let cats = repo.classify(Some(&repo.base), &[ARTIFACT]);
    assert!(cats.core, "a symlink is not a regular file, whatever it points at: {cats:?}");
}

/// The existing repository files ending `.md` that one source file's string literals name, each
/// with the line of its literal.
fn pages_named(root: &Path, canonical_root: &Path, rel: &str, text: &str) -> Vec<(String, usize)> {
    let own_crate: PathBuf = rel.split('/').take(2).collect();
    let mut pages = Vec::new();
    for literal in crate::serde_parse::scan::string_literals(text) {
        let body = literal.body.as_str();
        let is_page = Path::new(body).extension().is_some_and(|ext| ext == "md");
        if !is_page || !body.contains('/') || body.contains(char::is_whitespace) {
            continue;
        }
        let body = body.strip_prefix('/').unwrap_or(body);
        for base in [own_crate.as_path(), Path::new("")] {
            let resolved = root.join(base).join(body).canonicalize().ok().filter(|path| path.is_file());
            if let Some(page) = resolved.and_then(|path| crate::repo::relative(canonical_root, &path)) {
                pages.push((page, literal.line));
            }
        }
    }
    pages
}

/// A page a Rust test reads is that test's implementation, so a diff touching only the page must
/// still run the Rust tests: every page a literal under `crates/` names classifies into `rust`.
///
/// A literal counts when it ends `.md`, holds a `/` and no whitespace; one leading `/` is dropped
/// (the `concat!(env!("CARGO_MANIFEST_DIR"), "/../../…")` form), and it is resolved against its
/// own crate's directory and against the repository root. **The limit:** a path built at runtime
/// (a name joined onto a computed root), a literal with whitespace, a bare file name, and a page
/// read by anything other than a Rust literal resolve to nothing here and escape the census.
#[test]
fn every_page_a_crate_literal_names_classifies_as_rust() {
    let root = crate::repo::root().expect("the repository root");
    let canonical_root = root.canonicalize().expect("the repository root resolves");
    let mut named = BTreeSet::new();
    let mut escaped = Vec::new();
    let census = crate::repo::collect_files(&root, &root.join("crates"), &["rs"]);
    let inspected = census.inspect(
        &["crates/sutura-cli/tests/documented.rs"],
        |rel| rel.starts_with("crates/"),
        |rel, bytes| {
            for (page, line) in pages_named(&root, &canonical_root, rel, &String::from_utf8_lossy(bytes)) {
                if !crate::changes::classify(std::slice::from_ref(&page)).needs("rust") {
                    escaped.push(format!("{page}, named at {rel}:{line}"));
                }
                named.insert(page);
            }
        },
    );
    if let Err(refusal) = inspected {
        panic!("{}", refusal.describe());
    }
    assert!(
        named.contains("docs/getting-started.md") && named.contains("docs/serving-behind-a-gateway.md"),
        "the census resolves a root-relative and a crate-relative literal: {named:?}"
    );
    assert!(
        escaped.is_empty(),
        "a page a test reads classifies outside `rust`, so a diff to it alone runs no Rust test: {escaped:#?}"
    );
}

/// rust-oracledb's `guide` module compiles nineteen of its own pages into the crate, and no
/// `crates/` literal names them, so a non-member's page fails open rather than docs-only.
#[test]
fn a_non_member_page_is_never_docs_only() {
    for page in ["vendor/rust-oracledb/doc/arrow.md", "fuzz/README.md"] {
        let classified = crate::changes::classify(&[String::from(page)]);
        assert!(classified.docs_only.is_empty(), "{page} is not docs-only: {classified:?}");
        assert!(
            classified.run_all && classified.needs("rust"),
            "{page} runs everything: {classified:?}"
        );
    }
}

/// A docs-only diff selects no category leg; a page an area owns, or a non-member's page, is not
/// docs-only and still runs every category.
#[test]
fn a_docs_only_diff_selects_no_category() {
    let docs = super::tests::selected(&["README.md", "docs/adr/0001-x.md"]);
    assert!(!docs.core, "a docs-only diff is not core: {docs:?}");
    assert!(docs.selected.is_empty(), "a docs-only diff selects nothing: {docs:?}");
    for page in [
        "crates/sutura-cli/README.md",
        "docs/serving.md",
        "vendor/rust-oracledb/doc/arrow.md",
    ] {
        let cats = super::tests::selected(&["README.md", page]);
        assert!(cats.core, "{page} is read by a build or a test: {cats:?}");
    }
}

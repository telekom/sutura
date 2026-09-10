//! Which triples each venue's cross matrix builds, held here rather than by the three literals.
//!
//! Ordinary CI builds a REDUCED set - one cell per failure axis - on every event, and the FULL
//! four survive only where a release is built. That is three literals in three files that have to
//! agree, and until this module the only thing choosing was an inline `${{ A && B || C }}` on
//! `strategy.matrix.target` that no structural gate read. `check-workflows`, `check-shipped-binaries`
//! and `max-lines` all stayed green on a 4-on-PR regression, because they ask whether references
//! resolve, whether the shipped set agrees, and how long a file is - none of which is *which
//! triples a venue builds*.
//!
//! # The three sets, and why each is where it is
//!
//! | Venue | Matrix | Why |
//! | --- | --- | --- |
//! | `cross-link.yml`'s `link` | [`REDUCED`] | one cell per failure axis, on every event |
//! | `cachix-push.yml`'s `cross-build` | [`REDUCED`] | it fills the cache those legs substitute |
//! | `release.yml`'s `build` | [`FULL`] | full coverage is MOVED here, not removed |
//!
//! The third row is the one that makes the reduction safe, so it is the row a reader should check
//! first: `release.yml`'s `publish` asserts that exactly four artefacts arrived, so a link failure
//! unique to a dropped triple blocks a release rather than shipping a broken artefact. Delete that
//! matrix down to the reduced set and the reduction stops being *later signal* and becomes *no
//! signal* - which is why this module refuses it.
//!
//! # What this holds
//!
//! * **One set, on every ordinary-CI event.** An event-scoped `${{ }}` on `cross-link.yml`'s
//!   matrix is refused outright. A per-event set is exactly how the full four reached a pull
//!   request before, and with both events on the same list a ternary could only mislead.
//! * **Which triples, order-sensitively, in all three files.** A dropped cell, an added cell, a
//!   reordering or a swap reddens - so does the drift between `cross-link.yml` and
//!   `cachix-push.yml` that would publish a closure no leg reads, or build a leg nothing publishes.
//! * **That the full set still exists somewhere.** [`RELEASE`] is the anchor; without it every
//!   refusal here would pass on a tree that had quietly stopped cross-building anything.
//!
//! # What this does not hold
//!
//! * **Not whether Actions actually runs these cells.** Executing a matrix is runtime behaviour,
//!   not a string in this tree; this gate holds the literals that select the sets, and the header
//!   comments beside them stay honest only if they and this module stay one document.
//! * **Not the `ci.yml` caller's `cross` job condition** (`if:` / `on:`), which decides *when* the
//!   called workflow runs at all, nor whether the legs are required contexts (`contexts` owns
//!   that). A caller could drop the `cross` job entirely and these literals would stay green - the
//!   gate holds the matrices, not the invitation to them.
//! * **Not what a dropped triple's `feature-probes` step used to prove.** Those steps ride
//!   `cross-link.yml`'s matrix and nothing else runs them, so the reduction removes that coverage
//!   rather than deferring it. `docs/adr/0017` carries the correction; no gate can.

use std::path::Path;

/// Where the workflows live, read directly (not through [`super::sources`]) exactly as `sast` does.
const WORKFLOWS: &str = ".github/workflows";

/// The reduced set every ordinary-CI venue builds: file, job.
const LINK: (&str, &str) = ("cross-link.yml", "link");

/// The publisher that fills the cache those legs substitute: file, job.
const PUBLISH: (&str, &str) = ("cachix-push.yml", "cross-build");

/// The venue that still builds everything that ships: file, job.
const RELEASE: (&str, &str) = ("release.yml", "build");

/// One cell per failure axis, and the order is the canonical one.
///
/// `x86_64-unknown-linux-musl` isolates the static-allocator C risk on the host architecture;
/// `aarch64-unknown-linux-gnu` isolates the architecture the native `ci` job never compiles, on a
/// libc the host already exercises. `aarch64-unknown-linux-musl` would combine both axes, and
/// `x86_64-unknown-linux-gnu` is linked natively by `ci` on every run.
const REDUCED: &[&str] = &["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-musl"];

/// Every published triple, which only the release path builds now.
const FULL: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
];

/// Every way the three cross matrices can have stopped agreeing with the decision above.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for ((file, job), expected, role) in [
        (LINK, REDUCED, "the reduced ordinary-CI set"),
        (PUBLISH, REDUCED, "the reduced set whose closures it publishes"),
        (RELEASE, FULL, "the full published set"),
    ] {
        let path = root.join(WORKFLOWS).join(file);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                found.push(format!("{file} could not be read: {error}"));
                continue;
            }
        };
        match matrix_target(&text, job) {
            None => found.push(format!(
                "{file}: no `jobs.{job}.strategy.matrix.target` found - that venue's cross matrix is gone"
            )),
            Some(Target {
                line,
                inline: Some(value),
                ..
            }) => found.push(format!(
                "{file}:{line}: `target:` carries the expression `{value}`. An event-scoped matrix is \
                 refused here: every venue runs one fixed set now, so a `${{{{ }}}}` could only \
                 mislead - or bring back the per-event set that once put the full four on a pull \
                 request. Spell {role} as a plain list."
            )),
            Some(Target { line, items, .. }) if items != expected => found.push(format!(
                "{file}:{line}: `jobs.{job}.strategy.matrix.target` is not {role} - expected \
                 {expected:?} in that order, found {items:?}"
            )),
            Some(_) => {}
        }
    }
    found
}

/// One venue's `strategy.matrix.target`: where it is, and what it says.
struct Target {
    /// One-based, so a reader can open `cross-link.yml:130`.
    line: usize,
    /// The block-sequence items, in order. Empty when [`Target::inline`] is set.
    items: Vec<String>,
    /// A value written on the `target:` line itself - an expression, or a flow sequence.
    inline: Option<String>,
}

/// Locate `jobs.<job>.strategy.matrix.target` inside `text`.
///
/// Job-scoped rather than "the first `matrix:` in the file", because two of the three files hold
/// more than one job and a whole-file scan would read the wrong one - silently, and green. The job
/// header is the line `  <job>:` at the two-space column every job in this repository uses; the
/// span ends at the next line indented two spaces or fewer that is not blank and not a comment.
fn matrix_target(text: &str, job: &str) -> Option<Target> {
    let header = format!("  {job}:");
    let start = text.lines().position(|line| line.trim_end() == header)? + 1;
    let mut in_matrix = false;
    for (offset, line) in text.lines().enumerate().skip(start) {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // The next job, or the next top-level key: this job's span is over.
        if indent <= 2 {
            return None;
        }
        if trimmed == "matrix:" {
            in_matrix = true;
            continue;
        }
        if !in_matrix {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("target:") else {
            continue;
        };
        let rest = rest.trim();
        if !rest.is_empty() {
            return Some(Target {
                line: offset + 1,
                items: Vec::new(),
                inline: Some(rest.to_owned()),
            });
        }
        return Some(Target {
            line: offset + 1,
            items: items_after(text, offset),
            inline: None,
        });
    }
    None
}

/// The block-sequence items that follow the `target:` line at `offset`, in order.
///
/// Comment and blank lines inside the sequence are skipped rather than ending it, because this
/// repository writes a reason next to nearly every literal. Anything else - a sibling key, a
/// dedent - ends it.
fn items_after(text: &str, offset: usize) -> Vec<String> {
    let mut items = Vec::new();
    for line in text.lines().skip(offset + 1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(item) = trimmed.strip_prefix("- ") else {
            break;
        };
        items.push(item.trim().to_owned());
    }
    items
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// A synthetic root carrying all three matrices, so a test can break exactly one of them.
    ///
    /// Named per test rather than shared: the files are written into, and two tests sharing one
    /// root would pass or fail depending on which ran first.
    fn sound_root(tag: &str, link: &str, publish: &str, release: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-cross-{}-{tag}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(root.join(super::WORKFLOWS)).expect("temp workflows dir");
        for ((file, job), matrix) in [(super::LINK, link), (super::PUBLISH, publish), (super::RELEASE, release)] {
            std::fs::write(
                root.join(super::WORKFLOWS).join(file),
                format!("jobs:\n  {job}:\n    strategy:\n      matrix:\n        target:{matrix}\n"),
            )
            .expect("write a workflow");
        }
        root
    }

    /// A block sequence at the column the real files use.
    fn list(triples: &[&str]) -> String {
        triples.iter().fold(String::new(), |mut out, triple| {
            out.push_str("\n          - ");
            out.push_str(triple);
            out
        })
    }

    fn reduced() -> String {
        list(super::REDUCED)
    }

    fn full() -> String {
        list(super::FULL)
    }

    fn drop_root(root: &PathBuf) {
        std::fs::remove_dir_all(root).expect("the temp tree this test created is removable");
    }

    /// THE PRODUCTION ENTRY POINT, against the real tree. Every refusal below breaks one input to
    /// this same call, so `&& false` on any of them reddens one of the tests below rather than
    /// none - which is the difference between testing the predicate and testing the refusal.
    #[test]
    fn the_production_tree_holds_the_three_sets() {
        let root = crate::repo::root().expect("repo root");
        let found = super::problems(&root);
        assert!(found.is_empty(), "the production cross matrices have drifted: {found:?}");
    }

    /// The synthetic tree the refusals mutate is itself clean, or a refusal below could pass for
    /// the wrong reason.
    #[test]
    fn the_synthetic_tree_is_clean() {
        let root = sound_root("clean", &reduced(), &reduced(), &full());
        let found = super::problems(&root);
        assert!(found.is_empty(), "{found:?}");
        drop_root(&root);
    }

    /// The primary refusal: an event-scoped ternary back on the link matrix. That shape is how the
    /// full four reached a pull request before, and with one set per venue it can only mislead.
    #[test]
    fn an_event_scoped_expression_is_refused() {
        let target = " ${{ github.event_name == 'pull_request' && fromJSON('[\"aarch64-unknown-linux-gnu\"]') || fromJSON('[\"x86_64-unknown-linux-gnu\"]') }}";
        let root = sound_root("expression", target, &reduced(), &full());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("event-scoped matrix is refused"), "{found:?}");
        drop_root(&root);
    }

    /// The regression this reduction exists to prevent coming back: the full four in ordinary CI.
    #[test]
    fn the_full_set_in_ordinary_ci_is_refused() {
        let root = sound_root("full-in-ci", &full(), &reduced(), &full());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("cross-link.yml"), "{found:?}");
        drop_root(&root);
    }

    /// Both reduced cells are load-bearing, and each covers a different axis, so dropping either
    /// one leaves an axis unproven anywhere before a tag.
    #[test]
    fn dropping_a_reduced_cell_is_refused() {
        for (tag, kept) in [
            ("keep-arch", "aarch64-unknown-linux-gnu"),
            ("keep-libc", "x86_64-unknown-linux-musl"),
        ] {
            let root = sound_root(tag, &list(&[kept]), &reduced(), &full());
            let found = super::problems(&root);
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].contains("reduced ordinary-CI set"), "{found:?}");
            drop_root(&root);
        }
    }

    /// The set is pinned order-sensitively, so swapping the two cells is a change, not a no-op.
    #[test]
    fn a_reordered_reduced_set_is_refused() {
        let swapped = list(&["x86_64-unknown-linux-musl", "aarch64-unknown-linux-gnu"]);
        let root = sound_root("reordered", &swapped, &reduced(), &full());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("in that order"), "{found:?}");
        drop_root(&root);
    }

    /// A third cell in ordinary CI is the cost this reduction bought back, one triple at a time.
    #[test]
    fn a_third_ordinary_ci_cell_is_refused() {
        let widened = list(&[
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
        ]);
        let root = sound_root("third-cell", &widened, &reduced(), &full());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("reduced ordinary-CI set"), "{found:?}");
        drop_root(&root);
    }

    /// The publisher must fill exactly what the legs substitute. A triple published that no leg
    /// builds is store paths nothing reads; one built and not published is a leg with no carrier.
    #[test]
    fn a_publisher_that_has_drifted_is_refused() {
        let root = sound_root("publisher-drift", &reduced(), &full(), &full());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("cachix-push.yml"), "{found:?}");
        drop_root(&root);
    }

    /// THE ANCHOR. Reducing the release matrix too would turn *the signal arrives later* into *no
    /// signal arrives*, and every other refusal here would still pass.
    #[test]
    fn a_reduced_release_matrix_is_refused() {
        let root = sound_root("release-reduced", &reduced(), &reduced(), &reduced());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("release.yml"), "{found:?}");
        assert!(found[0].contains("full published set"), "{found:?}");
        drop_root(&root);
    }

    /// A matrix read out of the wrong job is the failure a whole-file scan would have: two of the
    /// three files hold several jobs, and an earlier one carries its own matrix.
    #[test]
    fn a_matrix_in_another_job_is_not_read() {
        let root = sound_root("wrong-job", &reduced(), &reduced(), &full());
        let path = root.join(super::WORKFLOWS).join(super::PUBLISH.0);
        let text = std::fs::read_to_string(&path).expect("read the publisher");
        let decoy = format!(
            "jobs:\n  push:\n    strategy:\n      matrix:\n        target:{}\n{}",
            full(),
            text.trim_start_matches("jobs:\n")
        );
        std::fs::write(&path, decoy).expect("write the decoy");
        let found = super::problems(&root);
        assert!(found.is_empty(), "the decoy job's matrix was read instead: {found:?}");
        drop_root(&root);
    }

    /// A job whose matrix is gone entirely reads as clean to any rule written as a refusal, which
    /// is why absence is its own failure.
    #[test]
    fn a_missing_matrix_is_refused() {
        let root = sound_root("missing", &reduced(), &reduced(), &full());
        let path = root.join(super::WORKFLOWS).join(super::LINK.0);
        std::fs::write(&path, "jobs:\n  link:\n    strategy:\n      fail-fast: false\n")
            .expect("write a link job with no matrix");
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("cross matrix is gone"), "{found:?}");
        drop_root(&root);
    }
}

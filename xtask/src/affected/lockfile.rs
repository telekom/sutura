//! Attributing a `Cargo.lock` diff to adapter categories through the reverse dependency closure.
//!
//! Every changed `[[package]]` is walked in reverse through **external** packages and **stops** at
//! the first workspace member reached, never through it - the same rule a path change follows,
//! where `crates/sutura-exec-duckdb/` selects `data_source_duckdb` although `sutura-app` depends on
//! it. A workspace member is a package with no `source` line (a path dependency), which covers
//! every `crates/` member plus the non-`crates` members (`sutura-dev`, `xtask`); the members reached
//! map to categories through [`super::category_from_crate`], the one mapping the path selection
//! also uses.
//!
//! **Fails closed to `core`** on anything it cannot attribute: a lock with no `[[package]]` block
//! or a `[[package]]` with a `name` but no `version`, a `[patch]` section, a dependency string that
//! names no package, a changed `source` or `checksum`, a reached member on neither adapter axis (a
//! shared crate, `sutura-dev`, `xtask`), a root, or a diff with no package change at all (lock
//! version, formatting). It can only narrow.
//!
//! **Limit.** A lock records no feature flags: a `[features]` edit that resolves to the same
//! versions leaves `Cargo.lock` unchanged and is invisible here. The line scan mirrors
//! `changes::package_name` rather than adding a TOML dependency for one format.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// The base and head `Cargo.lock` text, so the parent's `select` can attribute a `Cargo.lock` diff
/// to adapter categories via the reverse dependency closure. Constructed by the parent's
/// `derive_from` from a git base ref; injected by tests.
pub(super) struct Locks {
    pub(super) base: String,
    pub(super) head: String,
}

/// The category attribution of a `Cargo.lock` diff, or a refusal to attribute.
pub(super) enum Attribution {
    /// The categories the diff's changed packages reach through their reverse closure.
    Selected(BTreeSet<String>),
    /// The diff could not be attributed - `core` must run everything. Carries the reason.
    Core(String),
}

/// A package's `(name, version)`, the key `Cargo.lock` itself disambiguates by.
type Key = (String, String);

/// Each package, and the packages that list it as a dependency.
type Reverse = BTreeMap<Key, BTreeSet<Key>>;

/// One parsed `[[package]]` block: its raw dependency strings, optional `source`, and optional
/// `checksum`. A workspace member has no `source`; a `checksum` change is a content change.
#[derive(PartialEq)]
struct Package {
    deps: Vec<String>,
    source: Option<String>,
    checksum: Option<String>,
}

/// The parsed lockfile graph, keyed by `(name, version)`.
struct Graph {
    packages: BTreeMap<Key, Package>,
    /// `name → [version]` for resolving unsuffixed dep strings.
    by_name: BTreeMap<String, Vec<String>>,
}

impl Graph {
    /// Resolve a raw dependency string (`"name"` or `"name version"`) to a `(name, version)` key
    /// in this graph, if it exists.
    fn resolve(&self, dep: &str) -> Option<Key> {
        // `"name version"`: names never contain a space, and a version starts with a digit.
        if let Some((name, ver)) = dep.rsplit_once(' ')
            && ver.as_bytes().first().is_some_and(u8::is_ascii_digit)
        {
            let key = (name.to_owned(), ver.to_owned());
            return self.packages.contains_key(&key).then_some(key);
        }
        // An unsuffixed dep: `"sutura-domain"`. Resolves to the unique package with that name.
        match self.by_name.get(dep)?.as_slice() {
            [only] => Some((dep.to_owned(), only.clone())),
            _ => None,
        }
    }
}

/// Parse a `Cargo.lock` into a [`Graph`]. Returns `None` (fail-closed) on a `[patch]` section, a
/// lock with no `[[package]]` block, or a `[[package]]` with a `name` but no `version`.
fn parse(lock: &str) -> Option<Graph> {
    let mut packages: BTreeMap<Key, Package> = BTreeMap::new();
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut source: Option<String> = None;
    let mut checksum: Option<String> = None;
    let mut deps: Vec<String> = Vec::new();
    let mut in_deps = false;
    let mut malformed = false;

    for line in lock.lines() {
        let trimmed = line.trim();

        // Any patch-like section is a fail-closed trigger: patched dependencies bypass the
        // registry, so a lock diff cannot be attributed by version alone. `contains` catches
        // both `[patch]`/`[patch.crates-io]` (single-bracket) and `[[patch.unused]]` (double).
        if trimmed.contains("[patch") {
            return None;
        }

        // A `[[package]]` or any other `[section]` ends the current package and the deps block.
        if trimmed.starts_with('[') {
            malformed |= flush(
                &mut packages,
                &mut by_name,
                &mut name,
                &mut version,
                &mut source,
                &mut checksum,
                &mut deps,
            );
            in_deps = false;
            continue;
        }

        if in_deps {
            if trimmed == "]" {
                in_deps = false;
            } else if let Some(rest) = trimmed.strip_prefix('"')
                && let Some(value) = rest.strip_suffix("\",")
            {
                deps.push(value.to_owned());
            } else if let Some(rest) = trimmed.strip_prefix('"')
                && let Some(value) = rest.strip_suffix('"')
            {
                deps.push(value.to_owned());
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("name = ") {
            name = Some(String::from(rest.trim_matches('"')));
        } else if let Some(rest) = trimmed.strip_prefix("version = ") {
            version = Some(String::from(rest.trim_matches('"')));
        } else if let Some(rest) = trimmed.strip_prefix("source = ") {
            source = Some(String::from(rest.trim_matches('"')));
        } else if let Some(rest) = trimmed.strip_prefix("checksum = ") {
            checksum = Some(String::from(rest.trim_matches('"')));
        } else if trimmed == "dependencies = [" {
            in_deps = true;
        }
    }
    malformed |= flush(
        &mut packages,
        &mut by_name,
        &mut name,
        &mut version,
        &mut source,
        &mut checksum,
        &mut deps,
    );

    // A lock with no `[[package]]` block, or a `[[package]]` with no `version`, is not a
    // `Cargo.lock` the walk can attribute - fail closed rather than treating it as empty.
    if malformed || packages.is_empty() {
        return None;
    }
    Some(Graph { packages, by_name })
}

/// Push the current package (if complete) into the maps and reset the accumulators. Returns
/// `true` if a `[[package]]` with a `name` but no `version` was seen (a malformed lock).
fn flush(
    packages: &mut BTreeMap<Key, Package>,
    by_name: &mut BTreeMap<String, Vec<String>>,
    name: &mut Option<String>,
    version: &mut Option<String>,
    source: &mut Option<String>,
    checksum: &mut Option<String>,
    deps: &mut Vec<String>,
) -> bool {
    let Some(n) = name.take() else {
        // No package was being accumulated, but the top-level `version = 4` line or a stale
        // section may have left state. Reset everything so the next `[[package]]` starts clean.
        version.take();
        source.take();
        checksum.take();
        deps.clear();
        return false;
    };
    let Some(v) = version.take() else {
        // A `[[package]]` with a `name` but no `version` is malformed - it cannot be keyed, and
        // inheriting a stale version (e.g. the top-level `version = 4`) would invent a package.
        source.take();
        checksum.take();
        deps.clear();
        return true;
    };
    let key = (n.clone(), v.clone());
    by_name.entry(n).or_default().push(v);
    packages.insert(
        key,
        Package {
            deps: std::mem::take(deps),
            source: source.take(),
            checksum: checksum.take(),
        },
    );
    false
}

/// A changed package and which graph to walk its reverse closure in.
enum Changed {
    /// Added or deps-changed: present in head, walk the head reverse closure.
    Head(Key),
    /// Removed: absent from head, walk the base reverse closure.
    Base(Key),
}

/// Attribute a `Cargo.lock` diff to adapter categories.
///
/// Each changed package is walked in reverse through external packages until it reaches a
/// workspace member - a package with no `source` line (a path dependency), which covers every
/// `crates/` member plus the non-`crates` members (`sutura-dev`, `xtask`). Each reachable member
/// is mapped to a category through [`super::category_from_crate`]; a member on neither axis is
/// **shared** and forces `Core`.
pub(super) fn attribute(base: &str, head: &str) -> Attribution {
    let Some(base_graph) = parse(base) else {
        return Attribution::Core(String::from(
            "Cargo.lock: base lockfile has a [patch] section or could not be parsed - running every category",
        ));
    };
    let Some(head_graph) = parse(head) else {
        return Attribution::Core(String::from(
            "Cargo.lock: head lockfile has a [patch] section or could not be parsed - running every category",
        ));
    };

    // A package in both locks with a changed `source` (the same version resolved from another
    // place) or `checksum` (the same version republished with other content) is a fail-closed
    // trigger even beside a change that narrows: version-based attribution no longer holds.
    for (key, head_pkg) in &head_graph.packages {
        let Some(base_pkg) = base_graph.packages.get(key) else {
            continue;
        };
        let field = if base_pkg.source != head_pkg.source {
            "source"
        } else if base_pkg.checksum != head_pkg.checksum {
            "checksum"
        } else {
            continue;
        };
        return Attribution::Core(format!(
            "Cargo.lock: `{}` {} changed {field} - running every category",
            key.0, key.1
        ));
    }
    // The walk stops at workspace members: packages with no `source` (path dependencies), which
    // covers every `crates/` member plus the non-`crates` members (`sutura-dev`, `xtask`). A
    // boundary is taken from the same graph the walk uses, so a removed package stops at base's
    // members and an added one at head's.
    let head_boundaries = sourceless_names(&head_graph);
    let base_boundaries = sourceless_names(&base_graph);

    let changed = diff_packages(&base_graph, &head_graph);
    if changed.is_empty() {
        // The lockfile is in the diff but no package was added, removed, or changed: a lock-version
        // line or a formatting change. Nothing to attribute, so fail closed.
        return Attribution::Core(String::from(
            "Cargo.lock: diff has no package changes (version/format-only) - running every category",
        ));
    }

    let (head_reverse, base_reverse) = match (reverse_closure(&head_graph), reverse_closure(&base_graph)) {
        (Ok(head), Ok(base)) => (head, base),
        (Err(dep), _) | (_, Err(dep)) => {
            return Attribution::Core(format!(
                "Cargo.lock: dependency `{dep}` names no package - running every category"
            ));
        }
    };

    let mut categories = BTreeSet::new();
    for change in &changed {
        let (key, reverse, boundaries) = match change {
            Changed::Head(k) => (k, &head_reverse, &head_boundaries),
            Changed::Base(k) => (k, &base_reverse, &base_boundaries),
        };
        let reached = bfs_to_members(key, reverse, boundaries);
        if reached.is_empty() {
            return Attribution::Core(format!(
                "Cargo.lock: changed package `{}` {} reaches no workspace member - running every category",
                key.0, key.1
            ));
        }
        for name in &reached {
            match super::category_from_crate(name) {
                Some(category) => {
                    categories.insert(category);
                }
                None => {
                    // A shared (non-adapter) workspace member is the immediate boundary for
                    // this changed package. A shared-crate dep bump can break any adapter, so
                    // this fails closed rather than narrowing.
                    return Attribution::Core(format!(
                        "Cargo.lock: changed package `{}` {} reaches shared crate `{name}` - running every category",
                        key.0, key.1
                    ));
                }
            }
        }
    }

    Attribution::Selected(categories)
}

/// The changed packages between base and head, tagged with which graph to walk.
///
/// A package is changed if it is added (in head only), removed (in base only), or differs in any
/// field (in both). A changed `source` or `checksum` is refused earlier in [`attribute`].
fn diff_packages(base: &Graph, head: &Graph) -> Vec<Changed> {
    let mut changed = Vec::new();
    for (key, pkg) in &head.packages {
        match base.packages.get(key) {
            None => changed.push(Changed::Head(key.clone())),
            Some(base_pkg) => {
                if pkg != base_pkg {
                    changed.push(Changed::Head(key.clone()));
                }
            }
        }
    }
    for key in base.packages.keys() {
        if !head.packages.contains_key(key) {
            changed.push(Changed::Base(key.clone()));
        }
    }
    changed
}

/// Reverse adjacency: for each package, the set of package keys that list it as a dependency.
/// An edge it cannot resolve is an `Err`, never a skip: a dropped edge could hide a shared dependent.
fn reverse_closure(graph: &Graph) -> Result<Reverse, String> {
    let mut reverse: Reverse = BTreeMap::new();
    for (key, pkg) in &graph.packages {
        for dep in &pkg.deps {
            let dep_key = graph.resolve(dep).ok_or_else(|| dep.clone())?;
            reverse.entry(dep_key).or_default().insert(key.clone());
        }
    }
    Ok(reverse)
}

/// The names of packages with no `source` line - the workspace members (path dependencies),
/// including non-`crates` members like `sutura-dev` and `xtask`. The walk stops at these.
fn sourceless_names(graph: &Graph) -> BTreeSet<String> {
    graph
        .packages
        .iter()
        .filter(|(_, pkg)| pkg.source.is_none())
        .map(|(key, _)| key.0.clone())
        .collect()
}

/// The members a reverse walk from `start` stops at: packages with no `source` (path
/// dependencies). A package nothing depends on is a root and is returned too, so a root that is no
/// adapter maps to no category and fails closed.
fn bfs_to_members(start: &Key, reverse: &Reverse, boundaries: &BTreeSet<String>) -> BTreeSet<String> {
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut visited: BTreeSet<Key> = BTreeSet::new();
    let mut queue: VecDeque<Key> = VecDeque::new();
    visited.insert(start.clone());
    queue.push_back(start.clone());
    while let Some(current) = queue.pop_front() {
        // A member or a root is a boundary: record it, never walk through it.
        let parents = reverse.get(&current);
        if boundaries.contains(&current.0) || parents.is_none() {
            reached.insert(current.0.clone());
            continue;
        }
        for parent in parents.into_iter().flatten() {
            if visited.insert(parent.clone()) {
                queue.push_back(parent.clone());
            }
        }
    }
    reached
}

/// Read the base and head `Cargo.lock` text for lock attribution. Returns `None` on any read
/// failure - `select` then fails `Cargo.lock` closed to `core`.
pub(super) fn build_locks(root: &Path, base: &str) -> Option<Locks> {
    let head = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let out = std::process::Command::new("git")
        .args(["show", &format!("{base}:Cargo.lock")])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(Locks {
        base: String::from_utf8_lossy(&out.stdout).into_owned(),
        head,
    })
}

/// Attribute a `Cargo.lock` path to categories, or refuse to `core`. Called only for
/// `path == "Cargo.lock"` by the parent's `select`. When `locks` is `None` (no `--since`
/// base ref), the lock cannot be diffed and fails closed to `core`. Mutates the parent's
/// `selected`, `core`, and `reasons` in place so `select` has no match of its own.
pub(super) fn handle_lock(locks: Option<&Locks>, selected: &mut BTreeSet<String>, core: &mut bool, reasons: &mut Vec<String>) {
    let attribution = locks.map_or_else(
        || {
            Attribution::Core(String::from(
                "Cargo.lock changed without a base ref to diff against - running every category",
            ))
        },
        |locks| attribute(&locks.base, &locks.head),
    );
    match attribution {
        Attribution::Selected(cats) => selected.extend(cats),
        Attribution::Core(why) => {
            *core = true;
            reasons.push(why);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real workspace's shape: a shared domain crate, two adapters, a shared composition root
    /// that depends on both, and `xtask`, a member outside `crates/`. Members have no `source`.
    const LOCK: &str = "\
version = 4

[[package]]
name = \"arrow-array\"
version = \"59.2.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"duckdb\"
version = \"1.10.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
dependencies = [
 \"arrow-array 59.2.0\",
 \"libduckdb-sys\",
]

[[package]]
name = \"libduckdb-sys\"
version = \"1.10.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"tokio-postgres\"
version = \"0.7.12\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"tempfile\"
version = \"3.20.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"insta\"
version = \"1.40.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"sutura-domain\"
version = \"0.5.1\"
dependencies = [
 \"arrow-array 59.2.0\",
]

[[package]]
name = \"sutura-exec-duckdb\"
version = \"0.5.1\"
dependencies = [
 \"duckdb\",
 \"sutura-domain\",
]

[[package]]
name = \"sutura-exec-postgres\"
version = \"0.5.1\"
dependencies = [
 \"sutura-domain\",
 \"tempfile\",
 \"tokio-postgres\",
]

[[package]]
name = \"sutura-app\"
version = \"0.5.1\"
dependencies = [
 \"insta\",
 \"sutura-exec-duckdb\",
 \"sutura-exec-postgres\",
]

[[package]]
name = \"xtask\"
version = \"0.1.0\"
dependencies = [
 \"tempfile\",
]
";

    fn bump(lock: &str, name: &str, from: &str, to: &str) -> String {
        let head = lock.replace(
            &format!("\"{name}\"\nversion = \"{from}\""),
            &format!("\"{name}\"\nversion = \"{to}\""),
        );
        assert_ne!(head, lock, "the fixture has no `{name}` {from}");
        head.replace(&format!("\"{name} {from}\""), &format!("\"{name} {to}\""))
    }

    fn core_reason(base: &str, head: &str) -> String {
        match attribute(base, head) {
            Attribution::Core(why) => why,
            Attribution::Selected(cats) => panic!("must fail closed to core, selected {cats:?}"),
        }
    }

    fn narrowed(base: &str, head: &str) -> BTreeSet<String> {
        match attribute(base, head) {
            Attribution::Selected(cats) => cats,
            Attribution::Core(why) => panic!("must narrow, fell to core: {why}"),
        }
    }

    #[test]
    fn a_package_reached_only_by_an_adapter_narrows_despite_a_shared_dependent() {
        let cats = narrowed(LOCK, &bump(LOCK, "duckdb", "1.10.0", "1.11.0"));
        assert_eq!(cats, BTreeSet::from([String::from("data_source_duckdb")]));
    }

    #[test]
    fn a_transitive_package_reached_only_through_an_adapter_narrows() {
        let cats = narrowed(LOCK, &bump(LOCK, "libduckdb-sys", "1.10.0", "1.11.0"));
        assert_eq!(cats, BTreeSet::from([String::from("data_source_duckdb")]));
    }

    #[test]
    fn a_package_reached_directly_by_a_shared_crate_selects_core() {
        let why = core_reason(LOCK, &bump(LOCK, "insta", "1.40.0", "1.41.0"));
        assert!(why.contains("shared crate `sutura-app`"), "{why}");
    }

    #[test]
    fn a_package_an_adapter_shares_with_a_shared_crate_selects_core() {
        let why = core_reason(LOCK, &bump(LOCK, "arrow-array", "59.2.0", "60.0.0"));
        assert!(why.contains("shared crate `sutura-domain`"), "{why}");
    }

    #[test]
    fn a_package_a_root_outside_the_members_also_reaches_selects_core() {
        let why = core_reason(LOCK, &bump(LOCK, "tempfile", "3.20.0", "3.21.0"));
        assert!(why.contains("`xtask`"), "{why}");
    }

    #[test]
    fn a_dependency_that_names_no_package_selects_core() {
        let head = LOCK.replace(" \"tokio-postgres\",\n]", " \"tokio-postgres\",\n \"ghost\",\n]");
        assert_ne!(head, LOCK);
        let why = core_reason(LOCK, &head);
        assert!(why.contains("`ghost`"), "{why}");
    }

    #[test]
    fn a_lock_that_is_not_a_lockfile_fails_closed_to_core() {
        assert!(core_reason("this is not a lockfile", LOCK).contains("could not be parsed"));
        assert!(core_reason(LOCK, "garbage").contains("could not be parsed"));
        let unversioned = LOCK.replacen("version = \"0.7.12\"\n", "", 1);
        assert_ne!(unversioned, LOCK);
        assert!(core_reason(LOCK, &unversioned).contains("could not be parsed"));
    }

    #[test]
    fn a_patch_section_in_either_lock_fails_closed_to_core() {
        for section in ["[patch.crates-io]", "[[patch.unused]]"] {
            let patched = format!("{LOCK}\n{section}\n");
            assert!(core_reason(LOCK, &patched).contains("head"), "{section} in head");
            assert!(core_reason(&patched, LOCK).contains("base"), "{section} in base");
        }
    }

    #[test]
    fn a_changed_source_fails_closed_to_core() {
        let head = LOCK.replacen(
            "registry+https://github.com/rust-lang/crates.io-index",
            "git+https://example.com/arrow.git#59.2.0",
            1,
        );
        assert!(core_reason(LOCK, &head).contains("changed source"));
    }

    #[test]
    fn an_unchanged_lockfile_fails_closed_to_core() {
        assert!(core_reason(LOCK, LOCK).contains("no package changes"));
    }

    #[test]
    fn a_changed_checksum_fails_closed_to_core_beside_a_change_that_narrows() {
        let narrowing = bump(LOCK, "libduckdb-sys", "1.10.0", "1.11.0");
        let arrow = "name = \"arrow-array\"\nversion = \"59.2.0\"\n";
        let head = narrowing.replacen(arrow, &format!("{arrow}checksum = \"0123abcd\"\n"), 1);
        assert_ne!(head, narrowing);
        let why = core_reason(LOCK, &head);
        assert!(why.contains("`arrow-array` 59.2.0 changed checksum"), "{why}");
    }

    #[test]
    fn the_walk_stops_at_a_member_outside_crates() {
        let base = format!(
            "{}\
[[package]]
name = \"jsonwebtoken\"
version = \"11.1.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"sutura-dev\"
version = \"0.5.1\"
dependencies = [
 \"jsonwebtoken\",
]
",
            LOCK.replacen(
                " \"sutura-domain\",\n \"tempfile\",",
                " \"sutura-domain\",\n \"sutura-dev\",\n \"tempfile\",",
                1
            )
        );
        let why = core_reason(&base, &bump(&base, "jsonwebtoken", "11.1.0", "11.1.1"));
        assert!(why.contains("shared crate `sutura-dev`"), "{why}");
    }

    #[test]
    fn a_changed_package_that_reaches_no_member_fails_closed_to_core() {
        // An external cycle reaching no member: without the guard the walk selects nothing.
        let head = format!(
            "{LOCK}\
[[package]]
name = \"cyc-a\"
version = \"1.0.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
dependencies = [
 \"cyc-b 1.0.0\",
]

[[package]]
name = \"cyc-b\"
version = \"1.0.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
dependencies = [
 \"cyc-a 1.0.0\",
]
"
        );
        let why = core_reason(LOCK, &head);
        assert!(why.contains("reaches no workspace member"), "{why}");
    }

    fn declared() -> BTreeSet<String> {
        ["data_source_duckdb", "data_source_postgres", "catalog_local", "identity"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn a_duckdb_only_lockfile_change_selects_only_data_source_duckdb() {
        let head = bump(LOCK, "duckdb", "1.10.0", "1.11.0");
        let locks = Locks {
            base: String::from(LOCK),
            head,
        };
        let (core, selected, reasons) = super::super::select(&[String::from("Cargo.lock")], Some(&declared()), Some(&locks));
        assert!(!core, "a duckdb-only lock change must not run everything: {reasons:?}");
        assert_eq!(selected, BTreeSet::from([String::from("data_source_duckdb")]));
    }

    #[test]
    fn cargo_lock_without_a_base_ref_fails_closed_to_core() {
        let cats = super::super::derive_from(&[String::from("Cargo.lock")], Ok(declared()), Path::new("."), None);
        assert!(cats.core, "Cargo.lock without a base ref must run everything: {cats:?}");
    }
}

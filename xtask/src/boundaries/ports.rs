//! A driving port is not declared by one of its callers.
//!
//! `AGENTS.md` carried this as an invariant and said plainly that it was **not gated**, with the
//! right reason: `check-boundaries` reads dependency direction, not which crate declares a trait.
//! It is not hypothetical either - `Surface` WAS declared in `sutura-http` once, and a review moved
//! it, on the argument that a second transport would then have had to reach the application's
//! interface through the HTTP one.
//!
//! # Who a caller is, and why it is derived
//!
//! A crate that declares a **normal dependency on `sutura-app`** - the crate that owns the driving
//! port. Derived from the manifests rather than listed, because a hardcoded list of transports
//! would not cover the next one: as the tree stands it resolves to `sutura-cli`, `sutura-http`,
//! `sutura-mcp` and `sutura-serve`, which is exactly the set a list would have named, and a fifth
//! caller is covered the day it is written. DIRECT rather than transitive on purpose: a crate that
//! reaches `sutura-app` through a transport is not a caller of the port, it is a caller of the
//! transport.
//!
//! Zero callers is a failure, not a pass. A renamed application crate would otherwise leave this
//! rule green and checking nothing.
//!
//! # What the rule is
//!
//! **No `pub trait` in a caller's `src/`**, except one named in [`PERMITTED_IN_A_CALLER`] with a
//! reason. An allowlist rather than an attempt to tell a driving port from a driven one: which of
//! those a trait IS depends on who implements it and who calls it, and that is an architecture
//! question a text scan may not pretend to answer. So the gate makes every trait in a caller a
//! place where somebody wrote down which kind it is - and a stale entry, one naming a trait the
//! tree no longer declares, fails too, because an exception that has outlived its subject reads as
//! a rule still being applied.
//!
//! One entry today: `sutura_http::inbound::keys::KeySetSource`, which is a DRIVEN port and an
//! internal seam - its implementor `FileKeySet` is in the same file, and the second one would be a
//! JWKS endpoint. That is the shape an entry has to argue.
//!
//! # Limits
//!
//! * `src/` only. An integration test under `tests/` is a separate crate and its traits are test
//!   vocabulary - `sutura-app`'s `CatalogUnderTest` is the live example, and it is not in a caller
//!   anyway. A `pub trait` inside a caller's `#[cfg(test)]` module cannot arise: `unreachable_pub`
//!   is denied workspace-wide, so `pub` inside a private module is already an error.
//! * The scan is line-oriented, so a `pub trait` written inside a multi-line string literal would
//!   be reported. No such fixture exists in the four crates it reads, and the alternative -
//!   lexing Rust to answer a question about four `src` directories - buys nothing.
//! * It says nothing about a trait declared in the DOMAIN. That direction is right by
//!   construction: `sutura-domain` is what everything depends on, so a port there is inward.

use std::collections::BTreeSet;
use std::path::Path;

use crate::repo;

/// The crate that owns the driving port. A caller is anything that declares it.
const APPLICATION: &str = "sutura-app";

/// A trait a caller may declare, and why.
struct PermittedPort {
    /// The trait's name, as `pub trait <name>` spells it.
    name: &'static str,
    /// Which crate declares it, so an entry cannot quietly cover a second crate's trait of the
    /// same name.
    declared_by: &'static str,
    /// Why this one is not a driving port in the wrong crate. Printed, because an exception whose
    /// reason is unstated is how the next one gets added without an argument.
    why: &'static str,
}

/// **Adding an entry here is an architecture decision. That is the point** - the sentence
/// `ALLOWED_IN_DOMAIN` and `FORBIDDEN_EDGES` both carry, for the same reason: the diff is where
/// the argument happens.
const PERMITTED_IN_A_CALLER: &[PermittedPort] = &[PermittedPort {
    name: "KeySetSource",
    declared_by: "sutura-http",
    why: "a DRIVEN port and an internal seam of the inbound gate: where a key set is read from. \
          Its one implementor, `FileKeySet`, is in the same file, and the second would be a JWKS \
          endpoint - so nothing outside this transport implements it and no other crate has to \
          reach through the transport to use it. That is the opposite of the `Surface` case this \
          rule exists for, where the implementation was the application itself",
}];

/// What the check looked at, and what it found.
pub(super) struct Report {
    /// The callers it read, so a rule that found none cannot report `ok`.
    pub(super) callers: Vec<String>,
    /// Rust files examined.
    pub(super) files: usize,
    /// One line per violation, already formatted for stderr.
    pub(super) problems: Vec<String>,
    /// The exceptions and their reasons, formatted. Printed on SUCCESS, because an allowlist a
    /// green run never mentions is one nobody re-reads - the same argument the refusal gate's
    /// census listing makes.
    pub(super) permitted: Vec<String>,
}

/// Every caller of the driving port, scanned.
pub(super) fn check(meta: &serde_json::Value) -> Result<Report, String> {
    let (root, files) = repo::all_files()
        .and_then(|census| census.into_listing(repo::Unmigrated::Boundaries))
        .map_err(|why| why.describe())?;
    let callers = callers_of_the_application(meta, &root)?;
    let mut problems = Vec::new();
    let mut declared: BTreeSet<(String, String)> = BTreeSet::new();
    let mut scanned = 0_usize;

    for rel in &files {
        let Some(caller) = callers.iter().find(|caller| rel.starts_with(caller.src.as_str())) else {
            continue;
        };
        if !is_rust(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        for (index, name) in public_traits(&text) {
            declared.insert((caller.name.clone(), String::from(name)));
            if permitted(&caller.name, name) {
                continue;
            }
            problems.push(format!(
                "{rel}:{index}: `{}` declares `pub trait {name}`, and it is a caller of the driving port",
                caller.name
            ));
        }
    }
    if scanned == 0 {
        return Err(format!(
            "found {} caller(s) of {APPLICATION} but no .rs file in them",
            callers.len()
        ));
    }
    // The ratchet's other direction: an entry naming a trait no caller declares any more.
    for entry in PERMITTED_IN_A_CALLER {
        if !declared.contains(&(String::from(entry.declared_by), String::from(entry.name))) {
            problems.push(format!(
                "PERMITTED_IN_A_CALLER: `{}` no longer declares `pub trait {}` - delete the entry",
                entry.declared_by, entry.name
            ));
        }
    }
    Ok(Report {
        callers: callers.into_iter().map(|caller| caller.name).collect(),
        files: scanned,
        problems,
        permitted: PERMITTED_IN_A_CALLER
            .iter()
            .map(|entry| format!("{}::{} - {}", entry.declared_by, entry.name, entry.why))
            .collect(),
    })
}

/// A crate that calls the driving port, and where its own source lives.
///
/// `pub(super)` because [`super::answer_path`] asks the same question of the same set: who calls
/// the driving port. A second derivation of it would be a second thing to keep in step, and the
/// two rules would then disagree about who is covered the day a fifth transport is written.
#[derive(Debug)]
pub(super) struct Caller {
    /// The package name.
    pub(super) name: String,
    /// Repo-relative `src` directory, with a trailing slash so a prefix test cannot match a
    /// sibling whose name merely starts the same way.
    pub(super) src: String,
}

/// Workspace members declaring a normal dependency on [`APPLICATION`].
pub(super) fn callers_of_the_application(meta: &serde_json::Value, root: &Path) -> Result<Vec<Caller>, String> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let mut callers = Vec::new();
    for package in packages {
        if !declares_normal(package, APPLICATION) {
            continue;
        }
        let Some(name) = package.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let Some(src) = package
            .get("manifest_path")
            .and_then(|p| p.as_str())
            .map(Path::new)
            .and_then(Path::parent)
            .map(|dir| dir.join("src"))
            .as_deref()
            .and_then(|dir| repo::relative(root, dir))
        else {
            continue;
        };
        callers.push(Caller {
            name: String::from(name),
            src: format!("{src}/"),
        });
    }
    if callers.is_empty() {
        // A vacuous pass is the failure mode a gate exists to prevent, so say so instead.
        return Err(format!(
            "no workspace member declares a normal dependency on {APPLICATION} - either it was \
             renamed, or the driving port has no callers and this rule would check nothing"
        ));
    }
    Ok(callers)
}

/// Does `package` declare `wanted` as a normal dependency?
fn declares_normal(package: &serde_json::Value, wanted: &str) -> bool {
    package.get("dependencies").and_then(|d| d.as_array()).is_some_and(|deps| {
        deps.iter().any(|dep| {
            // A null `kind` is a normal dependency; the other two are `dev` and `build`.
            dep.get("kind").is_none_or(serde_json::Value::is_null) && dep.get("name").and_then(|n| n.as_str()) == Some(wanted)
        })
    })
}

/// Is this trait permitted in this crate?
fn permitted(caller: &str, name: &str) -> bool {
    PERMITTED_IN_A_CALLER
        .iter()
        .any(|entry| entry.name == name && entry.declared_by == caller)
}

/// Every `pub trait <Name>` in `text`, as a 1-based line number and the name.
fn public_traits(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(rest) = line.trim().strip_prefix("pub trait ") else {
            continue;
        };
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if let Some(name) = rest.get(..end)
            && !name.is_empty()
        {
            found.push((index.saturating_add(1), name));
        }
    }
    found
}

/// Is this path a Rust source file? Case-insensitive, because half of this repo is developed on a
/// case-insensitive filesystem and a case-sensitive test there is a silent hole.
pub(super) fn is_rust(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Printed when this half fails, because a rule whose reason is unstated gets reverted.
pub(super) fn explain() {
    eprintln!("A DRIVING port is declared by the application, never by one of its callers. A driven");
    eprintln!("port is dependency inversion - the interior declares what it needs - but a driving");
    eprintln!("port inverts nothing: the caller is already outside and the implementation already IS");
    eprintln!("the application. `Surface` was declared in `sutura-http` once, and a second transport");
    eprintln!("would have had to reach the application's interface through the HTTP one.");
    eprintln!();
    eprintln!("A trait in a caller is therefore either a driving port in the wrong crate, or a driven");
    eprintln!("port that is an internal seam - and which one it is depends on who implements it,");
    eprintln!("which no text scan can see. So say which in boundaries/ports.rs, with the reason:");
    eprintln!("that is an architecture decision and should be a visible diff.");
    eprintln!();
    eprintln!("Read `sutura_app::surface`'s own module documentation before moving a port or adding");
    eprintln!("one - including why deleting a one-implementation trait was the weaker option.");
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{PERMITTED_IN_A_CALLER, callers_of_the_application, declares_normal, permitted, public_traits};

    /// One caller, one non-caller, and one crate that only dev-depends on the application, shaped
    /// like `cargo metadata --no-deps`.
    fn metadata() -> serde_json::Value {
        serde_json::from_str(
            r#"{
                "packages": [
                    {
                        "name": "sutura-http",
                        "manifest_path": "/repo/crates/sutura-http/Cargo.toml",
                        "dependencies": [{"name": "sutura-app", "kind": null}]
                    },
                    {
                        "name": "sutura-semantic",
                        "manifest_path": "/repo/crates/sutura-semantic/Cargo.toml",
                        "dependencies": [{"name": "sutura-domain", "kind": null}]
                    },
                    {
                        "name": "sutura-exec-bigquery",
                        "manifest_path": "/repo/crates/sutura-exec-bigquery/Cargo.toml",
                        "dependencies": [{"name": "sutura-app", "kind": "dev"}]
                    }
                ]
            }"#,
        )
        .expect("fixture parses")
    }

    #[test]
    fn a_caller_is_a_crate_that_declares_the_application() {
        let callers = callers_of_the_application(&metadata(), Path::new("/repo")).expect("one caller");
        let names: Vec<&str> = callers.iter().map(|caller| caller.name.as_str()).collect();
        assert_eq!(names, vec!["sutura-http"], "{names:?}");
        assert_eq!(
            callers.first().map(|caller| caller.src.as_str()),
            Some("crates/sutura-http/src/")
        );
    }

    #[test]
    fn a_dev_dependency_on_the_application_does_not_make_a_caller() {
        // `sutura-exec-bigquery` dev-depends on `sutura-app` for its corpus differential, and a
        // test harness is not a caller of the driving port in the sense this rule is about.
        assert!(!declares_normal(
            metadata()
                .get("packages")
                .and_then(|p| p.as_array())
                .and_then(|p| p.get(2))
                .expect("the third fixture package"),
            "sutura-app"
        ));
    }

    #[test]
    fn a_workspace_with_no_caller_is_an_error_not_a_pass() {
        let meta: serde_json::Value =
            serde_json::from_str(r#"{"packages": [{"name": "a", "manifest_path": "/repo/a/Cargo.toml", "dependencies": []}]}"#)
                .expect("fixture parses");
        drop(callers_of_the_application(&meta, Path::new("/repo")).expect_err("must not pass vacuously"));
    }

    #[test]
    fn a_transport_declaring_a_pub_trait_is_found() {
        let text = "//! A transport.\n\npub trait Surface: Send + Sync + 'static {\n    fn answer(&self);\n}\n";
        assert_eq!(public_traits(text), vec![(3, "Surface")]);
    }

    #[test]
    fn a_doc_comment_naming_one_is_not_a_declaration() {
        // The obvious confound: this repo's prose quotes the shape it forbids.
        let text = "/// `pub trait Surface` was declared here once, and a review moved it.\npub fn f() {}\n";
        assert!(public_traits(text).is_empty());
    }

    #[test]
    fn a_private_or_crate_visible_trait_is_not_this_rules_business() {
        // Not reachable from outside, so nothing can be a port declared for another crate to
        // implement. `unreachable_pub` is what keeps the visibility honest.
        let text = "trait Local {}\npub(crate) trait Internal {}\n";
        assert!(public_traits(text).is_empty());
    }

    #[test]
    fn the_allowlist_is_matched_on_the_crate_as_well_as_the_name() {
        // So an entry taken for one transport cannot silently cover a same-named trait in the other.
        assert!(permitted("sutura-http", "KeySetSource"));
        assert!(!permitted("sutura-mcp", "KeySetSource"));
        assert!(!permitted("sutura-http", "Surface"));
    }

    #[test]
    fn every_permitted_port_says_why() {
        for entry in PERMITTED_IN_A_CALLER {
            assert!(!entry.name.is_empty(), "an entry names a trait");
            assert!(!entry.declared_by.is_empty(), "{} names no crate", entry.name);
            assert!(!entry.why.is_empty(), "{} has no reason", entry.name);
        }
    }
}

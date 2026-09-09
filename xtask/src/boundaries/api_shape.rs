//! The typed-surface half of the boundary gate: three rules from the three principles this
//! repo follows, in the only form that cannot rot - a check.
//!
//! * **newtype** - a library crate's `pub struct` keeps its fields private, so the invariant
//!   its constructor establishes cannot be walked around with a struct literal. A `pub` field
//!   makes the constructor advice.
//! * **structured errors** - a library crate declares no dynamic-error crate. `anyhow` in a
//!   library forces every caller to depend on it, erases the typed contract, and turns a major
//!   version bump of that crate into a breaking change of ours.
//! * **structured errors** - a library crate's `Result` error type is never `String`. A
//!   message is for a human; a caller that has to parse one has no contract at all.
//!
//! Scope is LIBRARY crates, taken from `cargo metadata` rather than a hardcoded list, because
//! all three rules are about a surface somebody else depends on. A binary's error may be a
//! string - its audience is a human reading stderr, which is the case the error guide
//! explicitly permits, and `xtask` and `sutura-dev` do exactly that on purpose.
//!
//! Test code is NOT exempt. The strict panic lints exempt tests because an assert is not
//! attack surface; these rules are about the shape of a type, which is the same shape whether
//! a test or a caller constructs it.
//!
//! Two limits, stated rather than hidden. The scan is line-oriented, so a signature that
//! rustfmt wrapped across lines escapes the `String`-error check - at `max_width = 130` that is
//! rare, and a parser that understood Rust properly would be a bigger thing to trust than the
//! rule. And a `pub(crate)` field is flagged too: inside a library it is still a way for the
//! crate's own code to build a value its constructor would have rejected.

use std::path::Path;

use crate::repo;

/// Crates whose whole purpose is an opaque, dynamically typed error.
///
/// Excellent in a binary, wrong in a library: the guide's rule is "return only your own or
/// standard library error types across crate boundaries". Adding a name here is an
/// architecture decision, same as the dependency allowlist next door.
const DYNAMIC_ERROR_CRATES: &[&str] = &["anyhow", "eyre", "color-eyre", "failure"];

/// What the scan looked at, and what it found.
pub(crate) struct Report {
    /// Library source files examined. Printed on success, so a gate that checked nothing
    /// cannot report "ok" and look like it did some work.
    pub(crate) files: usize,
    /// One line per violation, already formatted for stderr.
    pub(crate) problems: Vec<String>,
}

/// Repo-relative `src` directory of every workspace package that produces a library.
fn library_src_dirs(meta: &serde_json::Value, root: &Path) -> Result<Vec<String>, String> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;

    let mut dirs = Vec::new();
    for package in packages {
        let targets = package.get("targets").and_then(|t| t.as_array());
        for target in targets.into_iter().flatten() {
            if !is_library(target) {
                continue;
            }
            let Some(src_path) = target.get("src_path").and_then(|p| p.as_str()) else {
                continue;
            };
            // `src_path` is the crate root file; its directory is the crate's whole source.
            if let Some(dir) = Path::new(src_path).parent().and_then(|d| repo::relative(root, d)) {
                dirs.push(dir);
            }
        }
    }
    if dirs.is_empty() {
        // A vacuous pass is the failure mode a gate exists to prevent, so say so instead.
        return Err(String::from(
            "no library target in the workspace - this gate would check nothing",
        ));
    }
    Ok(dirs)
}

/// Is this cargo target a library? `rlib` because that is what cargo reports for a plain
/// `[lib]`, and both spellings appear depending on how the crate is built.
fn is_library(target: &serde_json::Value) -> bool {
    target
        .get("kind")
        .and_then(|k| k.as_array())
        .is_some_and(|kinds| kinds.iter().filter_map(|k| k.as_str()).any(|k| matches!(k, "lib" | "rlib")))
}

/// Library packages declaring a dynamic-error crate as a normal dependency.
///
/// Normal only: a dev-dependency never crosses the crate's public surface, so `anyhow` in a
/// test harness is not this rule's business.
fn dynamic_error_deps(meta: &serde_json::Value) -> Result<Vec<String>, String> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;

    let mut problems = Vec::new();
    for package in packages {
        let is_lib = package
            .get("targets")
            .and_then(|t| t.as_array())
            .is_some_and(|targets| targets.iter().any(is_library));
        if !is_lib {
            continue;
        }
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>");
        let deps = package.get("dependencies").and_then(|d| d.as_array());
        for dep in deps.into_iter().flatten() {
            // A null `kind` is a normal dependency; "dev" and "build" are the other two.
            if dep.get("kind").is_some_and(|k| !k.is_null()) {
                continue;
            }
            let dep_name = dep.get("name").and_then(|n| n.as_str()).unwrap_or_default();
            if DYNAMIC_ERROR_CRATES.contains(&dep_name) {
                problems.push(format!(
                    "{name}: depends on `{dep_name}`, which is a dynamic-error crate in a LIBRARY"
                ));
            }
        }
    }
    Ok(problems)
}

/// The name in `pub struct Name ..`, or `None` if this line is not one.
fn pub_struct_name(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("pub struct ")?;
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = rest.get(..end)?;
    (!name.is_empty()).then_some(name)
}

/// The text between the parentheses of a tuple struct declared on one line.
fn tuple_fields(trimmed: &str) -> Option<&str> {
    let open = trimmed.find('(')?;
    let close = trimmed.rfind(')')?;
    trimmed.get(open.saturating_add(1)..close)
}

/// Does any comma-separated field in `fields` carry a `pub` visibility?
fn declares_pub(fields: &str) -> bool {
    fields.split(',').any(|field| {
        let field = field.trim();
        field == "pub" || field.starts_with("pub ") || field.starts_with("pub(")
    })
}

/// Where the line-by-line walk of a file currently is.
enum Scan {
    /// Not inside a `pub struct` body.
    Outside,
    /// Saw the declaration, but a generic or `where` clause pushed the `{` to a later line.
    AwaitingBody(String),
    /// Inside the named struct's body, at this brace depth.
    InBody { name: String, depth: usize },
}

/// A `pub` field on a `pub struct` - the newtype invariant made bypassable.
fn pub_field_violations(rel: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut scan = Scan::Outside;

    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        let trimmed = line.trim();
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();

        scan = match scan {
            Scan::Outside => {
                let Some(name) = pub_struct_name(trimmed) else {
                    continue;
                };
                // A tuple struct is declared and finished on one line, so it never opens a
                // body to walk into. `continue` rather than an else-branch: the two cases
                // have nothing to share, and pairing them reads as one decision.
                if let Some(fields) = tuple_fields(trimmed) {
                    if declares_pub(fields) {
                        problems.push(format!("{rel}:{number}: `{name}` has a pub tuple field"));
                    }
                    continue;
                }
                if opens > closes {
                    Scan::InBody {
                        name: String::from(name),
                        depth: opens.saturating_sub(closes),
                    }
                } else if opens == 0 {
                    Scan::AwaitingBody(String::from(name))
                } else {
                    // `pub struct X {}` - declared and closed, no fields to check.
                    Scan::Outside
                }
            }
            Scan::AwaitingBody(name) => {
                if opens > closes {
                    Scan::InBody {
                        name,
                        depth: opens.saturating_sub(closes),
                    }
                } else {
                    Scan::AwaitingBody(name)
                }
            }
            Scan::InBody { name, depth } => {
                // Only depth 1 is the struct's own field list; anything deeper belongs to a
                // nested type in a field's position.
                //
                // The visibility test matches `declares_pub` above rather than a bare
                // `starts_with("pub")`, which also matched a field *named* `public` and failed the
                // gate on correct code. A gate that fires on a name is one people learn to work
                // around by renaming, which is how it stops meaning anything.
                let is_pub = trimmed.starts_with("pub ") || trimmed.starts_with("pub(") || trimmed == "pub";
                if depth == 1 && is_pub {
                    let field = trimmed.split(':').next().unwrap_or(trimmed).trim();
                    problems.push(format!("{rel}:{number}: `{name}` has a pub field `{field}`"));
                }
                let next = depth.saturating_add(opens).saturating_sub(closes);
                if next == 0 {
                    Scan::Outside
                } else {
                    Scan::InBody { name, depth: next }
                }
            }
        };
    }
    problems
}

/// The text up to the `>` closing a generic list that opened just before `tail`.
///
/// `-` and `=` guard against `->` and `=>`, which are not closing angle brackets and which
/// appear inside a `Result<..>` often enough to matter (`Result<Box<dyn Fn() -> u8>, E>`).
fn generic_args(tail: &str) -> Option<&str> {
    let mut depth: usize = 1;
    let mut previous = ' ';
    for (at, character) in tail.char_indices() {
        match character {
            '<' => depth = depth.saturating_add(1),
            '>' if previous != '-' && previous != '=' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return tail.get(..at);
                }
            }
            _ => {}
        }
        previous = character;
    }
    None
}

/// The last comma-separated argument at nesting depth zero, or `None` when there is no
/// top-level comma - which is how `Result<String>` (a crate alias with one parameter) avoids
/// being read as an error type.
fn last_top_level(args: &str) -> Option<&str> {
    let mut depth: usize = 0;
    let mut start: Option<usize> = None;
    let mut previous = ' ';
    for (at, character) in args.char_indices() {
        match character {
            '<' | '(' | '[' => depth = depth.saturating_add(1),
            '>' if previous != '-' && previous != '=' => depth = depth.saturating_sub(1),
            ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => start = Some(at.saturating_add(1)),
            _ => {}
        }
        previous = character;
    }
    args.get(start?..).map(str::trim)
}

/// The error type of every `Result<..>` written on this line.
fn result_error_types(line: &str) -> Vec<&str> {
    const MARKER: &str = "Result<";

    let mut found = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find(MARKER) {
        let tail = rest.get(at.saturating_add(MARKER.len())..).unwrap_or_default();
        if let Some(error) = generic_args(tail).and_then(last_top_level) {
            found.push(error);
        }
        rest = tail;
    }
    found
}

/// Is this the standard library's `String` in an error position?
fn is_stringly(error: &str) -> bool {
    error == "String" || error.ends_with("::String")
}

/// A `Result` in a library crate whose error type is `String`.
fn stringly_error_violations(rel: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for error in result_error_types(line) {
            if is_stringly(error) {
                problems.push(format!(
                    "{rel}:{}: `Result<.., {error}>` in a library - the error type is the API",
                    index.saturating_add(1)
                ));
            }
        }
    }
    problems
}

/// Is this path a Rust source file? Case-insensitive, because half of this repo is developed
/// on a case-insensitive filesystem and a case-sensitive test there is a silent hole.
fn is_rust(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Run all three checks over the workspace's library crates.
///
/// Its own `cargo metadata` call, and `--no-deps` on purpose: the dependency half next door
/// needs the whole resolve graph, while this half must see workspace members ONLY. With the
/// full graph, a transitive dependency that uses `anyhow` internally - which is its business,
/// not ours - would fail our gate.
pub(crate) fn check() -> Result<Report, String> {
    let meta = crate::cargo_metadata(&["--no-deps"])?;
    let (root, files) = repo::all_files()
        .and_then(|census| census.into_listing(repo::Unmigrated::Boundaries))
        .map_err(|why| why.describe())?;
    let dirs = library_src_dirs(&meta, &root)?;
    let mut problems = dynamic_error_deps(&meta)?;

    let mut checked = 0usize;
    for rel in &files {
        if !is_rust(rel) || !dirs.iter().any(|dir| rel.starts_with(dir.as_str())) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        checked = checked.saturating_add(1);
        problems.extend(pub_field_violations(rel, &text));
        problems.extend(stringly_error_violations(rel, &text));
    }
    if checked == 0 {
        return Err(format!(
            "found {} library source director(ies) but no .rs file in them",
            dirs.len()
        ));
    }
    Ok(Report {
        files: checked,
        problems,
    })
}

/// Printed when this half fails, because a rule whose reason is unstated gets reverted.
pub(crate) fn explain() {
    eprintln!("A library crate's types and errors ARE its contract:");
    eprintln!("  * a private field is what makes a constructor's invariant hold - a `pub` one");
    eprintln!("    lets a struct literal build the value the constructor would have rejected;");
    eprintln!("  * `anyhow` in a library makes every caller depend on it and erases the typed");
    eprintln!("    contract - return your own `thiserror` enum instead;");
    eprintln!("  * a `String` error makes callers parse prose. Name the failure modes.");
    eprintln!("If a case here genuinely belongs, change the rule in boundaries/api_shape.rs");
    eprintln!("with the reason: that is an architecture decision and should be a visible diff.");
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{dynamic_error_deps, library_src_dirs, pub_field_violations, result_error_types, stringly_error_violations};

    /// One library (`lib-crate`) and one binary (`bin-crate`), shaped like `cargo metadata`.
    fn metadata() -> serde_json::Value {
        serde_json::from_str(
            r#"{
                "packages": [
                    {
                        "name": "lib-crate",
                        "targets": [{"kind": ["lib"], "src_path": "/repo/crates/lib-crate/src/lib.rs"}],
                        "dependencies": [{"name": "thiserror", "kind": null}]
                    },
                    {
                        "name": "bin-crate",
                        "targets": [{"kind": ["bin"], "src_path": "/repo/bin-crate/src/main.rs"}],
                        "dependencies": [{"name": "anyhow", "kind": null}]
                    }
                ]
            }"#,
        )
        .expect("fixture parses")
    }

    #[test]
    fn only_library_targets_are_in_scope() {
        let dirs = library_src_dirs(&metadata(), Path::new("/repo")).expect("one library");
        assert_eq!(dirs, vec![String::from("crates/lib-crate/src")]);
    }

    #[test]
    fn a_workspace_with_no_library_is_an_error_not_a_pass() {
        let meta: serde_json::Value = serde_json::from_str(
            r#"{"packages": [{"name": "b", "targets": [{"kind": ["bin"], "src_path": "/repo/b/src/main.rs"}]}]}"#,
        )
        .expect("fixture parses");
        // A gate that silently checks nothing is worse than no gate.
        drop(library_src_dirs(&meta, Path::new("/repo")).expect_err("must not pass vacuously"));
    }

    #[test]
    fn a_binary_may_use_a_dynamic_error_crate() {
        // The fixture's `bin-crate` depends on anyhow deliberately: in a binary the error's
        // audience is a human reading stderr, which is the permitted case.
        assert!(
            dynamic_error_deps(&metadata()).expect("fixture walks").is_empty(),
            "the binary fixture resolves to no dynamic error dependencies"
        );
    }

    #[test]
    fn a_library_may_not() {
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [{
                    "name": "lib-crate",
                    "targets": [{"kind": ["lib"], "src_path": "/repo/l/src/lib.rs"}],
                    "dependencies": [
                        {"name": "anyhow", "kind": null},
                        {"name": "eyre", "kind": "dev"}
                    ]
                }]
            }"#,
        )
        .expect("fixture parses");
        let found = dynamic_error_deps(&meta).expect("fixture walks");
        // The dev-dependency is not a violation: it never crosses the public surface.
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.first().is_some_and(|p| p.contains("anyhow")), "{found:?}");
    }

    #[test]
    fn a_pub_tuple_field_is_a_violation() {
        let found = pub_field_violations("x.rs", "pub struct Digest(pub String);\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_private_tuple_field_is_the_point_of_the_pattern() {
        assert!(
            pub_field_violations("x.rs", "pub struct Digest(String);\n").is_empty(),
            "a private tuple field is not a public-field violation"
        );
    }

    #[test]
    fn a_pub_named_field_is_a_violation() {
        let text = "pub struct Config {\n    pub token: String,\n    inner: u8,\n}\n";
        let found = pub_field_violations("x.rs", text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.first().is_some_and(|p| p.contains("token")), "{found:?}");
    }

    #[test]
    fn a_private_field_merely_named_public_is_not_a_violation() {
        // This gate used to test `starts_with("pub")`, so a private field called `public` failed
        // it - and the way that gets resolved under time pressure is by renaming the field, which
        // teaches everyone that the gate is about spelling rather than about visibility.
        let text = "pub struct Tiers {\n    public: Quota,\n    private: Quota,\n}\n";
        let found = pub_field_violations("x.rs", text);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_pub_crate_field_is_still_a_way_round_the_constructor() {
        let found = pub_field_violations("x.rs", "pub struct Config {\n    pub(crate) token: String,\n}\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_method_after_the_body_is_not_a_field() {
        // The state machine has to leave the body, or every `pub fn` in the impl below reads
        // as a public field.
        let text = "pub struct Digest {\n    inner: String,\n}\n\nimpl Digest {\n    pub fn as_str(&self) -> &str {\n        &self.inner\n    }\n}\n";
        assert!(
            pub_field_violations("x.rs", text).is_empty(),
            "no method-after-the-body line reads as a public field"
        );
    }

    #[test]
    fn a_body_opened_on_a_later_line_is_still_scanned() {
        let text = "pub struct Wrapper<T>\nwhere\n    T: Clone,\n{\n    pub inner: T,\n}\n";
        assert_eq!(pub_field_violations("x.rs", text).len(), 1);
    }

    #[test]
    fn an_empty_struct_body_on_one_line_does_not_confuse_the_scan() {
        let text = "pub struct Marker {}\npub struct Other {\n    pub leak: u8,\n}\n";
        let found = pub_field_violations("x.rs", text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.first().is_some_and(|p| p.contains("leak")), "{found:?}");
    }

    #[test]
    fn a_private_struct_is_not_this_rules_business() {
        // Not reachable by a caller, so there is no constructor to walk around.
        assert!(
            pub_field_violations("x.rs", "struct Local {\n    pub inner: u8,\n}\n").is_empty(),
            "a private struct is not reported"
        );
    }

    #[test]
    fn a_string_error_type_is_found() {
        let found = stringly_error_violations("x.rs", "fn f() -> Result<(), String> {}\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_typed_error_is_not() {
        assert!(
            stringly_error_violations("x.rs", "fn f() -> Result<(), InvalidDigest> {}\n").is_empty(),
            "a typed error is not stringly"
        );
    }

    #[test]
    fn a_string_in_the_ok_position_is_not_an_error_type() {
        // The obvious false positive, and the reason the last argument is what gets read.
        assert!(
            stringly_error_violations("x.rs", "fn f() -> Result<String, InvalidDigest> {}\n").is_empty(),
            "a String in the Ok position is not flagged as an error type"
        );
    }

    #[test]
    fn a_nested_generic_does_not_shift_which_argument_is_the_error() {
        assert_eq!(result_error_types("fn f() -> Result<Vec<String>, MyError>"), vec!["MyError"]);
        assert_eq!(
            result_error_types("fn f() -> Result<(), HashMap<String, String>>"),
            vec!["HashMap<String, String>"]
        );
    }

    #[test]
    fn an_arrow_inside_the_generics_is_not_a_closing_bracket() {
        // `->` used to close the generic list one bracket early and report `E` as `u8`.
        assert_eq!(
            result_error_types("fn f() -> Result<Box<dyn Fn() -> u8>, MyError>"),
            vec!["MyError"]
        );
    }

    #[test]
    fn a_single_parameter_alias_is_not_read_as_an_error_type() {
        // `type Result<T> = std::result::Result<T, MyError>` is idiomatic; `Result<String>`
        // under it means Ok = String, and flagging it would make the gate a nuisance.
        assert!(
            result_error_types("fn f() -> Result<String>").is_empty(),
            "a single-parameter alias states no error type"
        );
    }

    #[test]
    fn a_qualified_string_is_still_stringly() {
        let found = stringly_error_violations("x.rs", "fn f() -> Result<(), std::string::String> {}\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn two_results_on_one_line_are_both_read() {
        assert_eq!(
            result_error_types("fn f(g: fn() -> Result<(), String>) -> Result<(), MyError>"),
            vec!["String", "MyError"]
        );
    }
}

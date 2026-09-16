//! The architecture-boundary gate. EIGHT halves - four about which way dependencies point, two
//! about the driving port, and two about what the crossing looks like:
//!
//! * the domain crate acquires no framework dependency ([`dependency_direction`])
//! * a named crate cannot reach a named crate ([`forbidden_edges`])
//! * the conformance harness reaches no adapter ([`harness_reaches_no_adapter`], and `harness`) -
//!   an allowlist in [`edges::ALLOWED_IN_DOMAIN`]'s shape, added after the rule was DISPROVED: an adapter
//!   under the harness's `[dependencies]`, used in its public API, left every other half of this
//!   gate green
//! * no adapter reaches an adapter of its own kind ([`adapter_classes`], and `adapters` for the
//!   definition, which is the whole of the work in that rule)
//! * a driving port is not declared by one of its callers ([`declared_ports`], and `ports`) - the
//!   one half that reads which crate declares a TRAIT rather than which crate depends on which
//! * a caller of that port reaches the answer path THROUGH it ([`answer_through_the_port`], and
//!   `answer_path`) - the half that keeps the audit record un-skippable, because the record is
//!   written by the port's implementor and by nothing else
//! * a library's types and errors are a typed contract, not a struct with public fields
//!   returning `Result<_, String>` (`api_shape`)
//! * an ungoverned route is mounted only inside `crate::router::Ungoverned::mount`
//!   (`ungoverned`, `xtask/src/boundaries/ungoverned.rs`) - the structural backstop of the
//!   ungoverned-route allowlist, holding that the one place `sutura-http`/`sutura-cli` may call
//!   `.nest`/`.nest_service`/`.route_service` (outside the governed `.nest(API_V1_PREFIX, …)`) is
//!   the single mount function
//!
//! One gate rather than three, because they all answer "is the boundary real?", and because a
//! rule in its own task has to be transcribed into the justfile, twice into devenv.nix, into
//! flake.nix and into a hook before it runs anywhere.
//!
//! The domain rule is an ALLOWLIST over the whole transitive tree, not a denylist over direct
//! dependencies. Two corrections to how this started, both found by review:
//!
//! * It read the *declared* dependency list, which is a manifest grep with extra steps. The
//!   domain could acquire an async runtime through any innocuous-looking crate and the gate
//!   printed "ok". It now walks `resolve.nodes`.
//! * A denylist only catches what somebody thought to name. `tower`, `tonic`, `rustls`,
//!   `sqlx` and everything else were permitted. Inverting it means a new dependency is a
//!   one-line diff to the list below - visible, arguable, and impossible to miss.
//!
//! [`forbidden_edges`] is the other shape, and it is deliberately the other shape. It is a
//! denylist, which the paragraph above argues against - and the argument does not apply here,
//! because these are not "dependencies somebody might not have thought to forbid". Each entry
//! names a crate that IS the decision: the whole content of the rule is that this one edge
//! stays absent. An allowlist would mean enumerating a legitimate tree of thirty crates to
//! express a fact about one of them, and then re-enumerating it every time an unrelated
//! dependency moved.

mod adapters;
mod answer_path;
mod api_shape;
mod edges;
mod harness;
mod ports;
mod ungoverned;

use crate::Verdict;

pub(crate) use edges::{DOMAIN, Edges, FORBIDDEN_EDGES, reaches, transitive_names, violations};

pub(crate) fn run(_args: &[String]) -> Verdict {
    // Every half runs even when an earlier one fails. They are independent findings, and a gate
    // that stops early makes the second violation look like it appeared after the first fix.
    let direction = dependency_direction();
    let edges = forbidden_edges();
    let packs = harness_reaches_no_adapter();
    let classes = adapter_classes();
    let declared = declared_ports();
    let through = answer_through_the_port();
    let surface = typed_surface();
    let mounts = ungoverned::check();
    let halves = [direction, edges, packs, classes, declared, through, surface, mounts];
    if halves.iter().all(|half| *half == Verdict::Pass) {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

/// Who DECLARES a port, which is the one question dependency direction cannot answer.
fn declared_ports() -> Verdict {
    // `--no-deps` on purpose, and for `api_shape`'s reason inverted: this half needs workspace
    // members and their DECLARED dependencies, not the resolve graph. A transitive reach into the
    // application is not what makes a crate a caller of its port.
    let meta = match crate::cargo_metadata(&["--no-deps", "--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };
    match ports::check(&meta) {
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            Verdict::Fail
        }
        Ok(report) if report.problems.is_empty() => {
            println!(
                "xtask check-boundaries: ok - no driving port declared by a caller ({} file(s) in {})",
                report.files,
                report.callers.join(", ")
            );
            for entry in &report.permitted {
                println!("  permitted, and here is the argument: {entry}");
            }
            Verdict::Pass
        }
        Ok(report) => {
            eprintln!("xtask check-boundaries: FAILED - a caller of the driving port declares a trait:");
            for problem in &report.problems {
                eprintln!("  {problem}");
            }
            eprintln!();
            ports::explain();
            Verdict::Fail
        }
    }
}

/// The other half about the driving port: a caller reaches the answer path THROUGH it.
///
/// The same `--no-deps --all-features` metadata [`declared_ports`] reads, and for the same reason:
/// this half needs workspace members and their DECLARED dependencies, because a crate that reaches
/// the application transitively is a caller of a transport rather than of its port.
///
/// Written as early returns rather than as its siblings' one `match`, because it has SIX ways to
/// fail and three of them are the rule reading nothing: either door it forbids no longer being
/// defined (`answer`'s and, since `#129` step 5, `run_sql`'s own), and no caller naming the
/// application at all. A fourth - `run_sql`'s module declared `pub` (`#703` finding 1) - is not the
/// rule reading nothing, it is the rule finding the exact hole it exists to close. The green line
/// names the three liveness checks, so a reader can tell a pass from a vacuous one without running
/// anything.
fn answer_through_the_port() -> Verdict {
    let meta = match crate::cargo_metadata(&["--no-deps", "--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };
    let report = match answer_path::check(&meta) {
        Ok(report) => report,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };
    // A door that moved is a FAILURE and not a pass. `paths` below counts paths rooted at the
    // CRATE, so it catches a rename of that and nothing else - with the FUNCTION renamed and a
    // bypass written to the new name, this half printed `ok` over 94 paths.
    let Some(door) = report.door else {
        eprintln!(
            "xtask check-boundaries: FAILED - `{}` is not defined in {}, so the path this rule forbids \
             names nothing. A caller could answer without recording and this half would still print \
             `ok`. Move the needle with the door, or delete this half if the door is gone.",
            answer_path::door(),
            answer_path::APPLICATION_LIB
        );
        return Verdict::Fail;
    };
    // The raw tool's own door, checked the same way and for the same reason: `run_sql` writes an
    // audit record exactly like `answer` does, and a liveness check that only ever looked at
    // `answer` would print `ok` forever once `run_sql` moved or was renamed.
    let Some(raw_door) = report.raw_door else {
        eprintln!(
            "xtask check-boundaries: FAILED - `{}` is not defined in {}, so the path this rule forbids \
             names nothing. A caller could run_sql without recording and this half would still print \
             `ok`. Move the needle with the door, or delete this half if the door is gone.",
            answer_path::raw_door(),
            answer_path::RAW_LIB
        );
        return Verdict::Fail;
    };
    // `#703` finding 1: `run_sql`'s module declared `pub` is a SECOND, ungated spelling of the
    // door (`sutura_app::raw::run_sql`), invisible to `names_the_door` by the same design that
    // spares `Surface::run_sql` - proven by a bypass at that spelling compiling clean and this gate
    // printing `ok` before this check existed. `Some` here is the failure, not `None`.
    if let Some(line) = report.raw_module_pub {
        eprintln!(
            "xtask check-boundaries: FAILED - {}:{line} declares `{}`. That module is `pub`, so \
             `sutura_app::raw::run_sql` is a second, legal spelling of the door this rule guards - \
             one this rule's classifier cannot see, because it matches a door only at the crate root \
             or inside a brace group (by design, so a call THROUGH the port is not flagged). Make the \
             module private (`mod raw;`) and re-export what callers need from it (`pub use raw::{{..., \
             run_sql}};`) - a door is guarded at the crate root only, so the module it lives in may \
             never be `pub`.",
            answer_path::APPLICATION_LIB,
            answer_path::RAW_MODULE
        );
        return Verdict::Fail;
    }
    // Zero paths read is a FAILURE and not a pass: a rule that no longer finds the application
    // in any caller is reading nothing while printing `ok`.
    if report.paths == 0 {
        eprintln!(
            "xtask check-boundaries: FAILED - no caller of the driving port names the application \
             in {} file(s) ({}). Either it was renamed or the path spelling changed, and this rule \
             is checking nothing.",
            report.files,
            report.callers.join(", ")
        );
        return Verdict::Fail;
    }
    if !report.problems.is_empty() {
        eprintln!("xtask check-boundaries: FAILED - a caller of the driving port answers without it:");
        for problem in &report.problems {
            eprintln!("  {problem}");
        }
        eprintln!();
        answer_path::explain();
        return Verdict::Fail;
    }
    println!(
        "xtask check-boundaries: ok - the answer path is reached through the port ({} path(s) in {} file(s) in {}, \
         door at {}:{door}, raw door at {}:{raw_door})",
        report.paths,
        report.files,
        report.callers.join(", "),
        answer_path::APPLICATION_LIB,
        answer_path::RAW_LIB
    );
    Verdict::Pass
}

/// Which way dependencies point, a third time: no edge INSIDE one class of adapter.
fn adapter_classes() -> Verdict {
    // `--all-features` for the reason every other half uses it, and `Edges::Normal` inside the
    // walk for the reason `adapters` states: a dev-dependency between two adapters is the
    // differential suite.
    let meta = match crate::cargo_metadata(&["--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };
    match adapters::check(&meta) {
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            Verdict::Fail
        }
        Ok(report) if report.problems.is_empty() => {
            let described = report
                .sizes
                .iter()
                .map(|(name, size)| format!("{name} ({size})"))
                .collect::<Vec<String>>()
                .join(", ");
            println!("xtask check-boundaries: ok - no edge inside an adapter class: {described}");
            Verdict::Pass
        }
        Ok(report) => {
            eprintln!("xtask check-boundaries: FAILED - an adapter reaches an adapter of its own kind:");
            for problem in &report.problems {
                eprintln!("  {problem}");
            }
            eprintln!();
            adapters::explain(&report.problems);
            Verdict::Fail
        }
    }
}

/// Which way dependencies point, the other way round: a named crate cannot reach a named crate.
fn forbidden_edges() -> Verdict {
    // `--all-features` for the reason [`dependency_direction`] uses it, and one more that is
    // specific to this half: a dependency moved behind a feature is still a dependency, and the
    // edge this forbids would be trivial to hide behind one.
    let meta = match crate::cargo_metadata(&["--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };

    let mut failed = false;
    for edge in FORBIDDEN_EDGES {
        // The count for the `ok` line is a second walk of the same tree `reaches` already took -
        // cheap here (one gate, run once) and it keeps `reaches` itself a one-question function a
        // fixture can call directly, rather than one that also has to hand back a count nothing
        // else needs.
        let tree = match transitive_names(&meta, edge.from, edge.edges) {
            Ok(names) => names,
            Err(message) => {
                eprintln!("xtask check-boundaries: {message}");
                failed = true;
                continue;
            }
        };
        let found = match reaches(&meta, edge) {
            Ok(found) => found,
            Err(message) => {
                eprintln!("xtask check-boundaries: {message}");
                failed = true;
                continue;
            }
        };
        if !found {
            println!(
                "xtask check-boundaries: ok - {} does not reach {} ({} crate(s) in its tree)",
                edge.from,
                edge.forbidden,
                tree.len()
            );
            continue;
        }
        failed = true;
        eprintln!(
            "xtask check-boundaries: FAILED - {} reaches {}, and it may not.",
            edge.from, edge.forbidden
        );
        eprintln!();
        eprintln!("  Why: {}", edge.why);
        eprintln!("  Do:  {}", edge.instead);
        eprintln!();
        eprintln!(
            "  `cargo tree -p {} -e {} --invert {}` names the edge.",
            edge.from,
            edge.edges.tree_flag(),
            edge.forbidden
        );
        eprintln!("  If the edge genuinely belongs, the entry in FORBIDDEN_EDGES is what has to");
        eprintln!("  go, and that is an architecture decision: it should be a visible diff with");
        eprintln!("  the argument in it, not a dependency somebody added on the way past.");
        eprintln!();
    }
    if failed { Verdict::Fail } else { Verdict::Pass }
}

/// Which way dependencies point: nothing framework-shaped is reachable from the domain.
fn dependency_direction() -> Verdict {
    // `--all-features` for the same reason every other gate uses it: adapters are default-off,
    // so the default graph is nearly empty and would hide exactly what this checks.
    let meta = match crate::cargo_metadata(&["--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };

    let tree = match transitive_names(&meta, DOMAIN, Edges::Every) {
        Ok(names) => names,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };

    let bad = violations(&tree);
    if bad.is_empty() {
        println!(
            "xtask check-boundaries: ok - {DOMAIN}'s whole tree is {} crate(s), all allowlisted",
            tree.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-boundaries: FAILED - {DOMAIN} reaches crates it may not:");
    for name in bad {
        eprintln!("  {name}");
    }
    eprintln!();
    eprintln!("The domain holds the types, and the port traits that arrive with the first");
    eprintln!("adapter. A dependency it can reach - directly or transitively - is one every");
    eprintln!("domain test pays for and one the hexagon leaks. If it genuinely belongs, add it");
    eprintln!("to ALLOWED_IN_DOMAIN with the reason: that is an architecture decision and");
    eprintln!("should be a visible diff.");
    Verdict::Fail
}

/// The conformance harness reaches no adapter, so a pack body cannot be written against one.
///
/// `harness` carries the argument, the disproof that produced this half, and what the walk does
/// NOT reach.
fn harness_reaches_no_adapter() -> Verdict {
    // The DEFAULT-feature walk. The harness's compile packs live behind a default-off `compile`
    // feature (`crates/sutura-conformance/Cargo.toml`), and `cargo metadata --all-features` would
    // activate it and hand this walk the compiler and the renderer as if they were the closure a
    // DATA adapter links. Whether that feature stays default-off and holds exactly its one set is
    // `harness::compile_feature`'s check - so PERMITTED continues to hold the closure a binding
    // actually links, which is the default one. The `[features]` declaration is present in cargo
    // metadata whether or not the feature is activated, so the shape check reads the same `meta`.
    let meta = match crate::cargo_metadata(&[]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };
    match harness::check(&meta) {
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            Verdict::Fail
        }
        Ok(report) if report.problems.is_empty() => {
            println!(
                "xtask check-boundaries: ok - the conformance harness reaches {} first-party crate(s) \
                 ({}) in a normal closure of {}",
                report.first_party.len(),
                report.first_party.join(", "),
                report.closure
            );
            Verdict::Pass
        }
        Ok(report) => {
            eprintln!("xtask check-boundaries: FAILED - the conformance harness reaches an adapter:");
            for problem in &report.problems {
                eprintln!("  {problem}");
            }
            eprintln!();
            harness::explain();
            Verdict::Fail
        }
    }
}

/// What the crossing looks like: a library's types and errors are a typed contract.
fn typed_surface() -> Verdict {
    match api_shape::check() {
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            Verdict::Fail
        }
        Ok(report) if report.problems.is_empty() => {
            println!(
                "xtask check-boundaries: ok - {} library source file(s), typed surface intact",
                report.files
            );
            Verdict::Pass
        }
        Ok(report) => {
            eprintln!("xtask check-boundaries: FAILED - a library crate's contract is not typed:");
            for problem in &report.problems {
                eprintln!("  {problem}");
            }
            eprintln!();
            api_shape::explain();
            Verdict::Fail
        }
    }
}

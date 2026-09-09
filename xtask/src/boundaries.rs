//! The architecture-boundary gate. SEVEN halves - four about which way dependencies point, two
//! about the driving port, and one about what the crossing looks like:
//!
//! * the domain crate acquires no framework dependency ([`dependency_direction`])
//! * a named crate cannot reach a named crate ([`forbidden_edges`])
//! * the conformance harness reaches no adapter ([`harness_reaches_no_adapter`], and `harness`) -
//!   an allowlist in [`ALLOWED_IN_DOMAIN`]'s shape, added after the rule was DISPROVED: an adapter
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
mod harness;
mod ports;

use std::collections::BTreeSet;

use crate::Verdict;

/// The complete transitive dependency tree `sutura-domain` is permitted.
///
/// Serde and thiserror, plus what their derive macros pull in. Anything else - an async
/// runtime, an HTTP client, a query engine, a TLS stack - makes the hexagon decoration and
/// makes every domain test pay for a framework build.
///
/// Adding a name here is an architecture decision. That is the point.
const ALLOWED_IN_DOMAIN: &[&str] = &[
    "serde",
    "serde_core",
    "serde_derive",
    "thiserror",
    "thiserror-impl",
    // The proc-macro chain the two derives above require.
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
    // The canonical form of a definition set, and its hash. These are here because a review showed
    // the digest cannot be computed anywhere else: while `PinnedDefinitions::pin` took the hashing
    // FUNCTION from its caller, safe public code could pass `|_| Ok(elsewhere)` and pair any digest
    // with any definitions - which is the whole invariant `PinnedDefinitions` exists to hold. The
    // hash has to be the domain's own or it is not a guarantee.
    //
    // The cost was measured rather than estimated. Twelve crates transitively, and **no new
    // lockfile entry**: every one of them was already compiled into the shipped binary through
    // `sutura-catalog-local`, which is where this code used to live. This moves an edge, not a
    // dependency. None is a framework - no runtime, no client, no engine - which is the line the
    // doc comment above actually draws.
    "serde_json",
    "sha2",
    // What those two actually compile, confirmed against `cargo tree -p sutura-domain
    // --all-features` rather than assumed: `sha2` brings `cfg-if`, `cpufeatures` and `digest`
    // (which brings `block-buffer`, `crypto-common`, `hybrid-array`, `typenum`), and `serde_json`
    // brings `itoa`, `memchr` and `zmij`.
    "block-buffer",
    "cfg-if",
    "cpufeatures",
    "crypto-common",
    "digest",
    "hybrid-array",
    "itoa",
    "memchr",
    "typenum",
    "zmij",
    // Below here is a DIFFERENT KIND OF ENTRY, and the difference is worth keeping visible.
    //
    // `cargo tree -p sutura-domain --all-features` does not list one of these. They are in this
    // list because this gate walks `cargo metadata`'s whole-workspace resolve graph, which
    // includes every optional edge any crate in the workspace enables and every target's
    // platform-specific ones. So they are what the domain links against when the WORKSPACE is
    // built, not what it needs:
    //
    //   * the `indexmap` stack arrives because `utoipa` enables `serde_json/preserve_order` for
    //     deterministic OpenAPI output, and feature unification applies that to the one
    //     `serde_json` in the graph - including the domain's;
    //   * `const-oid` is `digest`'s optional `oid` feature, on the same mechanism;
    //   * `libc` is declared by `cpufeatures` for `aarch64-linux` only, and appears because
    //     metadata resolves every target rather than the one being built.
    //
    // None of them is reachable from `sutura-domain`'s own code, and the fast inner loop
    // `AGENTS.md` cites - `cargo check -p sutura-domain --no-default-features` - does not compile
    // them. Left allowlisted rather than filtered out of the gate: a gate that reasoned about
    // which edges are "really" enabled would be a second, subtler feature resolver, and being
    // over-broad here fails safe. If one of these ever becomes a framework, this list is where
    // the argument happens.
    "allocator-api2",
    "const-oid",
    "equivalent",
    "foldhash",
    "hashbrown",
    "indexmap",
    "libc",
    // The Postgres driver's SCRAM client enables `digest`'s `mac` feature, which pulls its two
    // constant-time helpers into the one `digest` the workspace shares - the same whole-workspace
    // feature unification as `const-oid` above. `sutura-domain` hashes with `sha2` and calls none
    // of this: the inner loop is `cargo check -p sutura-domain --no-default-features`, which
    // compiles neither.
    "cmov",
    "ctutils",
    // The credential type, and the only entry here taken for a COMPILE ERROR rather than for a value
    // the domain has to compute. `docs/adr/0020` is the decision.
    //
    // `identity::Secret` was `Secret(String)` with a hand-written `Display` printing `REDACTED`, and
    // that left `format!("{token}")` and `tracing::info!(%token)` compiling - a redacted line where
    // an author believed a value was logged. `secrecy::SecretString` has no `Display` and no
    // `PartialEq`, so both of those and `==` stop building. Zeroize-on-drop arrives with it.
    //
    // **Two crates, and the second is why this is a decision rather than a convenience.** `secrecy`
    // is `forbid(unsafe_code)` and pulls only `zeroize`, with `default-features = false, features =
    // ["alloc"]`, so nothing further follows - measured against `cargo tree -p sutura-domain
    // --all-features` rather than assumed. `zeroize` DOES contain `unsafe`: volatile writes and a
    // compiler fence, which is precisely the thing a workspace with `unsafe_code = "forbid"` cannot
    // write for itself and should not try to. Neither is a framework - no runtime, no client, no
    // engine - which is the line this list's doc comment draws.
    //
    // `secrecy`'s `serde` feature is OFF, and the manifests say why at length: it is what would give
    // `SecretBox` a `Deserialize`. That is a supply-chain decision and not the mechanism - `Secret`
    // is a newtype that derives nothing, so feature unification cannot hand it one.
    "secrecy",
    "zeroize",
    // And one entry of the OVER-BROAD kind the block above describes, arriving with the same
    // decision. `zeroize_derive` is `zeroize`'s optional `derive` feature; nothing in this workspace
    // turns it on, so `cargo tree -p sutura-domain --all-features` does not list it and
    // `cargo check -p sutura-domain --no-default-features` does not compile it. It is here because
    // this gate walks the whole-workspace resolve graph rather than reasoning about which edges a
    // feature resolver would really enable - and **the gate found it rather than a reader**: adding
    // `secrecy` with `zeroize` alone failed `check-boundaries` by name, which is what the list is for.
    // Its own tree is `proc-macro2`, `quote` and `syn`, all already above for the serde derives.
    "zeroize_derive",
];

const DOMAIN: &str = "sutura-domain";

/// One crate that must not be reachable from another, and what to do about it.
struct ForbiddenEdge {
    /// The crate whose transitive tree is walked.
    from: &'static str,
    /// The crate that must not appear in it.
    forbidden: &'static str,
    /// Why the edge is forbidden. Printed, because a rule whose reason is unstated gets
    /// reverted by the next person who needs the edge for twenty minutes.
    why: &'static str,
    /// What to do instead. Printed, because a gate that only says "no" gets worked around.
    instead: &'static str,
}

/// Edges that must stay absent.
///
/// **Adding, removing or widening an entry here is an architecture decision. That is the
/// point** - the same sentence [`ALLOWED_IN_DOMAIN`] carries, for the same reason: the diff is
/// where the argument happens.
const FORBIDDEN_EDGES: &[ForbiddenEdge] = &[
    // Rendering is not the compiler's business, and this is the half that a comment could not
    // hold. `compile` already stopped at a `QueryPlan` - the `Warehouse` port carries a plan, so
    // an adapter that executes over Arrow renders nothing - but `generate` and `dialect` were
    // still `pub` modules OF the core. The consequence was in the closure rather than in the
    // call graph: `sutura-serve -> sutura-http -> sutura-app -> sutura-semantic -> polyglot-sql`
    // put a pre-1.0 SQL generator, with three enumerated lowering gaps, into the network binary,
    // which renders nothing and can reach none of it.
    ForbiddenEdge {
        from: "sutura-semantic",
        forbidden: "polyglot-sql",
        why: "the core compiles a question into a plan and renders nothing. A SQL generator in \
              its tree is one every consumer of the core links, including a build that only ever \
              executes plans on the engine",
        instead: "put the rendering in `sutura-sql` and depend on THAT from the SQL adapter that \
                  needs it. `sutura-exec-duckdb` and `sutura-cli` do",
    },
    // The re-entry path, and the reason this is two entries rather than one. Nothing stops
    // somebody adding `sutura-sql` to `sutura-semantic`'s manifest to "share" a type - and that
    // reintroduces the edge above transitively, with no line naming `polyglot-sql` anywhere for a
    // reviewer to notice.
    ForbiddenEdge {
        from: "sutura-semantic",
        forbidden: "sutura-sql",
        why: "it is the same edge one hop further out: `sutura-sql` carries the generator, so \
              reaching it puts the generator back in the core's closure",
        instead: "the two crates are siblings and neither needs the other. If a type genuinely \
                  belongs to both, it belongs in `sutura-domain`, which is where `QueryPlan` and \
                  `ParamValue` already are. A type only the renderer uses belongs in `sutura-sql`, \
                  which is where `GeneratedQuery` went",
    },
];

/// Which dependency edges a walk follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edges {
    /// Every edge cargo resolved, dev- and build-dependencies included.
    ///
    /// The right answer for the two halves below, and deliberately so: `ALLOWED_IN_DOMAIN` is
    /// about what the WORKSPACE build can reach, and an edge moved behind a feature or a
    /// dev-dependency is still an edge for that question.
    Every,
    /// Normal dependencies only.
    ///
    /// What the adapter-class half needs, and the difference is not a detail: a dev-dependency
    /// between two adapters is how a corpus reaches a real data system, so following dev edges
    /// there would forbid the differential suite. `adapters` carries the argument.
    Normal,
}

impl Edges {
    /// Does this walk follow this dependency?
    fn follows(self, dep: &serde_json::Value) -> bool {
        match self {
            Self::Every => true,
            // A dependency with no `dep_kinds` at all is older metadata than this repo produces;
            // reading it as normal keeps the walk over-broad, which fails safe for a rule that
            // forbids an edge.
            Self::Normal => dep
                .get("dep_kinds")
                .and_then(|kinds| kinds.as_array())
                .is_none_or(|kinds| kinds.iter().any(is_normal_kind)),
        }
    }
}

/// Is this one `dep_kinds` entry a normal dependency? `null` is normal; the other two are
/// spelled `dev` and `build`.
fn is_normal_kind(kind: &serde_json::Value) -> bool {
    kind.get("kind").is_none_or(serde_json::Value::is_null)
}

/// Every package name reachable from `start` over the edges `edges` follows.
fn transitive_names(meta: &serde_json::Value, start: &str, edges: Edges) -> Result<BTreeSet<String>, String> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let nodes = meta
        .get("resolve")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `resolve.nodes`"))?;

    let name_of = |id: &str| -> Option<String> {
        packages.iter().find_map(|p| {
            (p.get("id").and_then(|i| i.as_str()) == Some(id))
                .then(|| p.get("name")?.as_str().map(String::from))
                .flatten()
        })
    };
    let start_id = packages
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(start))
        .and_then(|p| p.get("id")?.as_str())
        .ok_or_else(|| format!("{start} not found in workspace metadata"))?;

    let deps_of = |id: &str| -> Vec<String> {
        nodes
            .iter()
            .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(id))
            .and_then(|n| n.get("deps")?.as_array())
            .map(|deps| {
                deps.iter()
                    .filter(|dep| edges.follows(dep))
                    .filter_map(|d| d.get("pkg")?.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };

    // Iterative, so a dependency cycle cannot blow the stack.
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![String::from(start_id)];
    while let Some(current) = stack.pop() {
        for dep in deps_of(&current) {
            if seen_ids.insert(dep.clone()) {
                if let Some(name) = name_of(&dep) {
                    names.insert(name);
                }
                stack.push(dep);
            }
        }
    }
    Ok(names)
}

/// Names in `tree` that the allowlist does not permit.
fn violations(tree: &BTreeSet<String>) -> Vec<&String> {
    tree.iter()
        .filter(|name| !ALLOWED_IN_DOMAIN.contains(&name.as_str()))
        .collect()
}

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
    let halves = [direction, edges, packs, classes, declared, through, surface];
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
/// Written as early returns rather than as its siblings' one `match`, because it has FOUR ways to
/// fail and two of them are the rule reading nothing: the door it forbids no longer being defined,
/// and no caller naming the application at all. The green line names both of the things those two
/// check, so a reader can tell a pass from a vacuous one without running anything.
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
        "xtask check-boundaries: ok - the answer path is reached through the port ({} path(s) in {} file(s) in {}, door at {}:{door})",
        report.paths,
        report.files,
        report.callers.join(", "),
        answer_path::APPLICATION_LIB
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
        // Walked per entry rather than once, because `from` differs per rule and a missing
        // `from` has to be an error rather than a vacuous pass: a renamed crate would
        // otherwise silently switch the rule off.
        let tree = match transitive_names(&meta, edge.from, Edges::Every) {
            Ok(names) => names,
            Err(message) => {
                eprintln!("xtask check-boundaries: {message}");
                failed = true;
                continue;
            }
        };
        if !tree.contains(edge.forbidden) {
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
            "  `cargo tree -p {} -e normal --invert {}` names the edge.",
            edge.from, edge.forbidden
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
    // `--all-features`, for [`dependency_direction`]'s reason and one of its own: a pack family
    // behind a feature is still a pack family, and an edge moved behind one would be trivial to
    // hide from a default-feature walk.
    let meta = match crate::cargo_metadata(&["--all-features"]) {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ALLOWED_IN_DOMAIN, Edges, FORBIDDEN_EDGES, transitive_names, violations};

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    #[test]
    fn an_allowlisted_tree_has_no_violations() {
        assert!(
            violations(&set(ALLOWED_IN_DOMAIN)).is_empty(),
            "an allowlisted tree yields no violations"
        );
    }

    #[test]
    fn a_framework_anywhere_in_the_tree_is_a_violation() {
        // The case a denylist over DIRECT dependencies missed: reached transitively, and not
        // a name anybody had thought to forbid.
        let tree = set(&["serde", "tower", "rustls"]);
        let found = violations(&tree);
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn the_walk_is_transitive() {
        // b is only reachable through a; a denylist reading declared deps would not see it.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "root", "name": "sutura-domain"},
                    {"id": "a-id", "name": "a"},
                    {"id": "b-id", "name": "b"}
                ],
                "resolve": {"nodes": [
                    {"id": "root", "deps": [{"pkg": "a-id"}]},
                    {"id": "a-id", "deps": [{"pkg": "b-id"}]},
                    {"id": "b-id", "deps": []}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let tree = transitive_names(&meta, "sutura-domain", Edges::Every).expect("walk succeeds");
        assert_eq!(tree, set(&["a", "b"]));
    }

    #[test]
    fn a_cycle_terminates() {
        // Cargo will not produce one, but an iterative walk should not depend on that.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "root", "name": "sutura-domain"},
                    {"id": "a-id", "name": "a"}
                ],
                "resolve": {"nodes": [
                    {"id": "root", "deps": [{"pkg": "a-id"}]},
                    {"id": "a-id", "deps": [{"pkg": "root"}]}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let tree = transitive_names(&meta, "sutura-domain", Edges::Every).expect("walk succeeds");
        assert!(tree.contains("a"));
    }

    #[test]
    fn a_missing_crate_is_an_error_not_a_pass() {
        let meta: serde_json::Value = serde_json::from_str(r#"{"packages": [], "resolve": {"nodes": []}}"#).expect("parses");
        drop(transitive_names(&meta, "sutura-domain", Edges::Every).unwrap_err());
    }

    #[test]
    fn a_forbidden_edge_is_caught_transitively() {
        // The case a manifest grep misses, and the one the second entry in `FORBIDDEN_EDGES`
        // exists for: `sutura-semantic` names `sutura-sql`, `sutura-sql` names the generator, and
        // no line anywhere in the core's manifest says `polyglot-sql`.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "sem", "name": "sutura-semantic"},
                    {"id": "sql", "name": "sutura-sql"},
                    {"id": "pg", "name": "polyglot-sql"}
                ],
                "resolve": {"nodes": [
                    {"id": "sem", "deps": [{"pkg": "sql"}]},
                    {"id": "sql", "deps": [{"pkg": "pg"}]},
                    {"id": "pg", "deps": []}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let tree = transitive_names(&meta, "sutura-semantic", Edges::Every).expect("walk succeeds");
        for edge in FORBIDDEN_EDGES {
            assert!(tree.contains(edge.forbidden), "{} was not seen in the tree", edge.forbidden);
        }
    }

    #[test]
    fn every_forbidden_edge_says_what_to_do_instead() {
        // A gate that only says "no" gets worked around, so the message is part of the rule
        // rather than a courtesy. Asserted rather than reviewed: an entry added with an empty
        // `instead` prints a blank line where the fix should be.
        for edge in FORBIDDEN_EDGES {
            assert!(
                !edge.from.is_empty() && !edge.forbidden.is_empty(),
                "an edge names two crates"
            );
            assert!(!edge.why.is_empty(), "{} -> {} has no reason", edge.from, edge.forbidden);
            assert!(!edge.instead.is_empty(), "{} -> {} has no fix", edge.from, edge.forbidden);
        }
    }
}

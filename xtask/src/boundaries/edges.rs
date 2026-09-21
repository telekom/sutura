//! The domain allowlist, the forbidden-edge denylist, and the `cargo metadata` walk that
//! judges a crate's tree against them. The two shapes and why they differ are
//! `super`'s own module doc.

use std::collections::BTreeSet;

/// The complete transitive dependency tree `sutura-domain` is permitted.
///
/// Serde and thiserror, plus what their derive macros pull in. Anything else - an async
/// runtime, an HTTP client, a query engine, a TLS stack - makes the hexagon decoration and
/// makes every domain test pay for a framework build.
///
/// Adding a name here is an architecture decision. That is the point.
pub(crate) const ALLOWED_IN_DOMAIN: &[&str] = &[
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
    // ============================================================================================
    // ARROW AT THE INTERIOR - `docs/adr/0039`, by owner instruction, and the largest single entry
    // this list has taken. It reverses `docs/adr/0007`'s `RowSet`-at-the-port decision and the
    // unmerged 0037's refusal of Arrow as domain vocabulary.
    //
    // **The argument is this list's own line - no runtime, no client, no engine.** Arrow is a data
    // FORMAT. The engine resolves it through `datafusion`, the ADBC driver manager through
    // `adbc_core`, and any Arrow Flight leg will too; all three are on major 59, so a batch crosses
    // from any of them into `sutura_domain::warehouse::arrow` with no conversion and no C data
    // interface - which `unsafe_code = "forbid"` puts out of reach anyway. What it buys is ONE
    // Arrow-to-`Value` decode where there were three, and the one it deletes went through TEXT: a
    // cast to `Utf8`, a text cell, then a `parse::<i64>()` back, so an exact total had two chances
    // to stop being exact.
    //
    // **What is really compiled, and what is only in this graph.** `cargo tree -p sutura-domain
    // --all-features` lists TWENTY-TWO of the entries below: the four `arrow-*` crates, `chrono`
    // with `iana-time-zone` and `core-foundation-sys` (a time-zone database), `getrandom` and
    // `zerocopy`/`zerocopy-derive` via `ahash` via `hashbrown` (an entropy source), `half`, the
    // `num-*` family, `bytes`, `libm`, `once_cell`, `autocfg` and `version_check`. **Two of those
    // are worth naming rather than counting** - a time-zone database and an entropy source are now
    // in the hexagon's interior, reachable from no code this crate has: nothing here reads a zone
    // and nothing here seeds a generator. If either ever is read, that is a decision and this is
    // where it gets argued.
    //
    // Everything else below is the OVER-BROAD kind the `allocator-api2` block above describes -
    // `chrono-tz`'s zone tables and its `phf` maps, the `futures-*` set, the `wasm-bindgen` and
    // `js-sys` pair, the `windows-*` family, `android_system_properties`, `iana-time-zone-haiku`,
    // the `cc`/`jobserver`/`shlex`/`find-msvc-tools` build stack, `bitflags`, `bumpalo`,
    // `const-random`, `crunchy`, `log`, `pin-project-lite`, `r-efi`, `rand_core`, `rustversion`,
    // `siphasher`, `slab`, `tiny-keccak`, `wasi`/`wasip2`/`wit-bindgen`. They are optional edges
    // some other workspace crate enables, or platform-specific ones `cargo metadata` resolves for
    // every target. **The gate found every one of them rather than a reader did**, which is the
    // whole reason the list is an allowlist over the resolve graph and not a denylist.
    // ============================================================================================
    "ahash",
    "android_system_properties",
    "arrow-array",
    "arrow-buffer",
    "arrow-data",
    "arrow-schema",
    "autocfg",
    "bitflags",
    "bumpalo",
    "bytes",
    "cc",
    "chrono",
    "chrono-tz",
    "const-random",
    "const-random-macro",
    "core-foundation-sys",
    "crunchy",
    "find-msvc-tools",
    "futures-channel",
    "futures-core",
    "futures-io",
    "futures-macro",
    "futures-sink",
    "futures-task",
    "futures-util",
    "getrandom",
    "half",
    "iana-time-zone",
    "iana-time-zone-haiku",
    "jobserver",
    "js-sys",
    "libm",
    "log",
    "num-bigint",
    "num-complex",
    "num-integer",
    "num-traits",
    "once_cell",
    "phf",
    "phf_shared",
    "pin-project-lite",
    "r-efi",
    "rand_core",
    "rustversion",
    "shlex",
    "siphasher",
    "slab",
    "tiny-keccak",
    "version_check",
    "wasi",
    "wasip2",
    "wasm-bindgen",
    "wasm-bindgen-macro",
    "wasm-bindgen-macro-support",
    "wasm-bindgen-shared",
    "windows-core",
    "windows-implement",
    "windows-interface",
    "windows-link",
    "windows-result",
    "windows-strings",
    "wit-bindgen",
    "zerocopy",
    "zerocopy-derive",
];

pub(crate) const DOMAIN: &str = "sutura-domain";

/// One crate that must not be reachable from another, and what to do about it.
pub(crate) struct ForbiddenEdge {
    /// The crate whose transitive tree is walked.
    pub(crate) from: &'static str,
    /// The crate that must not appear in it.
    pub(crate) forbidden: &'static str,
    /// Why the edge is forbidden. Printed, because a rule whose reason is unstated gets
    /// reverted by the next person who needs the edge for twenty minutes.
    pub(crate) why: &'static str,
    /// What to do instead. Printed, because a gate that only says "no" gets worked around.
    pub(crate) instead: &'static str,
    /// Which edges the walk follows for THIS entry.
    ///
    /// Named per entry rather than fixed at [`Edges::Every`] for the whole table, because the two
    /// existing rules and the newest one make genuinely different claims: `sutura-catalog-rdbms`'s
    /// own comment argues `Edges::Every` on purpose - a test-only compile of the SQL generator is
    /// still the thing that rule forbids. A crate whose claim is about what a SHIPPED BINARY links
    /// (nothing dev-only ever ships) needs [`Edges::Normal`] instead, or a test-only tool with no
    /// bearing on the claim - `rcgen`'s own `ring` feature, needed to generate self-signed test
    /// certificates and nowhere near a shipped artifact - would keep the rule permanently red.
    pub(crate) edges: Edges,
}

/// Edges that must stay absent.
///
/// **Adding, removing or widening an entry here is an architecture decision. That is the
/// point** - the same sentence [`ALLOWED_IN_DOMAIN`] carries, for the same reason: the diff is
/// where the argument happens.
pub(crate) const FORBIDDEN_EDGES: &[ForbiddenEdge] = &[
    // Rendering is not the compiler's business, and this is the half that a comment could not
    // hold. `compile` already stopped at a `QueryPlan` - the `Warehouse` port carries a plan, so
    // an adapter that executes over Arrow renders nothing - but `generate` and `dialect` were
    // still `pub` modules OF the core. The consequence was in the closure rather than in the
    // call graph: `sutura-cli -> sutura-http -> sutura-app -> sutura-semantic -> polyglot-sql`
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
        edges: Edges::Every,
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
        edges: Edges::Every,
    },
    // The same closure argument from the metadata side, and it was nearly missed: a checkpoint
    // compiled `authored_sql:` fragments at catalog load, which needs `sutura_sql::expression` and
    // so put the generator into `sutura-catalog-local`'s tree. The shipped binary links that
    // adapter unconditionally, so the network binary - which the two entries above keep the
    // generator out of - would have linked it through the catalog instead, with every line of
    // prose saying it does not still in place. Nothing fired, because nothing forbade this edge.
    // `docs/adr/0004` records the decision this holds: an authored fragment is stored as written,
    // and compiling it belongs to the first execution adapter that executes it.
    ForbiddenEdge {
        from: "sutura-catalog-local",
        forbidden: "sutura-sql",
        why: "a catalog adapter loads metadata and renders nothing, and the shipped binary links \
              this one unconditionally - so the generator in its tree is the generator in the \
              network binary's default closure, which the `sutura-semantic` entries exist to prevent",
        instead: "store the authored fragment as `sutura_domain::expression::SqlFragment` and leave \
                  it uncompiled; the adapter that declares `Warehouse::EXECUTES_AUTHORED_SQL` is the \
                  one that compiles it, beside the renderer for its own dialect",
        edges: Edges::Every,
    },
    // The sibling catalog adapter, and the one `docs/adr/0016` names as the next candidate to mint
    // an authored computation (`metricInfo.expression`). Same class, same reason as the entry
    // above - but NOT the same closure claim: `sutura-catalog-datahub` is a dev-dependency of
    // `sutura-app` only, and an OPTIONAL dependency of `sutura-cli` behind the default-off
    // `datahub` feature (`cargo tree -p sutura-cli -e normal -i sutura-catalog-datahub` finds
    // nothing until that feature is asked for), so it is not unconditionally in either shipped
    // binary's default closure the way `sutura-catalog-local` is. The entry stands as the class
    // rule the `sutura-catalog-rdbms` entry below argues explicitly: a catalog adapter loads
    // metadata and renders nothing regardless of which dependency kind or feature gate links it.
    ForbiddenEdge {
        from: "sutura-catalog-datahub",
        forbidden: "sutura-sql",
        why: "a catalog adapter loads metadata and renders nothing - the class rule the entry above \
              states, which holds regardless of the optional or dev-only dependency kind that links \
              this one",
        instead: "what the entry above says: a fragment is stored, and the executing adapter compiles it",
        edges: Edges::Every,
    },
    // The rule above is about the class, not the one adapter (`sutura-catalog-local`) that is
    // unconditionally in the shipped binary's default closure. `sutura-catalog-rdbms` is a
    // DEV-dependency of `sutura-app` only - `cargo tree -e normal -i sutura-catalog-rdbms` reaches
    // nothing, the same as `sutura-catalog-datahub` above - so it is not in any shipped binary's
    // default closure either. The entry stands anyway, as the class rule rather than the closure
    // argument: a catalog adapter loads metadata and renders nothing regardless of which dependency
    // kind links it, and `Edges::Every` walks dev-dependencies for exactly that reason - a test-only
    // compile of the generator inside a catalog adapter's own tree is still refused.
    ForbiddenEdge {
        from: "sutura-catalog-rdbms",
        forbidden: "sutura-sql",
        why: "a catalog adapter loads metadata and renders nothing; the entry above says the rest",
        instead: "what the entry above says: a fragment is stored, and the executing adapter compiles it",
        edges: Edges::Every,
    },
    // `github.com/telekom/sutura#705` review finding 1: the crate's own doc claimed "no dependency
    // on a crypto provider" while its manifest named `rustls` - and this workspace's `rustls` entry
    // pins `features = ["ring", "tls12"]`, so that edge was `ring` under a different name. The
    // sentence exists because the whole reason this crate is a THIRD crate rather than a dependency
    // between the two adapters that need it is that it must stay usable by any future outbound
    // adapter without dragging a TLS implementation along - `docs/adr/0010`'s `security.outbound`
    // reuse case names a shipped `sutura-http` reader as one, and `nix/shipped.nix` bans `ring` from
    // every published binary. A recall-held sentence is not a control; this is the control.
    //
    // `Edges::Normal`, not `Edges::Every`: the claim is about what a SHIPPED BINARY links, and this
    // crate's own dev-dependency on `rcgen` (to generate self-signed certificates for its tests)
    // reaches `ring` through `rcgen`'s own feature - measured, `cargo tree -p sutura-tls -e
    // normal,build,dev -i ring` names exactly that edge and no other. Nothing dev-only ever ships,
    // so `Edges::Every` here would hold a permanently-red rule over a fact this claim is not about -
    // the same reasoning `sutura-catalog-rdbms`'s own entry gives for the opposite choice, because
    // that rule's claim genuinely is about a test-only compile.
    ForbiddenEdge {
        from: "sutura-tls",
        forbidden: "ring",
        why: "this crate's whole reason to exist is a bundle-or-system-store READ any outbound TLS \
              adapter can depend on without acquiring a crypto provider - a shipped reader is the \
              reuse case `docs/adr/0010` names, and `nix/shipped.nix` refuses `ring` in every \
              published binary",
        instead: "read the bytes with `rustls-pki-types` (`CertificateDer`, `PrivateKeyDer`, the \
                  `PemObject` reader) - the same types `rustls::pki_types` re-exports verbatim, so \
                  a `rustls`-depending caller converts nothing at the seam. Building a `ClientConfig` \
                  or a `RootCertStore` is each adapter's own job, with its own crypto provider",
        edges: Edges::Normal,
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

    /// The `cargo tree -e` value that reproduces this same walk, for a refusal's own remedy line.
    ///
    /// A fixed `normal` printed for every entry (as `forbidden_edges`'s refusal used to) is wrong
    /// for an `Edges::Every` entry whose forbidden crate is reachable only through a dev or build
    /// edge - `sutura-tls -> ring` is exactly that shape, and `cargo tree -e normal --invert ring`
    /// prints nothing for it while the gate correctly refuses.
    pub(crate) const fn tree_flag(self) -> &'static str {
        match self {
            Self::Every => "normal,build,dev",
            Self::Normal => "normal",
        }
    }
}

/// Is this one `dep_kinds` entry a normal dependency? `null` is normal; the other two are
/// spelled `dev` and `build`.
fn is_normal_kind(kind: &serde_json::Value) -> bool {
    kind.get("kind").is_none_or(serde_json::Value::is_null)
}

/// Every package name reachable from `start` over the edges `edges` follows.
pub(crate) fn transitive_names(meta: &serde_json::Value, start: &str, edges: Edges) -> Result<BTreeSet<String>, String> {
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
pub(crate) fn violations(tree: &BTreeSet<String>) -> Vec<&String> {
    tree.iter()
        .filter(|name| !ALLOWED_IN_DOMAIN.contains(&name.as_str()))
        .collect()
}

/// Is `name` a package in this metadata's graph at all, workspace member or not?
///
/// [`super::second_workspace`]'s own reason: a declared satellite workspace need not contain every
/// crate a [`ForbiddenEdge`]'s `from` names - `fuzz/`'s own graph has no `sutura-semantic` package
/// unless something it depends on pulls it in - and that absence is not the rule going quiet the
/// way an EMPTY tree from the root workspace would be. It is a structural fact about a smaller
/// graph, so the caller skips the entry rather than asking [`transitive_names`] to error on it.
pub(crate) fn contains_package(meta: &serde_json::Value, name: &str) -> bool {
    meta.get("packages")
        .and_then(|p| p.as_array())
        .is_some_and(|packages| packages.iter().any(|p| p.get("name").and_then(|n| n.as_str()) == Some(name)))
}

/// Whether `edge.forbidden` is reachable from `edge.from`, over the edge kinds `edge.edges` names.
///
/// Factored out of [`super::forbidden_edges`] so a fixture can drive ONE entry's scoping directly: a
/// `dep_kinds: [{"kind": "dev"}]` route from `from` to `forbidden` must answer `true` for an
/// `Edges::Every` entry and `false` for an `Edges::Normal` one, which is the property the per-entry
/// `edges` field exists to hold and which `just lint` alone does not exercise both ways - the live
/// resolve graph only happens to make `Edges::Every` fail-visible today (`sutura-tls`'s `rcgen` dev
/// edge to `ring`), and nothing in the graph currently makes `Edges::Normal` fail-visible at all.
pub(crate) fn reaches(meta: &serde_json::Value, edge: &ForbiddenEdge) -> Result<bool, String> {
    let tree = transitive_names(meta, edge.from, edge.edges)?;
    Ok(tree.contains(edge.forbidden))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        ALLOWED_IN_DOMAIN, Edges, FORBIDDEN_EDGES, ForbiddenEdge, contains_package, reaches, transitive_names, violations,
    };

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    #[test]
    fn contains_package_answers_for_a_package_the_workspace_does_not_have_to_be_a_member_of() {
        let meta: serde_json::Value =
            serde_json::from_str(r#"{"packages": [{"id": "a", "name": "sutura-fuzz"}]}"#).expect("fixture parses");
        assert!(contains_package(&meta, "sutura-fuzz"));
        assert!(!contains_package(&meta, "sutura-semantic"));
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
        // One fixture asserting over every REAL entry in `FORBIDDEN_EDGES`, so a `forbidden` name
        // this fixture's graph does not reach is a fixture gap this test itself would name rather
        // than an edge the walk quietly missed - it is why `sutura-tls`/`ring` are wired in here
        // too, reachable transitively (through `tls`) rather than declared directly on `sem`, which
        // is the whole shape this test is about.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "sem", "name": "sutura-semantic"},
                    {"id": "sql", "name": "sutura-sql"},
                    {"id": "pg", "name": "polyglot-sql"},
                    {"id": "tls", "name": "sutura-tls"},
                    {"id": "ring", "name": "ring"}
                ],
                "resolve": {"nodes": [
                    {"id": "sem", "deps": [{"pkg": "sql"}, {"pkg": "tls"}]},
                    {"id": "sql", "deps": [{"pkg": "pg"}]},
                    {"id": "pg", "deps": []},
                    {"id": "tls", "deps": [{"pkg": "ring"}]},
                    {"id": "ring", "deps": []}
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
    fn a_forbidden_edges_kind_scoping_is_held_by_a_fixture() {
        // `reaches` is the per-entry question `forbidden_edges` asks, and the field it reads
        // (`edge.edges`) had no test of its own before this one: the fixture above always walks a
        // hardcoded `Edges::Every` from a hardcoded root, so it proves a NAME is reachable and
        // never that one entry's OWN `edges` choice is respected. One DEV-only route from `from`
        // to `forbidden` (the shape `boundaries::adapters`'s own fixtures build) answers both
        // directions at once: an `Edges::Every` entry must still catch it, and an `Edges::Normal`
        // entry - `sutura-tls`'s own claim, since its `rcgen` dev-dependency reaching `ring` is
        // exactly the route that must NOT trip a claim about what a shipped binary links - must not.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "from-id", "name": "from"},
                    {"id": "forbidden-id", "name": "forbidden"}
                ],
                "resolve": {"nodes": [
                    {"id": "from-id", "deps": [{"pkg": "forbidden-id", "dep_kinds": [{"kind": "dev"}]}]},
                    {"id": "forbidden-id", "deps": []}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let every = ForbiddenEdge {
            from: "from",
            forbidden: "forbidden",
            why: "test fixture",
            instead: "test fixture",
            edges: Edges::Every,
        };
        let normal = ForbiddenEdge {
            from: "from",
            forbidden: "forbidden",
            why: "test fixture",
            instead: "test fixture",
            edges: Edges::Normal,
        };
        assert!(
            reaches(&meta, &every).expect("walk succeeds"),
            "an Edges::Every entry must still catch a dev-only route"
        );
        assert!(
            !reaches(&meta, &normal).expect("walk succeeds"),
            "an Edges::Normal entry must not be tripped by a dev-only route"
        );
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

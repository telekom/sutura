//! Each half's WRAPPER (`cargo metadata` -> `check` -> printed verdict) driven over a repository
//! holding exactly one violation of that half's own rule. The submodule cells call `check`
//! directly, so a wrapper that answered `Pass` unconditionally left every one of them green.
//!
//! Two shapes. The graph halves read only `cargo metadata`, which is seeded into
//! [`crate::MetadataRun`]: no cargo runs and no working directory moves. The rest read source too,
//! and resolve it through `repo::root`, so they run inside a scratch repository the process has
//! moved into (nextest only, the guard `falsifier` carries and for its reason). Each cell runs its
//! half twice and asserts `(Pass, Fail)`: the control, the same without the violation, must pass,
//! because a `Fail` there would be the fixture and not the rule.

use super::edges::FORBIDDEN_EDGES;
use super::shared_client::ROWS;
use super::{
    adapter_classes, answer_through_the_port, composition_root, declared_ports, forbidden_edges, harness_reaches_no_adapter,
    no_adapter_in_application, no_adapter_in_shared_client, second_workspaces, typed_surface, ungoverned,
};
use crate::scratch_tree::Tree;
use crate::{METADATA_RUN, MetadataRun, Verdict};

const DOMAIN: &str = "sutura-domain";
const APP: &str = "sutura-app";
const HARNESS: &str = "sutura-conformance";
const EXEC_A: &str = "sutura-exec-a";
const EXEC_B: &str = "sutura-exec-b";

/// A resolve graph over every crate the graph halves name, with the harness's own feature shape and
/// the one edge the harness rule requires, plus `extra` normal edges.
fn graph(extra: &[(&str, &str)]) -> serde_json::Value {
    let mut names: Vec<&str> = vec![
        DOMAIN,
        APP,
        HARNESS,
        EXEC_A,
        EXEC_B,
        "sutura-catalog-a",
        "sutura-http",
        "sutura-mcp",
    ];
    names.extend(ROWS.iter().map(|row| row.watched));
    names.extend(FORBIDDEN_EDGES.iter().flat_map(|edge| [edge.from, edge.forbidden]));
    names.sort_unstable();
    names.dedup();
    let edges: Vec<(&str, &str)> = std::iter::once((HARNESS, DOMAIN)).chain(extra.iter().copied()).collect();
    let packages: Vec<serde_json::Value> = names
        .iter()
        .map(|name| {
            let features = if *name == HARNESS {
                serde_json::json!({"default": [], "compile": ["dep:sutura-semantic", "dep:sutura-sql", "dep:serde_json"]})
            } else {
                serde_json::json!({"default": []})
            };
            serde_json::json!({"id": format!("id-{name}"), "name": name, "features": features})
        })
        .collect();
    let nodes: Vec<serde_json::Value> = names
        .iter()
        .map(|name| {
            let deps: Vec<serde_json::Value> = edges
                .iter()
                .filter(|(from, _)| from == name)
                .map(|(_, to)| serde_json::json!({"pkg": format!("id-{to}"), "dep_kinds": [{"kind": null}]}))
                .collect();
            serde_json::json!({"id": format!("id-{name}"), "deps": deps})
        })
        .collect();
    let members: Vec<String> = names
        .iter()
        .filter(|name| name.starts_with("sutura-"))
        .map(|name| format!("id-{name}"))
        .collect();
    serde_json::json!({"packages": packages, "workspace_members": members, "resolve": {"nodes": nodes}})
}

/// A run whose every `cargo metadata` answer is `meta`.
fn seed(meta: &serde_json::Value) -> MetadataRun {
    let run = MetadataRun::open();
    METADATA_RUN.with_borrow_mut(|answers| {
        let answers = answers.as_mut().expect("an open run");
        for key in ["", "--all-features", "--no-deps", "--no-deps --all-features"] {
            answers.insert(String::from(key), Ok(meta.clone()));
        }
    });
    run
}

/// `half` over the clean graph, then over the graph with `extra` edges added.
fn over_graph(extra: &[(&str, &str)], half: fn() -> Verdict) -> (Verdict, Verdict) {
    let control = {
        let _run = seed(&graph(&[]));
        half()
    };
    let violated = {
        let _run = seed(&graph(extra));
        half()
    };
    (control, violated)
}

#[test]
fn forbidden_edges_refuses_one_forbidden_edge() {
    let row = &FORBIDDEN_EDGES[0];
    assert_eq!(
        over_graph(&[(row.from, row.forbidden)], forbidden_edges),
        (Verdict::Pass, Verdict::Fail),
        "{} -> {} must be refused over a graph that is otherwise clean",
        row.from,
        row.forbidden
    );
}

#[test]
fn adapter_classes_refuses_one_edge_inside_a_class() {
    assert_eq!(
        over_graph(&[(EXEC_A, EXEC_B)], adapter_classes),
        (Verdict::Pass, Verdict::Fail),
        "one data-system adapter reaching another must be refused"
    );
}

#[test]
fn harness_refuses_one_adapter_in_its_closure() {
    assert_eq!(
        over_graph(&[(HARNESS, EXEC_A)], harness_reaches_no_adapter),
        (Verdict::Pass, Verdict::Fail),
        "the harness reaching an adapter must be refused"
    );
}

#[test]
fn application_refuses_one_adapter_edge() {
    assert_eq!(
        over_graph(&[(APP, EXEC_A)], no_adapter_in_application),
        (Verdict::Pass, Verdict::Fail),
        "the application reaching an adapter must be refused"
    );
}

#[test]
fn shared_client_refuses_one_adapter_edge() {
    let watched = ROWS[0].watched;
    assert_eq!(
        over_graph(&[(watched, EXEC_A)], no_adapter_in_shared_client),
        (Verdict::Pass, Verdict::Fail),
        "{watched} reaching an adapter must be refused"
    );
}

type Files<'a> = &'a [(&'a str, &'a str)];

/// Puts the working directory back when the cell leaves it, a panic included.
struct Moved(std::path::PathBuf);

impl Drop for Moved {
    fn drop(&mut self) {
        drop(std::env::set_current_dir(&self.0));
    }
}

/// The workspace `cargo metadata --no-deps` of the scratch repository: the application, and one
/// caller of it, each with a library target under `root`.
fn workspace(root: &std::path::Path) -> serde_json::Value {
    let member = |name: &str, deps: &[&str]| {
        let dir = root.join("crates").join(name);
        serde_json::json!({
            "name": name,
            "manifest_path": dir.join("Cargo.toml"),
            "targets": [{"kind": ["lib"], "src_path": dir.join("src/lib.rs")}],
            "dependencies": deps.iter().map(|dep| serde_json::json!({"name": dep, "kind": null})).collect::<Vec<_>>(),
        })
    };
    serde_json::json!({"packages": [member(APP, &[]), member("sutura-http", &[APP])]})
}

/// `half` with the process inside a scratch repository of `files`, and the metadata answers
/// seeded from it. Moves the working directory, so it refuses without nextest's own process.
fn run_over(tag: &str, files: Files<'_>, half: fn() -> Verdict) -> Verdict {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this test moves the process's current directory: run it under `just test`"
    );
    let markers: Files<'_> = &[("flake.nix", ""), ("Cargo.toml", "")];
    let fixtures: Vec<_> = markers
        .iter()
        .chain(files)
        .map(|(rel, body)| (*rel, body.as_bytes()))
        .collect();
    let tree = Tree::of(tag, &fixtures);
    // Canonical, because the working directory reads back resolved (`/var` is `/private/var` on
    // macOS) and a package path under the other spelling is not under `repo::root`.
    let root = tree.root().canonicalize().expect("the scratch repository exists");
    let _back = Moved(std::env::current_dir().expect("a current directory"));
    std::env::set_current_dir(&root).expect("enter the scratch repository");
    let _run = seed(&workspace(&root));
    half()
}

/// `half` over `base`, then over `base` with `edit` written over (or added beside) its file.
fn over_repo(tag: &str, base: Files<'_>, edit: (&str, &str), half: fn() -> Verdict) -> (Verdict, Verdict) {
    let edited: Vec<(&str, &str)> = base.iter().copied().filter(|(rel, _)| *rel != edit.0).chain([edit]).collect();
    (run_over(tag, base, half), run_over(tag, &edited, half))
}

const ANSWER_DOOR: (&str, &str) = ("crates/sutura-app/src/lib.rs", "pub fn answer() {}\n");
const RAW_DOOR: (&str, &str) = ("crates/sutura-app/src/raw.rs", "pub fn run_sql() {}\n");
const CALLER_LIB: &str = "crates/sutura-http/src/lib.rs";
const CALLER: &str = "pub trait KeySetSource {}\nuse sutura_app::surface::Surface as _;\n";
const APPLICATION_TREE: Files<'static> = &[ANSWER_DOOR, RAW_DOOR, (CALLER_LIB, CALLER)];

#[test]
fn driving_port_refuses_one_trait_declared_by_a_caller() {
    assert_eq!(
        over_repo(
            "wrap-ports",
            APPLICATION_TREE,
            (
                CALLER_LIB,
                "pub trait KeySetSource {}\npub trait Surface {}\nuse sutura_app::surface::Surface as _;\n"
            ),
            declared_ports
        ),
        (Verdict::Pass, Verdict::Fail),
        "a caller of the driving port declaring `pub trait Surface` must be refused"
    );
}

#[test]
fn answer_path_refuses_one_caller_bypassing_the_port() {
    assert_eq!(
        over_repo(
            "wrap-answer",
            APPLICATION_TREE,
            (
                CALLER_LIB,
                "pub trait KeySetSource {}\nfn bypass() { let _ = sutura_app::answer(); }\n"
            ),
            answer_through_the_port
        ),
        (Verdict::Pass, Verdict::Fail),
        "a caller naming `sutura_app::answer` in its own source must be refused"
    );
}

#[test]
fn typed_surface_refuses_one_public_field() {
    assert_eq!(
        over_repo(
            "wrap-surface",
            APPLICATION_TREE,
            (
                ANSWER_DOOR.0,
                "pub fn answer() {}\npub struct Loose {\n    pub field: u32,\n}\n"
            ),
            typed_surface
        ),
        (Verdict::Pass, Verdict::Fail),
        "a library struct with a `pub` field must be refused"
    );
}

const CLI_LIB: &str = "crates/sutura-cli/src/lib.rs";
const MOUNT_TREE: Files<'static> = &[
    ("crates/sutura-http/src/lib.rs", "pub fn f() {}\n"),
    (CLI_LIB, "pub fn g() {}\n"),
];

#[test]
fn ungoverned_mounts_refuse_one_mount_outside_the_mechanism() {
    assert_eq!(
        over_repo(
            "wrap-mounts",
            MOUNT_TREE,
            (CLI_LIB, "pub fn g() { let _ = app.fallback_service(x); }\n"),
            ungoverned::check
        ),
        (Verdict::Pass, Verdict::Fail),
        "a `.fallback_service(` outside `Ungoverned::mount` must be refused"
    );
}

const COMPOSITION_TREE: Files<'static> = &[
    (
        "crates/sutura-cli/Cargo.toml",
        "[package]\ndescription = \"The sutura binary. Composes adapters; contains no business logic.\"\n",
    ),
    ("crates/sutura-cli/src/import.rs", "pub fn import() {}\n"),
    (
        "crates/sutura-cli/src/serve/kind.rs",
        "#[derive(Debug, Error)]\npub enum AnyWarehouseError {}\n",
    ),
];

#[test]
fn composition_root_refuses_one_error_definition() {
    assert_eq!(
        over_repo(
            "wrap-root",
            COMPOSITION_TREE,
            ("crates/sutura-cli/src/import.rs", "#[derive(Debug, Error)]\nenum Stray {}\n"),
            composition_root::check
        ),
        (Verdict::Pass, Verdict::Fail),
        "an error defined in the composition root beside its declared wrapper must be refused"
    );
}

/// One declared satellite (`fuzz`) with a real, offline-resolvable graph: it reaches only its one
/// path dependency, a `sutura-domain` that has none.
const SATELLITE_TREE: Files<'static> = &[
    ("Cargo.toml", "[workspace]\nmembers = []\nexclude = [\"fuzz\", \"domain\"]\n"),
    (
        "fuzz/Cargo.toml",
        "[package]\nname = \"fuzz\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [dependencies]\nsutura-domain = { path = \"../domain\" }\n\n[workspace]\n",
    ),
    (
        "fuzz/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"fuzz\"\nversion = \"0.0.0\"\n\
         dependencies = [\n \"sutura-domain\",\n]\n\n[[package]]\nname = \"sutura-domain\"\nversion = \"0.0.0\"\n",
    ),
    ("fuzz/src/lib.rs", ""),
    (
        "domain/Cargo.toml",
        "[package]\nname = \"sutura-domain\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n",
    ),
    ("domain/src/lib.rs", ""),
];

#[test]
fn second_workspaces_refuse_one_undeclared_workspace() {
    assert_eq!(
        over_repo(
            "wrap-satellite",
            SATELLITE_TREE,
            ("sidecar/Cargo.toml", "[workspace]\n"),
            second_workspaces
        ),
        (Verdict::Pass, Verdict::Fail),
        "a manifest declaring its own `[workspace]` outside DECLARED must be refused"
    );
}

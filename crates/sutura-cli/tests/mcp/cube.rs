//! `examples/cube` over the stdio agent surface: a composed `sutura mcp` plans a question from the
//! example's markdown catalog and the provisioned Cube computes the metric through its REST API.
//!
//! `#[ignore]`d, behind `just cube-acceptance`: the tier is a docker service, and `just test` and
//! the nix sandbox do not start one.

use std::path::{Path, PathBuf};

use sutura_app::Capability;

use crate::harness::{recurring_revenue_june, settings_tree, spawn_configured};

/// What `examples/cube/conf/base.yaml` declares relative to the repository root, and for the
/// endpoint and the token, which this run replaces with absolute paths, the tier's published port
/// and a token it signed.
const EXAMPLE_CATALOG: &str = "\"examples/";
const EXAMPLE_ENDPOINT: &str = "http://127.0.0.1:4000";
const EXAMPLE_TOKEN_FILE: &str = "/run/sutura/cube-token";

/// The example's own settings, pointed at this checkout, this run's tier and its token file.
fn example_settings(endpoint: &str, token_file: &Path) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root resolves");
    let example = root.join("examples/cube/conf/base.yaml");
    let base = std::fs::read_to_string(&example).expect("the example's settings are readable");
    for declared in [EXAMPLE_CATALOG, EXAMPLE_ENDPOINT, EXAMPLE_TOKEN_FILE] {
        assert!(
            base.contains(declared),
            "{} no longer declares `{declared}`:\n{base}",
            example.display()
        );
    }
    settings_tree(
        "mcp-cube-example",
        &base
            .replace(EXAMPLE_CATALOG, &format!("\"{}/examples/", root.display()))
            .replace(EXAMPLE_ENDPOINT, endpoint)
            .replace(EXAMPLE_TOKEN_FILE, &token_file.to_string_lossy()),
    )
}

#[test]
#[ignore = "needs `just dev-up-cube` - run `just cube-acceptance`"]
fn the_cube_example_answers_a_metric_through_the_provisioned_cube() {
    let endpoint = sutura_dev::provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "cube")
        .endpoint()
        .map(|endpoint| format!("http://{endpoint}"))
        .expect("`just cube-acceptance` provisions the cube tier first");
    let token = sutura_exec_cube::fixture::tier_token().expect("the tier's token signs");
    let token_file = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-cube-token-{}", std::process::id()));
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture token is written where the deployment reads it"
    )]
    std::fs::write(&token_file, token.expose_secret()).expect("the token file is writable");

    let mut agent = spawn_configured(&example_settings(&endpoint, &token_file));
    drop(agent.initialize());
    let result = agent.call(Capability::AskMetric, &recurring_revenue_june());
    let _ignored = std::fs::remove_file(&token_file);

    assert_ne!(result["isError"], serde_json::json!(true), "{result}");
    let content = &result["structuredContent"];
    assert_eq!(content["outcome"], "answer", "{result}");
    assert_eq!(content["rows"], serde_json::json!([["2026-06-01", "202121"]]), "{result}");
}

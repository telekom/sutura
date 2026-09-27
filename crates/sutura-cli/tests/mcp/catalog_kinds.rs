//! Catalog kinds: a declared RDBMS catalog on a build without its feature refuses the process,
//! naming the same remedy `sutura serve` gives.

#[cfg(not(feature = "rdbms"))]
use crate::harness::{SOURCE, VERSION, example_root, settings_tree, spawn_configured};

/// Both composition roots refuse a declared RDBMS catalog when this build lacks its feature.
/// The provisioned catalog test exercises the live reader directly, not this MCP composition.
#[test]
#[cfg(not(feature = "rdbms"))]
fn a_declared_rdbms_catalog_build_without_the_feature_refuses_naming_it() {
    let example = example_root();
    let dir = settings_tree(
        "mcp-rdbms-refused",
        &format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"a unit test reads its \
             own fixture files as one identity\"\n\
             catalogs:\n  - {{name: \"model\", kind: \"rdbms\", dir: \"{}\", data_dir: \"{}\", version: \"{VERSION}\", \
             environment: \"test\", source_alias: \"{SOURCE}\", connection: {{host: \"127.0.0.1\", port: 5432, \
             database: \"dictionary\", user: \"reader\", password_file: \"/run/secrets/dictionary\", \
             transport_mode: \"plaintext\"}}}}\n\
             sources:\n  \
               {SOURCE}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{}\"\n    \
                 posture: \"shared-service-user\"\n",
            example.join("catalog").display(),
            example.join("data").display(),
            example.join("data").display(),
        ),
    );
    let mut agent = spawn_configured(&dir);
    // Blocking reads, not `drain_log` - the process refuses and exits before this test asks
    // anything, so there is a race between that exit and the background thread finishing its
    // forward of standard error; `expect_log` waits for each line rather than snapshotting
    // whatever has arrived so far.
    let refusal = agent.expect_log("catalog.kind: rdbms");
    assert!(
        refusal.contains("--features rdbms"),
        "a build without the feature names the remedy - the same message `sutura serve` gives: {refusal}"
    );
    let status = agent.close();
    assert!(
        !status.success(),
        "a deployment declaring catalog.kind: rdbms on a build without the feature must refuse rather than serve; standard error:\n{}",
        agent.drain_log().join("\n")
    );
}

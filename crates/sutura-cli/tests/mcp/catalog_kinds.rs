//! Catalog kinds: a declared catalog kind with no reader on any build refuses the process,
//! naming the same follow-up `sutura serve` gives.

use crate::harness::{SOURCE, VERSION, example_root, settings_tree, spawn_configured};

/// A declared catalog kind with no reader on ANY build (`rdbms`) refuses this process the same
/// way it refuses `sutura serve` - the operator-facing message issue #970 asks for, since both
/// composition roots dispatch the SAME `crate::catalog::open_catalog`. (`openmetadata`, the
/// other kind a default build does not link, gets its own not-linked refusal cell beside
/// `datahub`'s.) Before #970, this command ignored `catalogs:` entirely and opened only its
/// directory arguments, so a settings tree declaring `catalog.kind: rdbms` was never read at all.
#[test]
fn a_declared_catalog_kind_without_a_reader_refuses_the_process_naming_the_same_follow_up_serve_gives() {
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
        refusal.contains("#972"),
        "the refusal did not name the follow-up issue - the same message `sutura serve` gives: {refusal}"
    );
    let status = agent.close();
    assert!(
        !status.success(),
        "a deployment declaring catalog.kind: rdbms must refuse rather than serve; standard error:\n{}",
        agent.drain_log().join("\n")
    );
}

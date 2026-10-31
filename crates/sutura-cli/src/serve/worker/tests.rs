//! A worker built from its pair and node authority, with no `server.nodes` declared.

use crate::serve::tests::nodes::{runtime, written};

#[test]
fn a_worker_builds_from_its_pair_and_node_authority_with_no_nodes_declared() {
    let (certificate, key) = written("worker");
    let path = |path: &std::path::Path| path.display().to_string();
    let tls = sutura_config::TlsMaterial::parse(Some(&path(&certificate)), Some(&path(&key)), Some(&path(&certificate)))
        .expect("a pair and an authority are TLS material")
        .expect("material was declared");
    assert!(tls.nodes().is_none(), "this cell is about a worker with no server.nodes");
    let built = super::mount(&tls, &certificate, runtime());
    let _ignored = (std::fs::remove_file(&certificate), std::fs::remove_file(&key));
    let _mount = built.expect("a signed node with its authority builds the worker it serves");
}

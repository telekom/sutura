//! The worker pool's `workerPool.addressFamily`, read as the binary reads the serving pod's settings.

use sutura_config::{Environment, Settings, Sources, TlsMaterial};

use super::{Inputs, documents, pool_inputs, testdata};

/// The family the serving pod of the pool golden `render` loads for the nodes it declares.
fn family_of(render: &str) -> String {
    let golden = testdata().join(format!("golden/pool/{render}.yaml"));
    let text = std::fs::read_to_string(&golden).unwrap_or_else(|error| panic!("{} is readable: {error}", golden.display()));
    let Inputs {
        environment,
        directory,
        variables,
    } = pool_inputs(&documents(&text), "test-release");
    let environment = Environment::parse(&environment).unwrap_or_else(|error| panic!("{render}: {error}"));
    let directory = directory.expect("the serving pod reads a settings directory");
    let loaded = Settings::load(
        &Sources::defaults(environment)
            .with_variables(variables)
            .with_directory(directory.clone()),
    );
    std::fs::remove_dir_all(directory).expect("the scratch directory is removed");
    let settings = loaded.unwrap_or_else(|error| panic!("the binary refuses {render}: {:?}", error.reason()));
    let nodes = settings
        .server()
        .tls()
        .and_then(TlsMaterial::nodes)
        .expect("the serving pod declares nodes");
    nodes.address_family().to_string()
}

#[test]
fn the_worker_pools_address_family_reaches_the_serving_pods_nodes() {
    assert_eq!(family_of("worker-pool"), "auto", "an unset value leaves the binary's default");
    assert_eq!(family_of("worker-pool-ipv6"), "ipv6");
}

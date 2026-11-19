#![forbid(unsafe_code)]
//! The chart's generated multi-role venue carries DNS and Google settings to each data-plane pod.

#[cfg(test)]
mod tests {
    use serde::Deserialize as _;
    use serde_norway::Value;

    #[test]
    fn private_google_dns_and_destinations_reach_every_data_plane_role() {
        let rendered = include_str!("../../../charts/sutura/testdata/golden/pool/reading-workers.yaml");
        let documents: Vec<Value> = serde_norway::Deserializer::from_str(rendered)
            .map(|document| Value::deserialize(document).expect("chart YAML"))
            .collect();
        let mut roles = Vec::new();
        let mut reader_configs = 0;
        for document in &documents {
            if document["kind"].as_str() == Some("Deployment") {
                let pod = &document["spec"]["template"]["spec"];
                let name = pod["containers"][0]["name"].as_str().expect("container name");
                if ["sutura", "worker", "reading-worker"].contains(&name) {
                    assert_eq!(
                        pod["hostAliases"][0]["ip"].as_str(),
                        Some("192.0.2.10"),
                        "{name} lost its DNS override"
                    );
                    let hosts = pod["hostAliases"][0]["hostnames"].as_sequence().expect("hosts");
                    assert!(
                        hosts.iter().any(|host| host.as_str() == Some("sts-example.p.googleapis.com")),
                        "{name} lost STS DNS"
                    );
                    roles.push(name);
                }
            }
            if document["kind"].as_str() == Some("ConfigMap")
                && document["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.contains("-reading-"))
            {
                let settings: Value = serde_norway::from_str(document["data"]["base.yaml"].as_str().expect("reader settings"))
                    .expect("settings YAML");
                assert_eq!(
                    settings["google_endpoints"]["sts"].as_str(),
                    Some("https://sts-example.p.googleapis.com/v1/token")
                );
                reader_configs += 1;
            }
        }
        roles.sort_unstable();
        assert_eq!(roles, ["reading-worker", "reading-worker", "sutura", "worker"]);
        assert_eq!(reader_configs, 2);
    }
}

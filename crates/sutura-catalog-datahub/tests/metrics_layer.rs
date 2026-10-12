#![forbid(unsafe_code)]
//! `examples/metrics-layer` serves the recorded `DataHub` corpus through Cube: a Cube source asks
//! for each table of the catalog as the cube of that name, for each column as one of its members,
//! and for each metric as a measure. The example's catalog gives each table its model's name (this
//! corpus keeps its own warehouse names), so `cube/model.yml` must name each model, column and metric.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use sutura_catalog_datahub::fixture::over_fixture_source;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    #[test]
    fn every_table_column_and_metric_of_the_recorded_corpus_is_in_the_example_cube_model() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/metrics-layer/cube/model.yml");
        let model = std::fs::read_to_string(&path).expect("the example Cube model exists");
        let mut cubes: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let (mut cube, mut members) = ("", false);
        for line in model.lines() {
            if let Some(name) = line.strip_prefix("  - name: ") {
                cube = name.trim();
            } else if let Some(key) = line.strip_prefix("    ").filter(|rest| !rest.starts_with(' ')) {
                members = matches!(key.trim_end(), "dimensions:" | "measures:");
            } else if let Some(member) = line.strip_prefix("      - name: ").filter(|_| members) {
                cubes.entry(cube).or_default().push(member.trim());
            }
        }

        let source = SourceName::parse("warehouse").expect("the example source name is a name");
        let version = DefinitionVersion::parse("example-metrics-layer").expect("the fixed version is a version");
        let pinned = over_fixture_source(source, version)
            .load()
            .expect("the recorded catalog loads");
        let definitions = pinned.definitions();
        assert!(!definitions.models().is_empty(), "the recorded corpus declares no table");
        for model in definitions.models().values() {
            let cube = model.name().as_str();
            let members = cubes
                .get(cube)
                .unwrap_or_else(|| panic!("no cube `{cube}` in the example model: {cubes:?}"));
            for column in model.columns() {
                let column = column.name().as_str();
                assert!(
                    members.contains(&column),
                    "cube `{cube}` has no member `{column}`: {members:?}"
                );
            }
        }
        for metric in definitions.metrics().values() {
            let measure = metric.name().as_str();
            assert!(
                cubes.values().any(|members| members.contains(&measure)),
                "no cube has the measure `{measure}`: {cubes:?}"
            );
        }
    }
}

//! Shared CSV fixture typing for the engine adapter.

use std::path::Path;

use datafusion::arrow::datatypes::{DataType, Field, Schema};
use sutura_domain::warehouse::csv::{self, FixtureType};

#[derive(Debug, thiserror::Error)]
pub(super) enum FixtureSchemaError {
    #[error("could not read the fixture CSV")]
    Read(#[from] std::io::Error),
    #[error("could not infer the fixture CSV schema")]
    Inference(#[from] csv::InferenceError),
}

pub(super) fn schema(path: &Path) -> Result<Schema, FixtureSchemaError> {
    let columns = csv::infer(&std::fs::read_to_string(path)?)?;
    let fields = columns
        .into_iter()
        .map(|column| {
            let kind = match column.kind() {
                FixtureType::Boolean => DataType::Boolean,
                FixtureType::Integer | FixtureType::WideInteger => DataType::Decimal256(38, 0),
                FixtureType::Decimal { scale } => DataType::Decimal256(38, scale.cast_signed()),
                FixtureType::Real => DataType::Float64,
                FixtureType::Date => DataType::Date32,
                FixtureType::Text => DataType::Utf8,
            };
            Field::new(column.name().as_str(), kind, true)
        })
        .collect::<Vec<_>>();
    Ok(Schema::new(fields))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use datafusion::arrow::datatypes::DataType;

    #[test]
    fn shared_wide_and_real_fixture_types_reach_arrow() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/datafusion-wide-fixture");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the fixture directory is writable");
        let path = dir.join("wide.csv");
        std::fs::write(
            &path,
            "amount,ordinary,rate,exact\n10000000000000000000,1,1e0,1.25\n1,2,2e0,2.50\n",
        )
        .expect("the fixture is writable");

        let schema = super::schema(&path).expect("the fixture has a schema");
        assert_eq!(schema.field(0).data_type(), &DataType::Decimal256(38, 0));
        assert_eq!(schema.field(1).data_type(), &DataType::Decimal256(38, 0));
        assert_eq!(schema.field(2).data_type(), &DataType::Float64);
        assert_eq!(schema.field(3).data_type(), &DataType::Decimal256(38, 2));
        drop(std::fs::remove_dir_all(dir));
    }
}

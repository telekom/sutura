//! Query-only execution of scoped plans over the files already registered by this adapter.

use sutura_domain::identity::Presented;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::RawRows;
use sutura_domain::warehouse::deadline::Deadline;

use datafusion::prelude::SQLOptions;

use crate::{DataFusionError, DataFusionWarehouse, collect};

pub(super) fn run(
    warehouse: &DataFusionWarehouse,
    statement: &RawStatement,
    presented: &Presented,
    deadline: Deadline,
) -> Result<RawRows, DataFusionError> {
    warehouse.deliverable(presented)?;
    let budget = deadline.budget().duration();
    let remaining = deadline
        .remaining_at(std::time::Instant::now())
        .ok_or(DataFusionError::DeadlineExceeded { budget })?;
    warehouse.runtime()?.block_on(async {
        tokio::time::timeout(remaining, async {
            let options = SQLOptions::new()
                .with_allow_ddl(false)
                .with_allow_dml(false)
                .with_allow_statements(false);
            let frame = warehouse
                .context
                .sql_with_options(statement.as_str(), options)
                .await
                .map_err(|cause| DataFusionError::Analyze { cause })?;
            let rows = collect::collected(frame, warehouse.working_set).await?;
            let (columns, rows) = rows
                .to_rows()
                .map_err(|cause| DataFusionError::Unreadable { cause })?
                .into_parts();
            Ok(RawRows::of(columns, rows))
        })
        .await
        .map_err(|_elapsed| DataFusionError::DeadlineExceeded { budget })?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::{ArrayRef, Int64Array};
    use datafusion::arrow::record_batch::RecordBatch;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use sutura_domain::model::{QualifiedTable, SourceName};
    use sutura_domain::warehouse::{Value, Warehouse as _};

    #[test]
    fn planned_files_read_registered_rows_but_never_enable_raw_writes() {
        let warehouse = DataFusionWarehouse::new(
            SourceName::parse("local").expect("a source"),
            crate::test_posture(),
            crate::WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("positive")),
        )
        .expect("a bounded warehouse");
        let batch =
            RecordBatch::try_from_iter([("amount", Arc::new(Int64Array::from(vec![1, 2, 3])) as ArrayRef)]).expect("a batch");
        drop(warehouse.context.register_batch("orders", batch).expect("register a table"));
        let presented = crate::test_fixtures::test_leg();
        let text = RawStatement::parse("SELECT count(*) AS n FROM orders WHERE amount > 1").expect("bounded SQL");
        let tables = BTreeSet::from([QualifiedTable::parse("orders").expect("a table")]);
        assert!(warehouse.accepts_planned_statements());
        assert!(!warehouse.accepts_raw_statements());
        assert!(warehouse.execute_raw(&text, &presented, crate::test_deadline()).is_none());
        let rows = warehouse
            .execute_planned_statement(&text, &tables, &presented, crate::test_deadline())
            .expect("planned execution exists")
            .expect("query executes");
        assert_eq!(rows.columns(), &[String::from("n")]);
        assert_eq!(rows.rows(), &[vec![Value::Integer(2)]]);
        let write = RawStatement::parse("DROP TABLE orders").expect("bounded SQL");
        warehouse
            .execute_planned_statement(&write, &tables, &presented, crate::test_deadline())
            .expect("implemented")
            .expect_err("planned execution refuses writes");
        assert!(warehouse.context.table_exist("orders").expect("table lookup"));
    }
}

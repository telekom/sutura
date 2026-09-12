//! A job's answer, as the domain's own result set: the schema pass, one cell, and the rows.
//!
//! Its own module because the value mapping is where a WRONG NUMBER would come from. **What it
//! does NOT hold: a credential, a plan, or any decision about who a query runs as** - it is generic
//! in the transport's error rather than in the transport, since nothing here touches one.

use sutura_domain::warehouse::{Real, RowSet, Value};

use crate::transport::{Cell, Field, FieldType, JobRows};
use crate::{BigQueryError, Mapped};

/// One cell, as the domain names it.
///
/// **The mapping is deliberately the same as `sutura-exec-duckdb`'s wherever both can answer**, and
/// that agreement is a correctness property rather than tidiness: one plan answered by two adapters
/// has to produce one number, or an anchor certified against one stops reproducing against the
/// other. The three arms where that matters are marked below.
fn cell<E>(field: &Field, value: Cell) -> Mapped<Value, E>
where
    E: core::error::Error + 'static,
{
    let column = || String::from(field.name());
    let text = match value {
        // A null is a null whatever the column is declared as, so it is answered before the
        // type is read - which is why the schema is checked by [`mappable`] before any row
        // is read. This arm is the belt: it cannot fire for a result that came through `rows`,
        // and it stays because `cell` is reachable from a test on its own and because a
        // null-answered type would be a wrong number rather than a refusal.
        Cell::Null => return Ok(Value::Null),
        Cell::Text(text) => text,
    };
    match *field.kind() {
        FieldType::Int64 => text
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|cause| BigQueryError::NotAnInteger { column: column(), cause }),
        // CHECKED, not taken - see `BigQueryError::NotFinite`. Both SQL adapters have this arm.
        FieldType::Float64 => {
            let parsed = text
                .parse::<f64>()
                .map_err(|cause| BigQueryError::NotADouble { column: column(), cause })?;
            Real::parse(parsed)
                .map(Value::Real)
                .map_err(|cause| BigQueryError::NotFinite { column: column(), cause })
        }
        // **Both stay TEXT, and the arms are joined because the behaviour really is one arm.** For
        // a string that is trivial; for an exact decimal it is the whole point - turning `NUMERIC`
        // into an `f64` here is how a total that was correct in the data system stops being
        // correct in an answer, which is the same sentence `sutura-exec-duckdb` carries on its own
        // `Decimal` arm.
        FieldType::Numeric | FieldType::String => Ok(Value::Text(text)),
        // `Integer(0 | 1)`, because the domain's `Value` has no boolean and `sutura-exec-duckdb`
        // answers a `BOOLEAN` the same way. Agreeing matters here: the example catalog counts a
        // `churned_in_month` flag, so the two adapters would otherwise disagree about a metric.
        FieldType::Bool => match text.as_str() {
            "true" => Ok(Value::Integer(1)),
            "false" => Ok(Value::Integer(0)),
            _ => Err(BigQueryError::NotABool { column: column() }),
        },
        // Parsed and re-rendered rather than passed through, so a malformed date is an error here
        // instead of text that looks like a date downstream. `sutura-exec-duckdb` reaches the same
        // `Value::Text(date.to_iso())` from a day count.
        FieldType::Date => sutura_domain::calendar::Date::parse(&text)
            .map(|date| Value::Text(date.to_iso()))
            .map_err(|cause| BigQueryError::NotADate { column: column(), cause }),
        FieldType::Unmapped(ref named) => Err(BigQueryError::UnmappedType {
            column: column(),
            named: named.clone(),
        }),
    }
}

/// Every column the endpoint declared is one this adapter maps, or the first one that is not.
///
/// **A schema-wide pass, and it runs BEFORE any row is read - which is the whole point.** The
/// per-cell check in [`cell`] can only see a column that HOLDS something: a null is
/// answered before the type is read, so a result with no rows never reached the check at all and
/// a result whose unmapped column happened to be entirely null passed it. A `TIMESTAMP` or a
/// `BYTES` column therefore came back as a successful `RowSet`, and whether this adapter mapped
/// a type depended on what the data happened to be. A test pins both shapes.
///
/// It reads the SCHEMA and nothing else, so the answer does not vary with the page.
fn mappable<E>(answered: &JobRows) -> Mapped<(), E>
where
    E: core::error::Error + 'static,
{
    for field in answered.fields() {
        if let FieldType::Unmapped(ref named) = *field.kind() {
            return Err(BigQueryError::UnmappedType {
                column: String::from(field.name()),
                named: named.clone(),
            });
        }
    }
    Ok(())
}

/// A job's result, as a domain result set.
pub(super) fn rows<E>(answered: &JobRows) -> Mapped<RowSet, E>
where
    E: core::error::Error + 'static,
{
    // The schema first, because it is the one check whose answer does not depend on the rows -
    // see `mappable`. A page this adapter could not read whatever it contained is refused
    // before its count is compared against anything.
    mappable(answered)?;
    // Then the count, which refuses a wrong number before any cell work: a page whose delivered
    // count is not what the endpoint reported must not read as *under the cap, not truncated* -
    // see `BigQueryError::Incomplete`.
    if answered.rows().len() != answered.total_rows() {
        return Err(BigQueryError::Incomplete {
            delivered: answered.rows().len(),
            total: answered.total_rows(),
        });
    }
    let columns: Vec<String> = answered.fields().iter().map(|f| String::from(f.name())).collect();
    let mut out: Vec<Vec<Value>> = Vec::with_capacity(answered.rows().len());
    for (index, row) in answered.rows().iter().enumerate() {
        // Checked here rather than left to `RowSet::new`, so the refusal can name WHICH row the
        // endpoint sent at the wrong width. `RowSet::new` catches it too, and that is the belt:
        // this is the one that produces a usable message.
        if row.len() != columns.len() {
            return Err(BigQueryError::RowWidth {
                row: index,
                cells: row.len(),
                columns: columns.len(),
            });
        }
        let mut cells: Vec<Value> = Vec::with_capacity(row.len());
        for (field, value) in answered.fields().iter().zip(row.iter()) {
            cells.push(cell(field, value.clone())?);
        }
        out.push(cells);
    }
    RowSet::new(columns, out).map_err(|cause| BigQueryError::Shape { cause })
}

//! Catalog-authored SQL, through the boundary that turns a fragment into a checked AST.
//!
//! **The boundary.** `sutura_sql::expression::compile` takes an [`AuthoredSql`] - a fragment of
//! SQL a catalog author wrote - and turns it into a [`CompiledExpression`] checked against a
//! metric's own model. Anything text-shaped here is handed to `polyglot-sql`'s parser over the
//! fragment (as `SELECT {fragment}` so the empty-token-list abort in the layer's raw fragment API
//! is never reached), then this crate's own refusal guards, then a render-and-reparse per dialect.
//! The author of a catalog is untrusted by this repository's threat model, so every byte of the
//! fragment is input this deployment did not write.
//!
//! **What is asserted beyond "did not abort".** The compile either succeeds with one rendering per
//! dialect, or refuses with a typed [`ExpressionError`] - there is no third outcome. A returned
//! `CompiledExpression` is then serialized to JSON, which is the shape the definition digest is
//! taken over; a panic in that `Serialize` would move a digest, so the harness trips it too.
//!
//! **The limit.** The authored-SQL hatch (`docs/adr/0004`) has no production caller yet: no shipped
//! binary compiles a fragment a caller supplied, because the load path is not wired and the FTYPES[]
//! document shape has no key for it (see `query-surface`'s built-and-not-wired inventory). So the
//! input here is authored content, one hop further from a caller than the token, and the boundary
//! only becomes live when the hatch is wired - at which point this target is already built and
//! running. It is the SQL-project parser this repository has, and the one the design scoped to.

#![no_main]

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use libfuzzer_sys::fuzz_target;
use sutura_domain::expression::{AuthoredSql, SqlFragment};
use sutura_domain::model::{ColumnName, TableName};
use sutura_sql::expression::compile;

/// The table and columns every metric this fuzzer writes sits over.
///
/// Fixed, because they are the DECLARATION, not the input: a fragment is fuzzed against a model,
/// and re-deriving the model from the input would let the fuzzer pick an easy column set instead
/// of exercising the parser. Two columns cover the aggregation shapes the hatch exists for (a
/// measure and a dimension), and both names parse as identifiers.
fn columns() -> BTreeSet<ColumnName> {
    BTreeSet::from([
        ColumnName::parse("mrr_eur").expect("a fixture column is a column"),
        ColumnName::parse("status").expect("a fixture column is a column"),
    ])
}

fn table() -> TableName {
    TableName::parse("fact_subscription").expect("a fixture table is a table")
}

fuzz_target!(|data: &[u8]| {
    // The fragment API refuses the empty and over-long cases before this target sees the text, so
    // arbitrary bytes arrive here lossily converted exactly as a catalog file would deliver them.
    let fragment = String::from_utf8_lossy(data);
    let Ok(fragment) = SqlFragment::parse(fragment) else {
        return;
    };

    // The authoring dialect is a reserved word (`portable`); only that one can reach every
    // dialect this build renders for, so it is the only key a generated fragment realistically
    // carries. `AuthoredSql::new` refuses an empty map and `compile` resolves it per dialect.
    let portable = sutura_domain::expression::DialectTag::parse("portable").expect("the portable word is one");
    let Ok(authored) = AuthoredSql::new(BTreeMap::from([(portable, fragment)])) else {
        return;
    };

    if let Ok(compiled) = compile(&authored, &table(), &columns()) {
        // The digest is taken over the serialized form, so the round trip must hold or the digest
        // moves. `to_value` compiles as part of this target when the sql_expression names it.
        drop(serde_json::to_value(&compiled));
    }
});

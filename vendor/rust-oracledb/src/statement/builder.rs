//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// builder.rs
//
// Builds a statement from SQL statement text and various options.
//-----------------------------------------------------------------------------

use super::Statement;
use super::StatementOptions;

use crate::client::ClientRef;
use crate::error::Error;
#[cfg(feature = "arrow")]
use arrow_schema::SchemaRef;

pub struct StatementBuilder<'sql> {
    client_ref: ClientRef,
    sql: &'sql str,
    options: StatementOptions,
    cache_statement: bool,
}

impl<'sql> StatementBuilder<'sql> {
    /// Creates a new builder with default options.
    pub(crate) fn new(client_ref: &ClientRef, sql: &'sql str) -> Self {
        Self {
            client_ref: client_ref.clone(),
            sql,
            options: StatementOptions::new(),
            cache_statement: true,
        }
    }

    /// Specifies the output row count for query_arrow_batches(). Responses
    /// are split or combined across bind executions to fill each batch; the
    /// final batch may be smaller. Without this option, each nonempty response
    /// produces one batch. The value must be non-zero or the function will
    /// panic.
    #[cfg(feature = "arrow")]
    pub fn arrow_batch_size(mut self, value: usize) -> Self {
        assert!(value > 0, "Arrow batch size must be a positive integer");
        self.options.set_arrow_batch_size(value);
        self
    }

    /// Makes query_arrow_batches() yield one batch per round trip to the
    /// database: first the rows fetched by the execute, then the rows of each
    /// fetch. A round trip is made only when the next batch is requested and
    /// never more than one per batch, so the application can stop between any
    /// two. Overrides arrow_batch_size().
    #[cfg(feature = "arrow")]
    pub fn arrow_batch_per_round_trip(mut self) -> Self {
        self.options.set_arrow_batch_per_round_trip();
        self
    }

    /// Makes the default Arrow schema fetch a NUMBER with no fixed precision,
    /// or whose scale's absolute value exceeds its precision, as its exact
    /// decimal text (Utf8) instead of Float64: no fixed Arrow decimal holds
    /// every value such a column may store, and Float64 rounds most of them.
    /// A requested schema takes precedence.
    #[cfg(feature = "arrow")]
    pub fn exact_number_text(mut self) -> Self {
        self.options.set_exact_number_text();
        self
    }

    /// Builds the statement from the SQL and the options.
    pub fn build(self) -> Result<Statement, Error> {
        let statement = {
            self.client_ref.lock()?.get_statement(
                self.sql,
                self.cache_statement,
                &self.options,
            )?
        };
        Ok(Statement::new(self.client_ref, statement))
    }

    /// Specifies that this statement should not be cached.
    pub fn exclude_from_cache(mut self) -> Self {
        self.cache_statement = false;
        self
    }

    /// Specifies the number of rows that should be fetched at a time from the
    /// database.
    pub fn fetch_array_size(mut self, value: u32) -> Self {
        self.options.set_fetch_array_size(value);
        self
    }

    /// Specifies that LOB values should be fetched as LOB locators.
    pub fn fetch_lobs(mut self) -> Self {
        self.options.set_fetch_lobs();
        self
    }

    /// Specifies the number of rows that should be fetched when the statement
    /// is executed.
    pub fn prefetch_rows(mut self, value: u32) -> Self {
        self.options.set_prefetch_rows(value);
        self
    }

    /// Specifies the Arrow schema to use for query results. Fields map to
    /// query columns by position. The schema must have one field per column
    /// and use supported fetch types. Applies to query_arrow() and
    /// query_arrow_batches().
    #[cfg(feature = "arrow")]
    pub fn with_requested_schema(mut self, schema: SchemaRef) -> Self {
        self.options.set_requested_schema(schema);
        self
    }
}

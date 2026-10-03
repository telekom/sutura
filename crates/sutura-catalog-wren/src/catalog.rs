//! A [`SemanticCatalog`] over a `WrenAI` MDL `manifest.json`.
//!
//! This is the reader half of the wren crate. [`super::import`] converts a manifest into markdown
//! catalog documents; this module reads the same manifest bytes (the committed fixture under
//! `testdata/manifest.json`) straight into a pinned sutura bundle, so the crate is not merely an
//! exporter but can also be registered as a `declaring` metadata source in the conformance matrix.
//!
//! **What this reader supplies is the physical model and nothing of the semantic layer.** A wren
//! `Model` with a `tableReference` is a physical table: its name is the model (and table) name and
//! every non-computed, non-relationship column carries a declared type, so the bundle provides
//! exactly [`DefinitionKind::Structure`] and - per column, since a wren `Column.type` is required
//! but a spelling this crate cannot represent is dropped per [`Column::new`]'s rule - may-provide
//! [`DefinitionKind::ColumnTypes`]. Everything else a wren project writes about joins, cubes and
//! metrics is a deliberate, declared absence for the same reason `sutura-catalog-okf` declares its
//! own: this adapter is measured against what it says it carries, and it says it carries only the
//! columns-as-defined.
//!
//! **What cannot be represented is refused, never dropped.** A `Model` whose rows come from an
//! authored statement (`refSql`) is not a physical table, and a column that navigates a relation or
//! carries a computation is not a physical field - each is a refusal naming the item rather than a
//! row silently missing from the model, mirroring [`super::convert`]'s own "everything maps or is
//! refused by name" rule.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, ColumnType, Definitions, Description, Model};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, InvalidIdentifier, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

use crate::wire;

/// A catalog read from one `WrenAI` `manifest.json`.
///
/// Like the other on-disk adapters, it carries a declared NAME (the key the contribution manifest
/// records this contributor under), the manifest path and a version: the version identifies which
/// snapshot of the manifest this is supplied from, exactly as [`sutura_domain::pinned`] expects.
#[derive(Debug, Clone)]
pub struct WrenCatalog {
    name: SourceName,
    manifest: PathBuf,
    version: DefinitionVersion,
}

impl WrenCatalog {
    /// Points a catalog at one wren `manifest.json`.
    ///
    /// The version is supplied rather than derived, because what identifies a snapshot of a
    /// manifest is not something the manifest knows - it is a commit id or a tag that only the
    /// caller has.
    pub const fn new(name: SourceName, manifest: PathBuf, version: DefinitionVersion) -> Self {
        Self { name, manifest, version }
    }

    /// The declared name this contributor is recorded under in a bundle's contribution manifest.
    #[inline]
    pub const fn name(&self) -> &SourceName {
        &self.name
    }

    /// Turns one manifest model into a domain [`Model`], refusing whatever is not a physical table.
    fn model_to_domain(&self, model: &wire::Model) -> Result<Model, WrenCatalogError> {
        if model.ref_sql.is_some() {
            return Err(WrenCatalogError::RefSqlModel {
                model: model.name.clone(),
            });
        }
        let name = ModelName::parse(&model.name).map_err(|cause| WrenCatalogError::InvalidModelName {
            model: model.name.clone(),
            cause,
        })?;
        // The table name is the model name: wren's `tableReference` is joined and pre-quoted, and
        // for a `Structure` declaration a table needs a stable, unique name - which the model's own
        // name is. `TableName` yields the unqualified path, matching `sutura-catalog-okf`'s use of
        // the file stem.
        let table = TableName::parse(&model.name).map_err(|cause| WrenCatalogError::InvalidModelName {
            model: model.name.clone(),
            cause,
        })?;
        let mut columns = Vec::with_capacity(model.columns.len());
        for column in &model.columns {
            // A relationship-navigation column carries no physical field of its own; a computed or
            // access-controlled column carries SQL or governance text a `Column` cannot hold. Each
            // is refused by name, never silently dropped.
            if column.relationship.is_some() {
                return Err(WrenCatalogError::UnrepresentableColumn {
                    model: model.name.clone(),
                    column: column.name.clone(),
                    reason: "a relationship-navigation column is not a physical field",
                });
            }
            if column.access_control.is_some() {
                return Err(WrenCatalogError::UnrepresentableColumn {
                    model: model.name.clone(),
                    column: column.name.clone(),
                    reason: "a column-level access control has no sutura equivalent",
                });
            }
            if column.is_calculated
                || column
                    .expression
                    .as_deref()
                    .is_some_and(|expression| !expression.trim().is_empty())
            {
                return Err(WrenCatalogError::UnrepresentableColumn {
                    model: model.name.clone(),
                    column: column.name.clone(),
                    reason: "a calculated column's SQL expression cannot be held in column metadata",
                });
            }
            let column_name = ColumnName::parse(&column.name).map_err(|cause| WrenCatalogError::InvalidColumnName {
                model: model.name.clone(),
                column: column.name.clone(),
                cause,
            })?;
            // `ColumnType::parse` drops an unrepresentable spelling per `Column::new`'s rule rather
            // than refusing the whole load - the same "a type is dropped, a description refuses"
            // split `sutura-catalog-okf` documents.
            columns.push(Column::new(
                column_name,
                ColumnType::parse(column.r#type.as_str()).ok(),
                Description::default(),
                None,
            ));
        }
        Ok(Model::new(name, self.name.clone(), table, columns, Description::default()))
    }
}

impl SemanticCatalog for WrenCatalog {
    type Error = WrenCatalogError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    fn capabilities() -> MetadataCapabilities {
        // Exactly what this adapter produces and nothing more: the physical model, and the column
        // types as a declared-and-empty may-provide because whether a `Column.type` spelling
        // survives `ColumnType::parse` is a property of the spelling rather than a structural
        // guarantee. No descriptions, no relationships, no metrics, no knowledge - a wren manifest
        // in this reader's subset carries none of those, and declaring them would be an over-claim.
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure]).and_may_provide([DefinitionKind::ColumnTypes]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let text =
            sutura_bounded_read::read_document(&self.manifest, &self.manifest, 0).map_err(|source| WrenCatalogError::Read {
                path: self.manifest.clone(),
                source,
            })?;
        let manifest: wire::Manifest = serde_json::from_str(&text).map_err(|source| WrenCatalogError::NotAManifest {
            path: self.manifest.clone(),
            source,
        })?;
        let mut models = Vec::with_capacity(manifest.models.len());
        for model in &manifest.models {
            models.push(self.model_to_domain(model)?);
        }
        let definitions =
            Definitions::assemble(models, Vec::new(), Vec::new()).map_err(|cause| WrenCatalogError::Inconsistent { cause })?;
        let knowledge = Knowledge::assemble(&definitions, KnowledgeInput::none())
            .map_err(|cause| WrenCatalogError::UncheckableKnowledge { cause })?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| WrenCatalogError::Digest { cause })
    }
}

/// Why one `manifest.json` could not be read as a wren catalog.
///
/// Every variant but three (`Inconsistent`, `UncheckableKnowledge`, `Digest`) carries the manifest
/// path, because a catalog is one file and the file is what failed.
#[derive(Debug, thiserror::Error)]
pub enum WrenCatalogError {
    #[error("could not read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: sutura_bounded_read::ReadError,
    },
    #[error("{} is not a wren MDL manifest: {source}", path.display())]
    NotAManifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("the model {model} could not be read as a name")]
    InvalidModelName {
        model: String,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the column {model}.{column} could not be read as a name")]
    InvalidColumnName {
        model: String,
        column: String,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the model {model} declares refSql, so it is not a physical table")]
    RefSqlModel { model: String },
    #[error("the column {model}.{column} is not a physical field: {reason}")]
    UnrepresentableColumn {
        model: String,
        column: String,
        reason: &'static str,
    },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: sutura_domain::catalog::InconsistentDefinitions,
    },
    #[error("the catalog's knowledge does not hold together")]
    UncheckableKnowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sutura_domain::capabilities::MetadataCapabilities;

    use super::*;
    use crate::tests::scratch;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/manifest.json");

    fn test_name() -> SourceName {
        SourceName::parse("test").expect("a test name is a name")
    }

    fn test_version() -> DefinitionVersion {
        DefinitionVersion::parse("0000000000000000000000000000000000000000").expect("a test version is a version")
    }

    fn catalog_at(manifest: &Path) -> WrenCatalog {
        WrenCatalog::new(test_name(), manifest.to_path_buf(), test_version())
    }

    /// The committed fixture loads, pins and - the declaring path's whole contract - produces
    /// exactly what it declares: a physical model whose typed columns make `ColumnTypes` a produced
    /// conditional kind, and nothing else.
    #[test]
    fn the_fixture_manifest_loads_pins_and_matches_its_declaration() {
        let manifest = Path::new(FIXTURE);
        let pinned = catalog_at(manifest).load().expect("the committed fixture manifest loads");
        assert_eq!(2, pinned.definitions().models().len(), "the fixture carries two models");
        let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
        assert_eq!(
            Ok(()),
            WrenCatalog::capabilities().checked_against(&produced),
            "the fixture bundle and the adapter's declaration disagree"
        );
    }

    /// The same bytes read twice give the same digest -
    /// `reads_the_same_documents_twice_to_the_same_digest`, exercised at the adapter's own level so
    /// a regression reddens the crate's own tests first.
    #[test]
    fn the_same_manifest_reads_twice_to_the_same_digest() {
        let manifest = Path::new(FIXTURE);
        let first = catalog_at(manifest).load().expect("the fixture loads once");
        let second = catalog_at(manifest).load().expect("the fixture loads twice");
        assert_eq!(first.digest(), second.digest());
    }

    /// A `refSql` model is not a physical table; the reader refuses it by name rather than reading
    /// an authored statement as one.
    #[test]
    fn a_ref_sql_model_is_refused_rather_than_read_as_a_physical_table() {
        let dir = scratch("ref-sql");
        let manifest = dir.join("manifest.json");
        std::fs::write(
            &manifest,
            r#"{"catalog":"example","schema":"public","models":[{"name":"revenue","refSql":"select * from orders","columns":[]}]}"#,
        )
        .expect("the scratch manifest is writable");

        let err = catalog_at(&manifest).load().expect_err("a refSql model is refused");

        assert!(
            matches!(err, WrenCatalogError::RefSqlModel { ref model } if model == "revenue"),
            "a refSql model is refused by name, got {err:?}"
        );
    }

    /// A computed column cannot be held in sutura's column metadata; the reader refuses it by name
    /// rather than dropping it from the model.
    #[test]
    fn a_calculated_column_is_refused_by_name() {
        let dir = scratch("calculated");
        let manifest = dir.join("manifest.json");
        std::fs::write(
            &manifest,
            r#"{"catalog":"example","schema":"public","models":[{"name":"orders","columns":[{"name":"total","type":"NUMERIC"},{"name":"double_total","type":"NUMERIC","isCalculated":true,"expression":"total * 2"}]}]}"#,
        )
        .expect("the scratch manifest is writable");

        let err = catalog_at(&manifest).load().expect_err("a calculated column is refused");

        assert!(
            matches!(
                err,
                WrenCatalogError::UnrepresentableColumn {
                    ref model,
                    ref column,
                    ..
                } if model == "orders" && column == "double_total"
            ),
            "a calculated column is refused by name, got {err:?}"
        );
    }

    /// A missing manifest is a `Read` naming the path, mirroring the importer's own split between a
    /// read failure and a parse failure.
    #[test]
    fn a_missing_manifest_is_a_read_error_naming_the_path() {
        let dir = scratch("missing");
        let manifest = dir.join("manifest.json");

        let err = catalog_at(&manifest).load().expect_err("a missing manifest does not load");

        assert!(
            matches!(err, WrenCatalogError::Read { ref path, .. } if path == &manifest),
            "a missing manifest is Read naming the path, got {err:?}"
        );
    }
}

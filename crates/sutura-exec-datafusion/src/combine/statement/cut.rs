//! Cuts a validated plan into the sub-plans each listed source answers (`Cutter`, a
//! `datafusion_federation` optimizer rule run inside [`super::session`]), and `Part`, what
//! [`super::parts`] collects about each resulting sub-plan: the source and dialect it runs under,
//! and whether it reads a column under a text policy.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::datatypes::{Schema, SchemaRef};
use datafusion::catalog::Session;
use datafusion::common::tree_node::{Transformed, TreeNode as _, TreeNodeRecursion};
use datafusion::common::{ExprSchema as _, plan_datafusion_err, plan_err};
use datafusion::config::ConfigOptions;
use datafusion::datasource::memory::MemorySourceConfig;
use datafusion::error::Result as Planned;
use datafusion::logical_expr::{Expr, Extension, Filter, LogicalPlan, Projection, TableSource};
use datafusion::optimizer::{Optimizer, OptimizerConfig, OptimizerRule};
use datafusion::physical_optimizer::PhysicalOptimizerRule;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::coop::CooperativeExec;
use datafusion_federation::{FederatedPlanNode, FederatedTableSource, FederationPlanner, FederationProvider, get_table_source};
use sutura_domain::model::{ProjectName, QualifiedTable, SourceName};
use sutura_domain::plan::{PolicyRead, StatementTable};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::schema::TypeDialect;

use super::{UnparsedWith, labelled, projection, pushdown, resolve, rewrite, text, written};

/// Wraps every node, not only the leaves the default rule wraps, so each batch any operator yields
/// spends the task's budget and the deadline's timer is polled between them.
#[derive(Debug)]
pub(super) struct Yielding;

impl PhysicalOptimizerRule for Yielding {
    fn optimize(&self, plan: Arc<dyn ExecutionPlan>, _: &ConfigOptions) -> Planned<Arc<dyn ExecutionPlan>> {
        plan.transform_up(|node| {
            let wrapped: Arc<dyn ExecutionPlan> = Arc::new(CooperativeExec::new(node));
            Ok(Transformed::yes(wrapped))
        })
        .map(|wrapped| wrapped.data)
    }

    fn name(&self) -> &'static str {
        "sutura_statement_yielding"
    }

    fn schema_check(&self) -> bool {
        true
    }
}

/// Each part's plan beside the rows its leg came back with.
type Leaves = Vec<(LogicalPlan, Vec<RecordBatch>)>;

/// One listed source, as the federation rule sees it: two tables' scans fuse when their sources do.
pub(super) struct Source {
    pub(super) name: SourceName,
    pub(super) cut: Arc<Optimizer>,
}

impl FederationProvider for Source {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn compute_context(&self) -> Option<String> {
        None
    }

    fn optimizer(&self) -> Option<Arc<Optimizer>> {
        Some(Arc::clone(&self.cut))
    }
}

/// A table of a listed source, typed by its pinned schema. It is never scanned here.
pub(super) struct Table {
    pub(super) source: Arc<Source>,
    pub(super) schema: SchemaRef,
}

impl TableSource for Table {
    fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }
}

impl FederatedTableSource for Table {
    fn federation_provider(&self) -> Arc<dyn FederationProvider> {
        Arc::<Source>::clone(&self.source)
    }
}

/// Each part's plan beside the rows its leg came back with, set once every leg has run.
#[derive(Debug, Default)]
pub(super) struct Fetched(OnceLock<Leaves>);

impl Fetched {
    /// Records every part's rows, once, for [`FederationPlanner::plan_federation`] to hand back as
    /// each leaf's own in-memory source. `Err` only when called a second time.
    pub(super) fn set(&self, leaves: Leaves) -> Result<(), Leaves> {
        self.0.set(leaves)
    }
}

#[async_trait::async_trait]
impl FederationPlanner for Fetched {
    async fn plan_federation(&self, node: &FederatedPlanNode, _: &dyn Session) -> Planned<Arc<dyn ExecutionPlan>> {
        let Some((_, batches)) = self
            .0
            .get()
            .and_then(|parts| parts.iter().find(|(plan, _)| plan == node.plan()))
        else {
            return plan_err!("a part of the plan was planned before its leg ran");
        };
        let leaf: Arc<dyn ExecutionPlan> =
            MemorySourceConfig::try_new_exec(core::slice::from_ref(batches), Arc::clone(node.plan().schema().inner()), None)?;
        Ok(leaf)
    }
}

/// The rule a source's sub-plan is handed to: it makes the sub-plan a leaf, planned over its leg's rows.
#[derive(Debug)]
pub(super) struct Cutter(pub(super) Arc<Fetched>, pub(super) TypeDialect, pub(super) bool);

pub(super) fn number_text(schema: &SchemaRef) -> bool {
    schema
        .fields()
        .iter()
        .any(|field| field.metadata().get("sutura:unbounded_number").map(String::as_str) == Some("utf8"))
}

/// Passthrough projections preserve the mapped representation. Value-dependent uses cannot run
/// locally as text; inspect inputs because their column can be absent from the output.
pub(super) fn number_text_safe(plan: &LogicalPlan) -> Planned<bool> {
    if let LogicalPlan::Distinct(datafusion::logical_expr::Distinct::All(input)) = plan {
        return Ok(!number_text(input.schema().inner()));
    }
    let mut safe = true;
    plan.apply_expressions(|expression| {
        if matches!(plan, LogicalPlan::Projection(_))
            && (matches!(expression, Expr::Column(_))
                || matches!(expression, Expr::Alias(alias) if matches!(alias.expr.as_ref(), Expr::Column(_))))
        {
            return Ok(TreeNodeRecursion::Continue);
        }
        expression.apply(|part| {
            if let Expr::Column(column) = part {
                for input in plan.inputs() {
                    safe &= input
                        .schema()
                        .field_from_column(column)
                        .or_else(|_| input.schema().field_with_unqualified_name(&column.name))
                        .map_or_else(
                            |_| !number_text(input.schema().inner()),
                            |field| field.metadata().get("sutura:unbounded_number").map(String::as_str) != Some("utf8"),
                        );
                }
            }
            Ok(TreeNodeRecursion::Continue)
        })?;
        Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(safe)
}

impl OptimizerRule for Cutter {
    fn name(&self) -> &'static str {
        "sutura_statement_cut"
    }

    fn rewrite(&self, plan: LogicalPlan, _: &dyn OptimizerConfig) -> Planned<Transformed<LogicalPlan>> {
        let mut already_cut = false;
        plan.apply_with_subqueries(|node| {
            already_cut |= part_of(node).is_some();
            Ok(TreeNodeRecursion::Continue)
        })?;
        if already_cut {
            return Ok(Transformed::no(plan));
        }
        let leaf = |plan| {
            let planner: Arc<dyn FederationPlanner> = Arc::<Fetched>::clone(&self.0);
            Transformed::yes(LogicalPlan::Extension(Extension {
                node: Arc::new(FederatedPlanNode::new(plan, planner)),
            }))
        };
        plan.transform_down_with_subqueries(|node| {
            let filtered_scan = pushdown::is_filtered_scan(&node);
            if self.2 && !filtered_scan && !matches!(node, LogicalPlan::TableScan(_)) {
                return Ok(Transformed::no(node));
            }
            let (mut safe, mut scans) = (true, false);
            node.apply_with_subqueries(|input| {
                scans |= matches!(input, LogicalPlan::TableScan(_));
                safe &= number_text_safe(input)?;
                // Keep Cube joins local while the same cut pushes their supported children down.
                if self.1 == TypeDialect::Cube && matches!(input, LogicalPlan::Join(_)) {
                    safe = false;
                }
                Ok(if safe {
                    TreeNodeRecursion::Continue
                } else {
                    TreeNodeRecursion::Stop
                })
            })?;
            if !safe || !scans {
                return Ok(Transformed::no(node));
            }
            if self.2
                && let LogicalPlan::Filter(filter) = &node
            {
                let Some(scan) = pushdown::cube_scan(filter)? else {
                    return Ok(Transformed::no(node));
                };
                let cut = leaf(scan);
                let kept = LogicalPlan::Filter(Filter::try_new(filter.predicate.clone(), Arc::new(cut.data))?);
                return Ok(Transformed::new(kept, true, TreeNodeRecursion::Jump));
            }
            let node = if matches!(&node, LogicalPlan::TableScan(_)) {
                let columns = node.schema().columns().into_iter().map(Expr::Column).collect();
                LogicalPlan::Projection(Projection::try_new(columns, Arc::new(node))?)
            } else {
                node
            };
            let cut = leaf(node);
            Ok(Transformed::new(cut.data, true, TreeNodeRecursion::Jump))
        })
    }
}

/// A sub-plan one source answers.
pub(super) struct Part {
    pub(super) plan: LogicalPlan,
    pub(super) source: SourceName,
    pub(super) dialect: TypeDialect,
    pub(super) policy: PolicyRead,
}

/// Whether a scan of `plan` projects a column under a text policy. Cube's policy runs here over the
/// rows its leg answers; supported conjuncts on columns without a policy narrow those rows at Cube.
fn policy_read(plan: &LogicalPlan, dialect: TypeDialect) -> Planned<PolicyRead> {
    let mut reads = false;
    plan.apply_with_subqueries(|node| {
        if let LogicalPlan::TableScan(scan) = node {
            reads |= text::reads_policy(scan.projected_schema.inner());
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(match (reads, dialect) {
        (false, _) => PolicyRead::Unread,
        (true, TypeDialect::Cube) => PolicyRead::Locally,
        (true, _) => PolicyRead::AtSource,
    })
}

impl Part {
    pub(super) fn scan_tables(&self, admitted: &[StatementTable]) -> Planned<BTreeSet<QualifiedTable>> {
        let mut tables = BTreeSet::new();
        self.plan.apply_with_subqueries(|node| {
            if let LogicalPlan::TableScan(scan) = node {
                let Some(table) = admitted.iter().find(|table| {
                    let (catalog, schema) = table.table().qualifier().map_or(("", ""), |qualifier| {
                        (
                            qualifier.project().map_or("", ProjectName::as_str),
                            qualifier.dataset().as_str(),
                        )
                    });
                    table.source() == &self.source
                        && resolve::registered_leaf(admitted, table) == scan.table_name.table()
                        && schema == scan.table_name.schema().unwrap_or("")
                        && catalog == scan.table_name.catalog().unwrap_or("")
                }) else {
                    return plan_err!("a part scans a table outside its admitted source");
                };
                tables.insert(table.table().clone());
            }
            Ok(TreeNodeRecursion::Continue)
        })?;
        Ok(tables)
    }

    /// The part in its source's dialect: as planned when it is the `whole` statement, otherwise
    /// with its columns labelled `c0..cn`, so the rows can be read by position and checked by name.
    pub(super) fn statement(&self, whole: bool, tables: &[StatementTable], output_schema: &Schema) -> UnparsedWith<RawStatement> {
        let scanned = self.scan_tables(tables)?;
        let read: Vec<_> = tables
            .iter()
            .filter(|table| table.source() == &self.source && scanned.contains(table.table()))
            .cloned()
            .collect();
        let full_lobs = read.iter().any(text::oracle_lobs);
        if full_lobs && !whole {
            return plan_err!("Oracle text-policy LOB operators must run wholly at the source");
        }
        // `self.plan`'s scan and column names may still carry a collision's mangled registration
        // leaf (`resolve::registered_leaf`); the source has no notion of that sutura-internal name.
        let physical = rewrite::physically_qualified(&self.plan, tables)?;
        let (text, ineligible) = if whole && !full_lobs && !projection::duplicate_names(physical.schema().inner()) {
            written(&physical, self.dialect, &read)?
        } else {
            written(&labelled(&physical)?, self.dialect, &read)?
        };
        let text = if full_lobs {
            text::bounded_oracle_rows(&text, output_schema, &read)?
        } else {
            text
        };
        RawStatement::parse(text)
            .map_err(|cause| plan_datafusion_err!("{cause}"))
            .map(|statement| (statement, ineligible))
    }
}

/// The first part that reads a column under a text policy, or `Unread` when none does.
pub(super) fn policy_of(parts: &[Part]) -> PolicyRead {
    parts
        .iter()
        .map(|part| part.policy)
        .find(|read| read.is_read())
        .unwrap_or(PolicyRead::Unread)
}

pub(super) fn part_of(plan: &LogicalPlan) -> Option<&FederatedPlanNode> {
    let LogicalPlan::Extension(extension) = plan else {
        return None;
    };
    extension.node.as_any().downcast_ref()
}

/// Every distinct part of `plan`, subqueries included, each with the source and dialect it runs in.
pub(super) fn parts(plan: &LogicalPlan, tables: &[StatementTable]) -> Planned<Vec<Part>> {
    let mut parts: Vec<Part> = Vec::new();
    plan.apply_with_subqueries(|node| {
        if let Some(cut) = part_of(node)
            && !parts.iter().any(|part| &part.plan == cut.plan())
        {
            let name = source_of(cut.plan())?;
            let Some(table) = tables.iter().find(|table| table.source().as_str() == name) else {
                return plan_err!("a part of the plan reads a source the question does not carry");
            };
            parts.push(Part {
                plan: cut.plan().clone(),
                source: table.source().clone(),
                dialect: table.dialect(),
                policy: policy_read(cut.plan(), table.dialect())?,
            });
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    if parts.is_empty() {
        return plan_err!("the statement reads no table of a listed source");
    }
    Ok(parts)
}

/// The one source `part` reads, subqueries included. The rule does not look inside an `EXISTS`, so
/// a part can hold a second source's table, and such a part is refused rather than sent to either.
fn source_of(part: &LogicalPlan) -> Planned<String> {
    let mut names = BTreeSet::new();
    part.apply_with_subqueries(|node| {
        if let LogicalPlan::TableScan(scan) = node
            && let Some(table) = get_table_source(&scan.source)?
        {
            names.insert(table.federation_provider().name().to_owned());
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    let mut names = names.into_iter();
    match (names.next(), names.next()) {
        (Some(name), None) => Ok(name),
        (None, _) => plan_err!("a part of the plan reads no table"),
        (Some(_), Some(_)) => plan_err!("a part of the plan reads more than one source"),
    }
}

//! Resolve only the caller's admitted metadata, before the engine sees a table reference.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use datafusion::sql::sqlparser::ast::{
    Expr, Ident, JoinConstraint, JoinOperator, ObjectName, ObjectNamePart, Query, SelectItem, SelectItemQualifiedWildcardKind,
    SetExpr, Statement, TableAlias, TableFactor, TableWithJoins, VisitMut as _, VisitorMut,
};
use sutura_domain::model::QualifiedTable;
use sutura_domain::plan::StatementTable;
use sutura_domain::raw::SqlConstruct;
use sutura_domain::warehouse::schema::TypeDialect;

#[derive(Debug, Clone, Copy)]
pub(super) enum Refused {
    UnknownTable,
    UnknownColumn,
    Unsupported(SqlConstruct),
    /// A name holds a backslash or a quote character, which no source statement carries.
    UnquotableName,
}

type Resolved<T> = Result<T, Refused>;
type Body = (Relation, Vec<Relation>);
/// The statement's own admitted physical tables, plus whether any reference resolved only by
/// naming its source explicitly - see [`statement`]'s own doc for what that flag then obliges.
type ResolvedTables = Resolved<(BTreeSet<QualifiedTable>, bool)>;

#[derive(Clone, Copy)]
enum Fold {
    Lower,
    Upper,
}

impl Fold {
    fn of(dialect: TypeDialect) -> Self {
        if dialect == TypeDialect::Oracle {
            Self::Upper
        } else {
            Self::Lower
        }
    }

    fn name(self, ident: &Ident) -> String {
        if ident.quote_style.is_some() {
            return ident.value.clone();
        }
        match self {
            Self::Lower => ident.value.to_lowercase(),
            Self::Upper => ident.value.to_uppercase(),
        }
    }
}

#[derive(Clone)]
struct Column {
    name: String,
    fold: Fold,
}

#[derive(Clone)]
struct Relation {
    path: Vec<String>,
    fold: Fold,
    columns: Vec<Column>,
    complete: bool,
}

impl Relation {
    fn matches(&self, path: &[Ident]) -> bool {
        path.len() <= self.path.len()
            && path
                .iter()
                .rev()
                .zip(self.path.iter().rev())
                .all(|(ident, name)| self.fold.name(ident) == *name)
    }

    fn projection(&self) -> Resolved<Vec<SelectItem>> {
        if !self.complete {
            return Err(unsupported());
        }
        Ok(self
            .columns
            .iter()
            .map(|column| {
                let mut path: Vec<_> = self.path.iter().cloned().map(quoted).collect();
                path.push(quoted(column.name.clone()));
                SelectItem::UnnamedExpr(Expr::CompoundIdentifier(path))
            })
            .collect())
    }

    fn alias(mut self, alias: &mut TableAlias) -> Self {
        normalize(&mut alias.name);
        self.path = vec![alias.name.value.clone()];
        self.fold = Fold::Lower;
        if !alias.columns.is_empty() {
            self.complete = true;
            self.columns = alias
                .columns
                .iter_mut()
                .map(|column| {
                    normalize(&mut column.name);
                    Column {
                        name: column.name.value.clone(),
                        fold: Fold::Lower,
                    }
                })
                .collect();
        }
        self
    }
}

fn normalize(ident: &mut Ident) {
    *ident = quoted(Fold::Lower.name(ident));
}

fn quoted(name: String) -> Ident {
    Ident::with_quote('"', name)
}

const fn unsupported() -> Refused {
    Refused::Unsupported(SqlConstruct::Query)
}

/// Quoted paths and schemas remain exact; an unquoted Oracle component folds up, all others down.
///
/// A table's own physical path resolves first, exactly as before; only when nothing matches it does
/// that same path prefixed with the sutura source it lives at (`source.schema.table` beside
/// `schema.table`, `source.table` beside `table`) get tried - one caller-facing syntax, not a second
/// one gated behind a flag, and never the reason an unprefixed path two sources share stops
/// resolving on its own. The prefix only selects among `tables`, which is already the caller's
/// listed and permitted set: a source segment naming anything else matches no candidate and falls
/// through to the same [`Refused::UnknownTable`] as an unknown schema or table does, so nothing
/// about which other sources exist leaks through it. The source segment compares case-preserved,
/// never folded by the candidate's dialect - a sutura source name is not a column or table
/// identifier at that source, so `doku.DOKU.X` reaches an Oracle source without quoting.
///
/// The returned flag is `true` once any reference in the statement resolved only through the prefix
/// reading: the caller text that follows is no longer admission-checked on its own (a source name is
/// not a thing the source itself knows), so a caller of this module must not treat a later failure
/// the same as an ordinary one that never named a source.
pub(super) fn statement(statement: &mut Statement, tables: &[StatementTable]) -> ResolvedTables {
    resolved(statement, tables, false)
}

pub(super) fn explicit_statement(statement: &mut Statement, tables: &[StatementTable]) -> ResolvedTables {
    resolved(statement, tables, true)
}

fn resolved(statement: &mut Statement, tables: &[StatementTable], explicit: bool) -> ResolvedTables {
    let Statement::Query(query) = statement else {
        return Err(Refused::Unsupported(SqlConstruct::NonQuery));
    };
    let mut resolver = Resolver {
        tables,
        physical: BTreeSet::new(),
        explicit,
        prefixed: false,
    };
    drop(resolver.query(query, &[], &[])?);
    if resolver.physical.is_empty() {
        return Err(Refused::Unsupported(SqlConstruct::SelectWithoutTable));
    }
    Ok((resolver.physical, resolver.prefixed))
}

struct Resolver<'a> {
    tables: &'a [StatementTable],
    physical: BTreeSet<QualifiedTable>,
    explicit: bool,
    /// Whether any reference so far resolved only by naming its source explicitly.
    prefixed: bool,
}

impl Resolver<'_> {
    fn query(&mut self, query: &mut Query, outer: &[Relation], ctes: &[Relation]) -> Resolved<Relation> {
        if query.fetch.is_some() {
            return Err(Refused::Unsupported(SqlConstruct::FetchFirst));
        }
        if !query.locks.is_empty() {
            return Err(Refused::Unsupported(SqlConstruct::NonQuery));
        }
        if !query.pipe_operators.is_empty() {
            return Err(unsupported());
        }
        let mut ctes = ctes.to_vec();
        if let Some(with) = &mut query.with {
            if with.recursive {
                return Err(unsupported());
            }
            for cte in &mut with.cte_tables {
                let relation = self.query(&mut cte.query, outer, &ctes)?.alias(&mut cte.alias);
                ctes.retain(|existing| existing.path != relation.path);
                ctes.push(relation);
            }
        }
        let (outputs, scope) = self.body(&mut query.body, outer, &ctes)?;
        let mut expressions = Expressions::new(self, &scope, outer, &ctes, Some(&outputs));
        continued(query.order_by.visit(&mut expressions))?;
        continued(query.limit_clause.visit(&mut expressions))?;
        Ok(outputs)
    }

    fn body(&mut self, body: &mut SetExpr, outer: &[Relation], ctes: &[Relation]) -> Resolved<Body> {
        match body {
            SetExpr::Select(select) => {
                if select.into.is_some() {
                    return Err(Refused::Unsupported(SqlConstruct::NonQuery));
                }
                let mut scope = Vec::new();
                for from in &mut select.from {
                    self.from(from, outer, ctes, &mut scope)?;
                }
                if self.explicit {
                    let mut projection = Vec::new();
                    for item in std::mem::take(&mut select.projection) {
                        match item {
                            SelectItem::Wildcard(options) if options.to_string().is_empty() => {
                                for relation in &scope {
                                    projection.extend(relation.projection()?);
                                }
                            }
                            SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::ObjectName(name), options)
                                if options.to_string().is_empty() =>
                            {
                                let path = identifiers(&name)?;
                                let relation =
                                    unique(scope.iter().filter(|relation| relation.matches(&path)))?.ok_or_else(unsupported)?;
                                projection.extend(relation.projection()?);
                            }
                            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _) => return Err(unsupported()),
                            other => projection.push(other),
                        }
                    }
                    select.projection = projection;
                }
                let mut expressions = Expressions::new(self, &scope, outer, ctes, None);
                continued(select.projection.visit(&mut expressions))?;
                let mut columns = Vec::new();
                let mut complete = true;
                for item in &mut select.projection {
                    match item {
                        SelectItem::ExprWithAlias { alias, .. } => {
                            normalize(alias);
                            columns.push(Column {
                                name: alias.value.clone(),
                                fold: Fold::Lower,
                            });
                        }
                        SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                            if let Some(column) = column(&[], ident, &scope)? {
                                columns.push(column);
                            }
                        }
                        SelectItem::UnnamedExpr(Expr::CompoundIdentifier(path)) => {
                            if let Some((ident, qualifier)) = path.split_last()
                                && let Some(column) = column(qualifier, ident, &scope)?
                            {
                                columns.push(column);
                            }
                        }
                        SelectItem::Wildcard(_) => {
                            complete &= scope.iter().all(|relation| relation.complete);
                            columns.extend(scope.iter().flat_map(|relation| relation.columns.clone()));
                        }
                        SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::ObjectName(name), _) => {
                            let identifiers = identifiers(name)?;
                            let relation = unique(scope.iter().filter(|relation| relation.matches(&identifiers)))?
                                .ok_or_else(unsupported)?;
                            *name = object_name(&relation.path);
                            complete &= relation.complete;
                            columns.extend(relation.columns.clone());
                        }
                        _ => complete = false,
                    }
                }
                let projection = std::mem::take(&mut select.projection);
                let having = select.having.take();
                let qualify = select.qualify.take();
                let mut expressions = Expressions::new(self, &scope, outer, ctes, None);
                let rest = continued(select.visit(&mut expressions));
                select.projection = projection;
                select.having = having;
                select.qualify = qualify;
                rest?;
                let outputs = Relation {
                    path: Vec::new(),
                    fold: Fold::Lower,
                    columns,
                    complete,
                };
                let mut expressions = Expressions::new(self, &scope, outer, ctes, Some(&outputs));
                continued(select.having.visit(&mut expressions))?;
                continued(select.qualify.visit(&mut expressions))?;
                Ok((outputs, scope))
            }
            SetExpr::Query(query) => Ok((self.query(query, outer, ctes)?, Vec::new())),
            SetExpr::SetOperation { left, right, .. } => {
                let (columns, _) = self.body(left, outer, ctes)?;
                drop(self.body(right, outer, ctes)?);
                Ok((columns, Vec::new()))
            }
            SetExpr::Values(values) => {
                let mut expressions = Expressions::new(self, &[], outer, ctes, None);
                continued(values.visit(&mut expressions))?;
                Ok((
                    Relation {
                        path: Vec::new(),
                        fold: Fold::Lower,
                        columns: Vec::new(),
                        complete: false,
                    },
                    Vec::new(),
                ))
            }
            SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_) => {
                Err(Refused::Unsupported(SqlConstruct::NonQuery))
            }
            SetExpr::Table(_) => Err(unsupported()),
        }
    }

    fn from(
        &mut self,
        from: &mut TableWithJoins,
        outer: &[Relation],
        ctes: &[Relation],
        scope: &mut Vec<Relation>,
    ) -> Resolved<()> {
        let start = scope.len();
        self.factor(&mut from.relation, outer, ctes, scope)?;
        for join in &mut from.joins {
            let right = scope.len();
            self.factor(&mut join.relation, outer, ctes, scope)?;
            let (JoinOperator::Join(constraint)
            | JoinOperator::Inner(constraint)
            | JoinOperator::Left(constraint)
            | JoinOperator::LeftOuter(constraint)
            | JoinOperator::Right(constraint)
            | JoinOperator::RightOuter(constraint)
            | JoinOperator::FullOuter(constraint)
            | JoinOperator::CrossJoin(constraint)
            | JoinOperator::Semi(constraint)
            | JoinOperator::LeftSemi(constraint)
            | JoinOperator::RightSemi(constraint)
            | JoinOperator::Anti(constraint)
            | JoinOperator::LeftAnti(constraint)
            | JoinOperator::RightAnti(constraint)
            | JoinOperator::StraightJoin(constraint)
            | JoinOperator::AsOf { constraint, .. }) = &mut join.join_operator
            else {
                continue;
            };
            if matches!(constraint, JoinConstraint::Using(_) | JoinConstraint::Natural) {
                for relation in scope.iter_mut().skip(start) {
                    relation.complete = false;
                }
            }
            if let JoinConstraint::Using(names) = constraint {
                for name in names {
                    let path = identifiers(name)?;
                    let [ident] = path.as_slice() else {
                        return Err(unsupported());
                    };
                    let left = scope.get(start..right).ok_or_else(unsupported)?;
                    let right = scope.get(right..).ok_or_else(unsupported)?;
                    let left = column(&[], ident, left)?.ok_or(Refused::UnknownColumn)?;
                    let right = column(&[], ident, right)?.ok_or(Refused::UnknownColumn)?;
                    if left.name != right.name {
                        return Err(unsupported());
                    }
                    *name = object_name(&[left.name]);
                }
            }
        }
        Ok(())
    }

    fn factor(
        &mut self,
        factor: &mut TableFactor,
        outer: &[Relation],
        ctes: &[Relation],
        scope: &mut Vec<Relation>,
    ) -> Resolved<()> {
        let (relation, alias) = match factor {
            TableFactor::Table {
                name, alias, args: None, ..
            } => {
                let path = identifiers(name)?;
                if let Some(cte) = unique(ctes.iter().filter(|cte| path.len() == 1 && cte.matches(&path)))? {
                    *name = object_name(&cte.path);
                    (cte.clone(), alias)
                } else {
                    let (table, prefixed) = resolve_table(self.tables, &path)?;
                    self.prefixed |= prefixed;
                    let path = table_path(table);
                    *name = object_name(&registered_path(self.tables, table));
                    self.physical.insert(table.table().clone());
                    let fold = Fold::of(table.dialect());
                    let columns = table
                        .schema()
                        .fields()
                        .iter()
                        .map(|field| Column {
                            name: field.name().clone(),
                            fold,
                        })
                        .collect();
                    (
                        Relation {
                            path,
                            fold,
                            columns,
                            complete: true,
                        },
                        alias,
                    )
                }
            }
            TableFactor::Derived {
                subquery,
                alias,
                lateral: false,
                ..
            } => (self.query(subquery, outer, ctes)?, alias),
            TableFactor::NestedJoin {
                table_with_joins,
                alias: None,
            } => {
                return self.from(table_with_joins, outer, ctes, scope);
            }
            TableFactor::Table { args: Some(_), .. }
            | TableFactor::TableFunction { .. }
            | TableFactor::Function { .. }
            | TableFactor::UNNEST { .. } => return Err(Refused::Unsupported(SqlConstruct::TableFunction)),
            _ => return Err(unsupported()),
        };
        scope.push(if let Some(alias) = alias {
            relation.alias(alias)
        } else {
            relation
        });
        Ok(())
    }
}

/// Does `path` name `table`'s own physical path, folding an unquoted segment the same way `table`'s
/// path already does - per `table`'s dialect, since that is the convention a federated statement
/// already applies per-candidate?
fn matches_physical(table: &StatementTable, path: &[Ident]) -> bool {
    let physical = table_path(table);
    let fold = Fold::of(table.dialect());
    path.len() == physical.len() && path.iter().zip(&physical).all(|(ident, name)| fold.name(ident) == *name)
}

/// Does `path` name `table`'s physical path prefixed with the sutura source it lives at?
///
/// The source segment is a sutura name, not a column or table identifier at `table`'s source, so it
/// compares case-preserved - never folded by `table`'s dialect, or `doku.DOKU.X` could not reach an
/// Oracle source without quoting.
fn matches_prefixed(table: &StatementTable, path: &[Ident]) -> bool {
    let physical = table_path(table);
    let fold = Fold::of(table.dialect());
    path.split_first().is_some_and(|(source, rest)| {
        source.value == table.source().as_str()
            && rest.len() == physical.len()
            && rest.iter().zip(&physical).all(|(ident, name)| fold.name(ident) == *name)
    })
}

/// `path`'s table among `tables`: resolved first against every candidate's own physical path, and
/// only when that phase matches nothing, against the same path prefixed with the table's source.
///
/// Ambiguity within either phase refuses, and the second phase runs only once the first has matched
/// nothing - so a source prefix can only select among candidates no physical path already claims,
/// and an unprefixed reference to a path two sources share still refuses as unknown rather than
/// silently picking one. The second element of the result is whether the match came from the
/// prefixed phase.
fn resolve_table<'a>(tables: &'a [StatementTable], path: &[Ident]) -> Resolved<(&'a StatementTable, bool)> {
    let mut physical = tables.iter().filter(|table| matches_physical(table, path));
    if let Some(table) = physical.next() {
        return if physical.next().is_some() {
            Err(Refused::UnknownTable)
        } else {
            Ok((table, false))
        };
    }
    let mut prefixed = tables.iter().filter(|table| matches_prefixed(table, path));
    let table = prefixed.next().ok_or(Refused::UnknownTable)?;
    if prefixed.next().is_some() {
        return Err(Refused::UnknownTable);
    }
    Ok((table, true))
}

fn table_path(table: &StatementTable) -> Vec<String> {
    let mut path = Vec::new();
    if let Some(qualifier) = table.table().qualifier() {
        if let Some(project) = qualifier.project() {
            path.push(String::from(project.as_str()));
        }
        path.push(String::from(qualifier.dataset().as_str()));
    }
    path.push(String::from(table.table().name().as_str()));
    path
}

/// The physical leaf [`super::session`] registers `table` under: mangled with its source when
/// another admitted table shares the same physical path at a different source, since a session's
/// single per-path catalog slot cannot otherwise hold both. `\u{1}` is not a character any source's
/// own leaf carries, so the mangled leaf never coincides with an unrelated table's own name, and a
/// caller can never type it - the mangled form is reachable only by this module's own rewrite.
pub(super) fn registered_leaf(tables: &[StatementTable], table: &StatementTable) -> String {
    let leaf = String::from(table.table().name().as_str());
    let colliding = tables
        .iter()
        .any(|other| other.table() == table.table() && other.source() != table.source());
    if colliding {
        format!("{}\u{1}{leaf}", table.source().as_str())
    } else {
        leaf
    }
}

/// `table`'s physical path with its leaf replaced by [`registered_leaf`] - the path the AST must
/// name for the planner to resolve to the catalog slot `session` actually registered `table` under.
pub(super) fn registered_path(tables: &[StatementTable], table: &StatementTable) -> Vec<String> {
    let mut path = table_path(table);
    path.pop();
    path.push(registered_leaf(tables, table));
    path
}

fn identifiers(name: &ObjectName) -> Resolved<Vec<Ident>> {
    name.0
        .iter()
        .map(|part| part.as_ident().cloned().ok_or_else(unsupported))
        .collect()
}

fn object_name(path: &[String]) -> ObjectName {
    ObjectName(
        path.iter()
            .map(|part| ObjectNamePart::Identifier(quoted(part.clone())))
            .collect(),
    )
}

fn unique<T>(mut candidates: impl Iterator<Item = T>) -> Resolved<Option<T>> {
    let first = candidates.next();
    if candidates.next().is_some() {
        return Err(unsupported());
    }
    Ok(first)
}

fn column(qualifier: &[Ident], ident: &Ident, scope: &[Relation]) -> Resolved<Option<Column>> {
    let mut candidates = scope
        .iter()
        .filter(|relation| qualifier.is_empty() || relation.matches(qualifier))
        .flat_map(|relation| &relation.columns)
        .filter(|column| column.fold.name(ident) == column.name);
    let first = candidates.next();
    if let Some(first) = first
        && candidates.any(|other| other.name != first.name)
    {
        return Err(unsupported());
    }
    Ok(first.cloned())
}

fn refuse_lowercase_fallback(qualifier: &[Ident], ident: &Ident, scope: &[Relation]) -> Resolved<()> {
    if ident.quote_style.is_none()
        && scope
            .iter()
            .filter(|relation| qualifier.is_empty() || relation.matches(qualifier))
            .flat_map(|relation| &relation.columns)
            .any(|column| {
                matches!(column.fold, Fold::Upper)
                    && Fold::Lower.name(ident) == column.name
                    && Fold::Upper.name(ident) != column.name
            })
    {
        return Err(Refused::UnknownColumn);
    }
    Ok(())
}

const fn continued(flow: ControlFlow<Refused>) -> Resolved<()> {
    match flow {
        ControlFlow::Continue(()) => Ok(()),
        ControlFlow::Break(reason) => Err(reason),
    }
}

struct Expressions<'a, 'b> {
    resolver: &'a mut Resolver<'b>,
    scope: &'a [Relation],
    outer: &'a [Relation],
    ctes: &'a [Relation],
    outputs: Option<&'a Relation>,
    queries: usize,
    factors: usize,
}

impl<'a, 'b> Expressions<'a, 'b> {
    const fn new(
        resolver: &'a mut Resolver<'b>,
        scope: &'a [Relation],
        outer: &'a [Relation],
        ctes: &'a [Relation],
        outputs: Option<&'a Relation>,
    ) -> Self {
        Self {
            resolver,
            scope,
            outer,
            ctes,
            outputs,
            queries: 0,
            factors: 0,
        }
    }

    fn expression(&self, expr: &mut Expr) -> Resolved<()> {
        let (qualifier, ident) = match expr {
            Expr::Identifier(ident) => (&mut [][..], ident),
            Expr::CompoundIdentifier(path) => {
                let Some((ident, qualifier)) = path.split_last_mut() else {
                    return Ok(());
                };
                (qualifier, ident)
            }
            _ => return Ok(()),
        };
        if qualifier.is_empty()
            && let Some(outputs) = self.outputs
            && let Some(output) = column(&[], ident, std::slice::from_ref(outputs))?
        {
            *ident = quoted(output.name);
            return Ok(());
        }
        let resolved = match column(qualifier, ident, self.scope)? {
            Some(column) => Some(column),
            None => column(qualifier, ident, self.outer)?,
        };
        if let Some(column) = resolved {
            *ident = quoted(column.name);
            if !qualifier.is_empty() {
                let relation = match unique(self.scope.iter().filter(|relation| relation.matches(qualifier)))? {
                    Some(relation) => Some(relation),
                    None => unique(self.outer.iter().filter(|relation| relation.matches(qualifier)))?,
                };
                if let Some(relation) = relation {
                    for (ident, name) in qualifier.iter_mut().rev().zip(relation.path.iter().rev()) {
                        *ident = quoted(name.clone());
                    }
                }
            }
        } else {
            refuse_lowercase_fallback(qualifier, ident, self.scope)?;
            refuse_lowercase_fallback(qualifier, ident, self.outer)?;
        }
        Ok(())
    }
}

impl VisitorMut for Expressions<'_, '_> {
    type Break = Refused;

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<Self::Break> {
        if self.queries == 0
            && self.factors == 0
            && let Err(reason) = self.expression(expr)
        {
            return ControlFlow::Break(reason);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, _: &mut TableFactor) -> ControlFlow<Self::Break> {
        self.factors += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_table_factor(&mut self, _: &mut TableFactor) -> ControlFlow<Self::Break> {
        self.factors -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &mut Query) -> ControlFlow<Self::Break> {
        if self.queries == 0 && self.factors == 0 {
            let mut outer = self.scope.to_vec();
            outer.extend_from_slice(self.outer);
            if let Err(reason) = self.resolver.query(query, &outer, self.ctes) {
                return ControlFlow::Break(reason);
            }
        }
        self.queries += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _: &mut Query) -> ControlFlow<Self::Break> {
        self.queries -= 1;
        ControlFlow::Continue(())
    }
}

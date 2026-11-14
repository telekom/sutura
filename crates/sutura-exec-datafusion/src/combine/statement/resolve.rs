//! Resolve only the caller's admitted metadata, before the engine sees a table reference.

use std::ops::ControlFlow;

use datafusion::sql::sqlparser::ast::{
    Expr, Ident, JoinConstraint, JoinOperator, ObjectName, ObjectNamePart, Query, SelectItem, SelectItemQualifiedWildcardKind,
    SetExpr, Statement, TableAlias, TableFactor, TableWithJoins, VisitMut as _, VisitorMut,
};
use sutura_domain::plan::StatementTable;
use sutura_domain::raw::SqlConstruct;
use sutura_domain::warehouse::schema::TypeDialect;

#[derive(Debug, Clone, Copy)]
pub(super) enum Refused {
    UnknownTable,
    UnknownColumn,
    Unsupported(SqlConstruct),
}

type Resolved<T> = Result<T, Refused>;
type Body = (Vec<Column>, Vec<Relation>);

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

    fn alias(mut self, alias: &mut TableAlias) -> Self {
        normalize(&mut alias.name);
        self.path = vec![alias.name.value.clone()];
        self.fold = Fold::Lower;
        if !alias.columns.is_empty() {
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
pub(super) fn statement(statement: &mut Statement, tables: &[StatementTable]) -> Resolved<()> {
    let Statement::Query(query) = statement else {
        return Err(Refused::Unsupported(SqlConstruct::NonQuery));
    };
    let mut resolver = Resolver { tables, physical: 0 };
    drop(resolver.query(query, &[], &[])?);
    if resolver.physical == 0 {
        return Err(Refused::Unsupported(SqlConstruct::SelectWithoutTable));
    }
    Ok(())
}

struct Resolver<'a> {
    tables: &'a [StatementTable],
    physical: usize,
}

impl Resolver<'_> {
    fn query(&mut self, query: &mut Query, outer: &[Relation], ctes: &[Relation]) -> Resolved<Vec<Column>> {
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
                let columns = self.query(&mut cte.query, outer, &ctes)?;
                let relation = Relation {
                    path: Vec::new(),
                    fold: Fold::Lower,
                    columns,
                }
                .alias(&mut cte.alias);
                ctes.retain(|existing| existing.path != relation.path);
                ctes.push(relation);
            }
        }
        let (columns, scope) = self.body(&mut query.body, outer, &ctes)?;
        let outputs = Relation {
            path: Vec::new(),
            fold: Fold::Lower,
            columns: columns.clone(),
        };
        let mut expressions = Expressions::new(self, &scope, outer, &ctes, Some(&outputs));
        continued(query.order_by.visit(&mut expressions))?;
        continued(query.limit_clause.visit(&mut expressions))?;
        Ok(columns)
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
                let mut expressions = Expressions::new(self, &scope, outer, ctes, None);
                continued(select.projection.visit(&mut expressions))?;
                let mut columns = Vec::new();
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
                        SelectItem::Wildcard(_) => columns.extend(scope.iter().flat_map(|relation| relation.columns.clone())),
                        SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::ObjectName(name), _) => {
                            let identifiers = identifiers(name)?;
                            let relation = unique(scope.iter().filter(|relation| relation.matches(&identifiers)))?
                                .ok_or_else(unsupported)?;
                            *name = object_name(&relation.path);
                            columns.extend(relation.columns.clone());
                        }
                        _ => {}
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
                    columns: columns.clone(),
                };
                let mut expressions = Expressions::new(self, &scope, outer, ctes, Some(&outputs));
                continued(select.having.visit(&mut expressions))?;
                continued(select.qualify.visit(&mut expressions))?;
                Ok((columns, scope))
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
                Ok((Vec::new(), Vec::new()))
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
                    let mut candidates = self.tables.iter().filter(|table| {
                        let physical = table_path(table);
                        path.len() == physical.len()
                            && path
                                .iter()
                                .zip(physical)
                                .all(|(ident, name)| Fold::of(table.dialect()).name(ident) == name)
                    });
                    let table = candidates.next().ok_or(Refused::UnknownTable)?;
                    if candidates.next().is_some() {
                        return Err(Refused::UnknownTable);
                    }
                    let path = table_path(table);
                    *name = object_name(&path);
                    self.physical += 1;
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
                    (Relation { path, fold, columns }, alias)
                }
            }
            TableFactor::Derived {
                subquery,
                alias,
                lateral: false,
                ..
            } => {
                let columns = self.query(subquery, outer, ctes)?;
                (
                    Relation {
                        path: Vec::new(),
                        fold: Fold::Lower,
                        columns,
                    },
                    alias,
                )
            }
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

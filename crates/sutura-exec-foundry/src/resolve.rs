//! Which name a statement gives each table: the RID the directory resolves an API name to, after the
//! exclusions are checked on both.

use sutura_domain::identity::Secret;
use sutura_domain::model::{ObjectTypeExclusions, QualifiedTable, ResourceId};
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_sql::ObjectReferences;

use crate::FoundryError;
use crate::directory::{Directory, Resolution, Whose};

/// The references a statement takes, and the API names resolved for it - what is forgotten when
/// Foundry finds no object type behind one of them.
pub(crate) type Resolved = (ObjectReferences, Vec<String>);

/// The API name a table names its object type by, or `None` for one written as a RID.
pub(crate) fn api_name(table: &QualifiedTable) -> Option<String> {
    if table.resource_id().is_some() {
        return None;
    }
    Some(table.object_path().map_or_else(
        || String::from(table.name().as_str()),
        sutura_domain::model::ObjectPath::dotted,
    ))
}

/// The tables `exclusions` leaves out by API name - what a boot pre-flight refuses.
pub(crate) fn excluded<'a>(
    tables: impl IntoIterator<Item = &'a QualifiedTable>,
    exclusions: &ObjectTypeExclusions,
) -> std::collections::BTreeSet<QualifiedTable> {
    tables
        .into_iter()
        .filter(|table| api_name(table).is_some_and(|api_name| exclusions.excludes(&api_name)))
        .cloned()
        .collect()
}

/// The RID each of `tables` named by API name is queried by, read through `directory`'s `whose` map
/// under `token`. A table the exclusions leave out - by API name, or by a RID a read listed as
/// excluded - is refused, as is one whose API name resolves to nothing usable. A refusal names the
/// table as the catalog does, never the API name or the RID.
pub(crate) fn references<E>(
    directory: Option<&Directory>,
    exclusions: &ObjectTypeExclusions,
    whose: Whose,
    tables: &[&QualifiedTable],
    token: &Secret,
    deadline: Deadline,
) -> Result<Resolved, FoundryError<E>>
where
    E: std::error::Error + 'static,
{
    let mut references = ObjectReferences::default();
    let named: Vec<(&QualifiedTable, String)> = tables
        .iter()
        .filter_map(|&table| api_name(table).map(|api_name| (table, api_name)))
        .collect();
    if let Some(&(table, _)) = named.iter().find(|(_, api_name)| exclusions.excludes(api_name)) {
        return Err(FoundryError::ExcludedObjectType {
            table: table.name().clone(),
        });
    }
    let Some(directory) = directory else {
        return Ok((references, Vec::new()));
    };
    let ids: Vec<(&QualifiedTable, &ResourceId)> = tables
        .iter()
        .filter_map(|&table| table.resource_id().map(|id| (table, id)))
        .collect();
    if named.is_empty() && ids.is_empty() {
        return Ok((references, Vec::new()));
    }
    let api_names: Vec<&str> = named.iter().map(|(_, api_name)| api_name.as_str()).collect();
    let asked: Vec<&ResourceId> = ids.iter().map(|&(_, id)| id).collect();
    let lookup = directory
        .lookup(whose, &api_names, &asked, token, deadline)
        .map_err(|cause| FoundryError::Directory { cause })?;
    if let Some(&(table, _)) = ids.iter().find(|&&(_, id)| lookup.excludes(id)) {
        return Err(FoundryError::ExcludedObjectType {
            table: table.name().clone(),
        });
    }
    let mut resolved = Vec::with_capacity(named.len());
    for (table, api_name) in named {
        let table_name = || table.name().clone();
        match lookup.resolved(&api_name) {
            Resolution::Id(id) => {
                references.insert(table.name().clone(), id);
                resolved.push(api_name);
            }
            Resolution::Unusable => return Err(FoundryError::NoResourceId { table: table_name() }),
            Resolution::Unlisted => return Err(FoundryError::UnknownObjectType { table: table_name() }),
        }
    }
    Ok((references, resolved))
}

/// Every table `executable` names: its own, and each one it joins.
pub(crate) fn tables_of(executable: Executable<'_>) -> Vec<&QualifiedTable> {
    match executable {
        Executable::Query(plan) => std::iter::once(plan.table())
            .chain(plan.joins().iter().map(sutura_domain::plan::PlanJoin::table))
            .collect(),
        Executable::Leg(leg) => std::iter::once(leg.table())
            .chain(leg.tables().joins().iter().map(sutura_domain::plan::PlanJoin::table))
            .collect(),
        Executable::Stacked(plan) => plan
            .arms()
            .iter()
            .flat_map(|arm| std::iter::once(arm.table()).chain(arm.joins().iter().map(sutura_domain::plan::PlanJoin::table)))
            .collect(),
    }
}

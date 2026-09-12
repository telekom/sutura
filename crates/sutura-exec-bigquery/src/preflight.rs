//! The boot pre-flight: ask each dataset once whether it holds the tables the bundle names.
//!
//! Its own module, delegated to from the [`Warehouse`](sutura_domain::warehouse::Warehouse) impl
//! the way `crate::identity_read` is: nothing here is about a plan, a credential or a row, and the
//! vocabulary a short listing needs ([`Gap`]) is used by nothing else. **What a gap does NOT
//! establish is that a table is absent** - it only BOUNDS the answer; see [`preflight`].

use core::num::NonZeroU64;
use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::model::QualifiedTable;
use sutura_domain::warehouse::preflight::{TablesPresent, UnaccountedTables};

use crate::transport::{DatasetAddress, DatasetId, JobTransport, ListingTotal, ProjectId};
use crate::{BigQueryError, BigQueryWarehouse, Mapped};

/// The bundle's tables, grouped by the dataset each resolves in.
///
/// Named for the reason [`Mapped`] is: the map is over the `type_complexity` threshold this
/// workspace tightened, and *by dataset* is what it means where the spelled-out type is not.
type ByDataset<'bundle> = BTreeMap<DatasetAddress, Vec<&'bundle QualifiedTable>>;

/// What a pre-flight's short listings left behind: the tables none of them named, and how many
/// tables those listings left out of their own totals.
///
/// **A struct and not two locals, which is a review finding rather than a nod to
/// `type_complexity`.** Two bindings is how a shortfall of one comes to describe three tables, and
/// how a count derived by arithmetic gets to disagree with the set beside it - both shapes were
/// reproduced. Here they are written together or not at all, and the count is a [`NonZeroU64`], so
/// it cannot arrive as the zero that used to route this answer back to *these tables are absent*.
struct Gap {
    /// The tables no short listing named. Non-empty: this type is built only when one is added.
    unaccounted_for: BTreeSet<QualifiedTable>,
    /// How many tables those listings claim that no readable id accounted for.
    shortfall: NonZeroU64,
}

impl Gap {
    /// One dataset's own gap, folded into whatever the earlier datasets left.
    fn widened(gap: Option<Self>, unnamed: BTreeSet<QualifiedTable>, by: NonZeroU64) -> Self {
        match gap {
            None => Self {
                unaccounted_for: unnamed,
                shortfall: by,
            },
            Some(mut so_far) => {
                so_far.unaccounted_for.extend(unnamed);
                so_far.shortfall = so_far.shortfall.saturating_add(by.get());
                so_far
            }
        }
    }
}

/// Which dataset one model's table path resolves in, or `None` if this adapter cannot address it.
///
/// **The unqualified case is the connection's own pair and not a guess**, which is the same
/// decision the request body's `defaultDataset` carries: a bare table name resolves in the
/// dataset the source was opened against, inside the project the job is billed to. A qualified
/// path overrides one or both, and each part is re-parsed by the type for its position, because
/// the value written into a request path is this crate's to accept or refuse.
///
/// **`None` rather than an `Err`, and the measurement is why.** An `Err` here propagated out of
/// [`preflight`]'s grouping loop **before any dataset was listed** - so one mixed-case
/// project id in a forty-model bundle turned the whole check off for that source, as a single
/// `WARN`. A path this adapter cannot write into a request is a **definite negative and not an
/// unknown**: no question against that model could ever be answered, whatever the dataset holds.
/// So it is an ABSENCE, it reaches the operator as a refusal naming the model, and it costs no
/// round trip.
fn addressed<T>(warehouse: &BigQueryWarehouse<T>, table: &QualifiedTable) -> Option<DatasetAddress>
where
    T: JobTransport,
{
    let billed_to = warehouse.billing_project.clone();
    let Some(qualifier) = table.qualifier() else {
        return Some(DatasetAddress::of(
            billed_to,
            warehouse.billing_project.clone(),
            warehouse.default_dataset.clone(),
        ));
    };
    let project = match qualifier.project() {
        None => warehouse.billing_project.clone(),
        Some(project) => ProjectId::parse(project.as_str()).ok()?,
    };
    let dataset = DatasetId::parse(qualifier.dataset().as_str()).ok()?;
    Some(DatasetAddress::of(billed_to, project, dataset))
}

/// Asks the dataset which of the bundle's tables it holds.
///
/// **One call per DATASET and not per model, which is what makes this affordable at boot.** The
/// tables asked about are grouped by the pair they resolve in - the connection's own project and
/// dataset for an unqualified path, whatever the path names otherwise - and each group costs one
/// metadata read.
///
/// **The comparison is case-SENSITIVE, deliberately.** `GoogleSQL` folds the case of an alias and
/// a result column and does not fold a table name, so a model naming `Dim_Customer` where the
/// dataset holds `dim_customer` is a model whose questions really would fail - reporting it
/// present because a case-folded comparison matched would put the failure back on the first
/// caller, which is the whole defect this method exists to remove.
///
/// **A path this adapter cannot ADDRESS is an absence and never an error** - see
/// [`addressed`] for the measurement of what the other shape cost.
///
/// **A listing SHORT of its own total answers about the tables it named and about no others.**
/// A table the short listing named is present, and a table it did not name is
/// [`TablesPresent::Unaccounted`] rather than absent, because the listing has a gap the table
/// could be sitting in. Before this the gap was rounded down to zero and the bundle was charged
/// for it - a dataset answering with no readable id beside a non-zero total refused the boot
/// saying every table it names is missing. `telekom/sutura#275`.
///
/// An unreadable total beside zero readable IDs answers [`TablesPresent::UnreadableInventory`]
/// without inventing a count. Readable IDs rejected by name filtering still count as identified.
/// `Unreported` beside no ids is where every boot stood before the field was decoded, and
/// `Accounted` beside no NAMED ids is a dataset every id of which `usable_table_id` drops.
///
/// **What a gap does NOT establish:** this adapter cannot tell a document whose shape changed
/// from a table created or dropped while the listing was being read, and does not pretend to.
/// Within one dataset the gap only BOUNDS the answer - a shortfall of one over three unnamed
/// tables means two of them really are missing, and nothing here can say which, so both numbers
/// travel.
///
/// # Errors
///
/// [`BigQueryError::Endpoint`] where a dataset could not be listed - no permission, no such
/// dataset, no answer - which the port keeps distinct from a table that is absent so an operator
/// is not sent to fix the wrong thing. Which of those it was is
/// [`Warehouse::preflight_was_refused`](sutura_domain::warehouse::Warehouse::preflight_was_refused)'s
/// question, because only a permission failure is worth stopping a boot for.
pub(super) fn preflight<T>(warehouse: &BigQueryWarehouse<T>, tables: &BTreeSet<QualifiedTable>) -> Mapped<TablesPresent, T::Error>
where
    T: JobTransport,
{
    // **`NotAsked` and not `All` for an empty set**, which is a review nit worth taking: `All`
    // means *asked, nothing missing*, and nothing was asked. The composition root guards this
    // case, so what the arm buys is that any other caller gets the honest answer.
    if tables.is_empty() {
        return Ok(TablesPresent::NotAsked);
    }
    let mut grouped: ByDataset<'_> = BTreeMap::new();
    let mut absent: BTreeSet<QualifiedTable> = BTreeSet::new();
    let mut unreadable: BTreeSet<QualifiedTable> = BTreeSet::new();
    // **The gap: a set and a count in ONE binding, written together or not at all** - see
    // [`Gap`]. A table a short listing did not name is not a table the dataset does not hold,
    // and *how many the listing left out* is a fact about the listing rather than about the
    // bundle, so neither number can be recovered from the other.
    let mut gap: Option<Gap> = None;
    for table in tables {
        // Partitioned BEFORE anything is listed, so an unaddressable path can neither skip the
        // loop nor cost a call: it is already an answer.
        match addressed(warehouse, table) {
            Some(at) => grouped.entry(at).or_default().push(table),
            None => {
                absent.insert(table.clone());
            }
        }
    }
    for (at, asked) in grouped {
        let held = warehouse
            .transport
            .list_tables(&at)
            .map_err(|cause| BigQueryError::Endpoint { cause })?;
        let unnamed = asked.into_iter().filter(|table| !held.holds(table.name().as_str())).cloned();
        match held.total() {
            // **The cross-check, connected.** A listing that reported more tables than it
            // carried readable ids for has a gap in it, and a table the bundle names that this
            // listing did not name may be sitting in that gap - so it is unaccounted for and
            // NOT absent. `telekom/sutura#275` is the decision; `docs/adr/0018` carries why it
            // is a value on the answer rather than an `Err`.
            ListingTotal::Short(short) => {
                let unnamed: BTreeSet<QualifiedTable> = unnamed.collect();
                // A listing that fell short and still named everything the bundle asks about
                // costs this deployment nothing: a short listing cannot un-name an entry it
                // carried, so those tables really are there and this dataset contributes no
                // shortfall to reason about.
                if !unnamed.is_empty() {
                    gap = Some(Gap::widened(gap, unnamed, short.unaccounted()));
                }
            }
            ListingTotal::Unreadable { identified: 0 } => unreadable.extend(unnamed),
            // Readable IDs survive this decision even when name filtering drops all of them.
            ListingTotal::Accounted { .. } | ListingTotal::Unreported | ListingTotal::Unreadable { .. } => {
                absent.extend(unnamed);
            }
        }
    }
    // Among successful listings, report a definite absence first: it gives an operator a table
    // to fix. Unreadable inventories and counted gaps also stop startup and wait for the next
    // boot. A transport error above still short-circuits the walk; this orders answers only.
    if !absent.is_empty() {
        return Ok(TablesPresent::of(absent));
    }
    // Among answered inventories, diagnose an unreadable one before a counted gap. Keep its
    // tables separate: the unreadable total says nothing about another dataset's shortfall.
    if let Ok(tables) = UnaccountedTables::parse(unreadable) {
        return Ok(TablesPresent::UnreadableInventory(tables));
    }
    // **There is no route from here to `AllBut`, and that is the point.** A count arriving as
    // zero used to fall back to `TablesPresent::of(unaccounted_for)` - the defect being fixed.
    // The count is `Shortfall`'s now, so a zero cannot arrive; the only reading left for an
    // absent gap is that no listing fell short.
    let Some(gap) = gap else {
        return Ok(TablesPresent::All);
    };
    Ok(
        UnaccountedTables::parse(gap.unaccounted_for).map_or(TablesPresent::All, |tables| TablesPresent::Unaccounted {
            tables,
            shortfall: gap.shortfall,
        }),
    )
}

//! The object types one caller can see, listed with that caller's own Foundry token.
//!
//! The [`CallerListing`](sutura_domain::visibility::CallerListing) `docs/adr/20261125083027-catalog-visibility-per-caller.md` decides.
//! Dials only the object-type list the catalog reader dials, at the same [`Endpoint`](crate::http::Endpoint), under the
//! same bounds; never the link types. The bearer is the [`Presented::SubjectToken`](sutura_domain::identity::Presented::SubjectToken) a broker minted
//! for this caller, borrowed as a [`BorrowedBearer`](crate::http::BorrowedBearer) for the one request and never kept or cloned.
//! An object type is listed under both names a model may give it: its API name's table, and its
//! resource identifier's. An API name the mapping refuses is not listed by that name - no model can
//! name it so.
//!
//! The same list answers [`CallerObjectTypes::resource_ids`](crate::caller::CallerObjectTypes::resource_ids), which resolves an API name to the RID
//! Ontology SQL queries it by, under whatever token the question it serves is about to send. An
//! object type listed without a usable RID is set aside by name rather than failing the read, so
//! only a question that needs it is refused.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use sutura_domain::identity::Presented;
use sutura_domain::identity::Secret;
use sutura_domain::model::{ObjectTypeExclusions, QualifiedTable, ResourceId};
use sutura_domain::visibility::{CallerListing, VisibleTables};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_http_client::Budget;
use sutura_http_client::token::{BorrowedBearer, UnusableToken};

use crate::document::ObjectType;
use crate::http::{Credential, Endpoint, HttpReaderError, OntologyApi, OntologyIdentifier, ReadBounds, RotatingAgent};

/// Why one caller's object types were not listed. Carries no token and no answer text outside
/// [`HttpReaderError`]'s own bounded fields.
#[derive(Debug, thiserror::Error)]
pub enum CallerListingError {
    /// The leg is not a caller's own token: a source that holds a deployment identity has nothing
    /// to list as the caller with.
    #[error("the credential presented for this listing is not a caller's own token")]
    NotACallerToken,
    #[error("the caller's token cannot be sent as a bearer")]
    Unusable(#[source] UnusableToken),
    #[error("the request deadline was spent before the object types were listed")]
    DeadlineSpent,
    #[error("the caller's object types were not listed")]
    Read(#[source] HttpReaderError),
}

/// Each object type's resource identifier, by its API name.
pub type ResourceIds = BTreeMap<String, ResourceId>;

/// [`ResolvedList`]'s parts: the RIDs by API name, the API names listed with no usable RID, and
/// the RIDs of the object types the exclusions leave out.
pub type ResolvedParts = (ResourceIds, BTreeSet<String>, BTreeSet<ResourceId>);

/// One read of the object-type list, as what a statement may name.
#[derive(Debug, Default)]
pub struct ResolvedList {
    ids: ResourceIds,
    unusable: BTreeSet<String>,
    excluded: BTreeSet<ResourceId>,
}

impl ResolvedList {
    #[must_use]
    pub fn into_parts(self) -> ResolvedParts {
        (self.ids, self.unusable, self.excluded)
    }
}

/// Foundry's object-type list, asked under each caller's own token.
#[derive(Debug)]
pub struct CallerObjectTypes {
    api: OntologyApi,
    exclusions: ObjectTypeExclusions,
}

impl CallerObjectTypes {
    #[must_use]
    pub const fn new(endpoint: Endpoint, ontology: OntologyIdentifier, bounds: ReadBounds, agent: RotatingAgent) -> Self {
        Self {
            api: OntologyApi::new(endpoint, ontology, bounds, agent),
            exclusions: ObjectTypeExclusions::none(),
        }
    }

    /// Leaves out the object types `exclusions` matches: neither listed as visible nor resolved.
    #[must_use]
    pub fn excluding(mut self, exclusions: ObjectTypeExclusions) -> Self {
        self.exclusions = exclusions;
        self
    }

    /// Every object type `token` lists, read inside `deadline`, exclusions and all.
    fn every(&self, token: &Secret, deadline: Deadline) -> Result<Vec<ObjectType>, CallerListingError> {
        let left = deadline
            .remaining_at(Instant::now())
            .ok_or(CallerListingError::DeadlineSpent)?;
        let bearer = BorrowedBearer::of(token).map_err(CallerListingError::Unusable)?;
        let budget = Budget::opened(left.min(self.api.bounds().timeout()));
        self.api
            .object_types(Credential::Borrowed(bearer), budget)
            .map_err(CallerListingError::Read)
    }

    /// Every object type `token` lists that the exclusions keep.
    fn listed(&self, token: &Secret, deadline: Deadline) -> Result<Vec<ObjectType>, CallerListingError> {
        self.every(token, deadline)
            .map(|object_types| crate::kept(object_types, &self.exclusions).0)
    }

    /// Each kept object type `token` lists, by API name, with the resource identifier Ontology SQL
    /// resolves it by; each one listed with no usable RID, by API name - counted, and named at
    /// `DEBUG`; and the RIDs of the object types the exclusions leave out, so a statement naming one
    /// by RID is refused as one naming it by API name is.
    ///
    /// # Errors
    ///
    /// [`CallerListingError`] when the list was not read.
    pub fn resource_ids(&self, token: &Secret, deadline: Deadline) -> Result<ResolvedList, CallerListingError> {
        let mut list = ResolvedList::default();
        for object_type in self.every(token, deadline)? {
            let api_name = object_type.api_name();
            let id = object_type.rid().and_then(|rid| ResourceId::parse(rid).ok());
            match (self.exclusions.excludes(api_name), id) {
                (true, Some(id)) => {
                    list.excluded.insert(id);
                }
                (true, None) => {}
                (false, Some(id)) => {
                    list.ids.insert(String::from(api_name), id);
                }
                (false, None) => {
                    list.unusable.insert(String::from(api_name));
                }
            }
        }
        if !list.unusable.is_empty() {
            tracing::debug!(count = list.unusable.len(), api_names = ?list.unusable, "foundry object types listed with no usable resource identifier");
        }
        Ok(list)
    }
}

impl CallerListing for CallerObjectTypes {
    type Error = CallerListingError;

    fn visible_to(&self, credential: &Presented, deadline: Deadline) -> Result<VisibleTables, Self::Error> {
        let Presented::SubjectToken { material } = credential else {
            return Err(CallerListingError::NotACallerToken);
        };
        let object_types = self.listed(material, deadline)?;
        let by_api_name = object_types
            .iter()
            .filter_map(|object_type| crate::object_table(object_type.api_name()).ok());
        let by_resource_id = object_types
            .iter()
            .filter_map(|object_type| ResourceId::parse(object_type.rid()?).ok())
            .map(QualifiedTable::resource);
        Ok(VisibleTables::of(by_api_name.chain(by_resource_id).collect::<BTreeSet<_>>()))
    }
}

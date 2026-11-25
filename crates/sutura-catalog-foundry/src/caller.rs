//! The object types one caller can see, listed with that caller's own Foundry token.
//!
//! The [`CallerListing`] `docs/adr/20261125083027-catalog-visibility-per-caller.md` decides.
//! Dials only the object-type list the catalog reader dials, at the same [`Endpoint`], under the
//! same bounds; never the link types. The bearer is the [`Presented::SubjectToken`] a broker minted
//! for this caller, sent as it is and never kept: each call builds a fixed [`TokenSource`] and drops
//! it. An API name the identifier grammar refuses is not listed - no model can name it, so leaving
//! it out hides nothing the bundle holds.

use std::collections::BTreeSet;
use std::time::Instant;

use sutura_domain::identity::Presented;
use sutura_domain::visibility::{CallerListing, VisibleTables};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_http_client::Budget;
use sutura_http_client::token::{TokenSource, UnusableToken};

use crate::http::{Endpoint, HttpReaderError, OntologyApi, OntologyIdentifier, ReadBounds, RotatingAgent};

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

/// Foundry's object-type list, asked under each caller's own token.
#[derive(Debug)]
pub struct CallerObjectTypes {
    api: OntologyApi,
}

impl CallerObjectTypes {
    #[must_use]
    pub const fn new(endpoint: Endpoint, ontology: OntologyIdentifier, bounds: ReadBounds, agent: RotatingAgent) -> Self {
        Self {
            api: OntologyApi::new(endpoint, ontology, bounds, agent),
        }
    }
}

impl CallerListing for CallerObjectTypes {
    type Error = CallerListingError;

    fn visible_to(&self, credential: &Presented, deadline: Deadline) -> Result<VisibleTables, Self::Error> {
        let Presented::SubjectToken { material } = credential else {
            return Err(CallerListingError::NotACallerToken);
        };
        let left = deadline
            .remaining_at(Instant::now())
            .ok_or(CallerListingError::DeadlineSpent)?;
        let token = TokenSource::fixed(material.clone()).map_err(CallerListingError::Unusable)?;
        let budget = Budget::opened(left.min(self.api.bounds().timeout()));
        let object_types = self.api.object_types(&token, budget).map_err(CallerListingError::Read)?;
        Ok(VisibleTables::of(
            object_types
                .iter()
                .filter_map(|object_type| crate::object_table(object_type.api_name()).ok())
                .collect::<BTreeSet<_>>(),
        ))
    }
}

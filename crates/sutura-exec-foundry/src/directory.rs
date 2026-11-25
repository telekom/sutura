//! Each object type's resource identifier, by API name: the one name Ontology SQL resolves an
//! object type by.
//!
//! # What is shared, and what a caller can tell from it
//!
//! A source keeps **two** maps, never one: the one questions read, filled only by reads made under a
//! question's own token, and the one anchors read, filled only by reads under the startup identity
//! ([`Whose`]). Within the questions' map an entry is one API name's RID, kept for [`DIRECTORY_TTL`]
//! from the read that last returned it; a read adds and refreshes entries and never replaces the
//! map, so one caller's narrower list does not evict what another's returned. A name the map does
//! not hold, or holds stale, is read again at once under the asking token.
//!
//! **An entry another caller's read added is a hit for a caller who cannot see that object type.**
//! The statement then names a RID their own list would not have given them, and Foundry, under their
//! own token, decides. sutura answers that caller the same refusal either way: a name their read
//! lacks is [`crate::FoundryError::UnknownObjectType`], and Foundry not finding a RID the statement
//! named is [`crate::FoundryError::StaleObjectType`] - both `source_refused`, naming no object type.
//! **The limit:** which status Foundry answers for a RID the token may not see is unverified; a `401`
//! or `403` is the same refusal, a `404` or an `errorName` ending `NotFound` is read as *not found*,
//! and any other answer surfaces as the data system failing. A RID is metadata, not row data, and no
//! row is read except under the caller's own token.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use sutura_domain::identity::Secret;
use sutura_domain::model::ResourceId;
use sutura_domain::warehouse::deadline::Deadline;

/// Foundry's object-type list, read for each object type's resource identifier under one token.
///
/// **Why it exists:** Ontology SQL resolves an object type by its RID, not by its API name, so a
/// model a catalog names by API name is queried by the RID this list returns for it. The
/// composition root implements it over the catalog reader's own list request.
pub trait ObjectTypeDirectory: core::fmt::Debug + Send + Sync {
    /// One read of the list under `token`.
    ///
    /// # Errors
    ///
    /// [`DirectoryFailed`] when the list was not read whole.
    fn listed(&self, token: &Secret, deadline: Deadline) -> Result<Listed, DirectoryFailed>;
}

/// Each object type's resource identifier, by its API name.
pub type ResourceIds = BTreeMap<String, ResourceId>;

/// One read of the object-type list: the RID of each kept object type by API name, the API names
/// listed with no usable RID, and the RIDs of the object types the exclusions leave out.
#[derive(Debug, Default)]
pub struct Listed {
    ids: ResourceIds,
    unusable: BTreeSet<String>,
    excluded: BTreeSet<ResourceId>,
}

impl Listed {
    #[must_use]
    pub const fn new(ids: ResourceIds, unusable: BTreeSet<String>, excluded: BTreeSet<ResourceId>) -> Self {
        Self { ids, unusable, excluded }
    }
}

/// Why the object-type list was not read. Carries no token and no answer text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DirectoryFailed {
    #[error("Foundry refused the object-type list with {status}")]
    Refused { status: u16 },
    #[error("the object-type list was not read whole")]
    Unread,
}

/// How long one entry answers its API name after the read that last returned it.
pub const DIRECTORY_TTL: core::time::Duration = core::time::Duration::from_secs(300);

/// Which map a lookup reads and fills: a question's, under its own token, or an anchor's, under the
/// startup identity. Never both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Whose {
    Question,
    Boot,
}

/// What one API name resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    Id(ResourceId),
    /// Listed, with no usable RID.
    Unusable,
    /// Not in any fresh read.
    Unlisted,
}

/// What a map holds for one API name.
#[derive(Debug, Clone)]
enum Known {
    Id(ResourceId),
    Unusable,
}

/// Each API name's entry and when a read last returned it.
type Entries = BTreeMap<String, (Instant, Known)>;

/// The entries of one map and when each was last returned.
#[derive(Debug, Default)]
struct Held {
    names: Entries,
    excluded: BTreeMap<ResourceId, Instant>,
    read_at: Option<Instant>,
}

impl Held {
    fn fresh(at: Instant) -> bool {
        at.elapsed() < DIRECTORY_TTL
    }

    /// Whether every one of `api_names` is held fresh, and - when `excluded_too` - a read is.
    fn answers<'a>(&self, mut api_names: impl Iterator<Item = &'a str>, excluded_too: bool) -> bool {
        api_names.all(|api_name| self.names.get(api_name).is_some_and(|&(at, _)| Self::fresh(at)))
            && (!excluded_too || self.read_at.is_some_and(Self::fresh))
    }

    fn merge(&mut self, listed: Listed) {
        let now = Instant::now();
        self.names.retain(|_, &mut (at, _)| Self::fresh(at));
        self.excluded.retain(|_, at| Self::fresh(*at));
        for (api_name, id) in listed.ids {
            self.names.insert(api_name, (now, Known::Id(id)));
        }
        for api_name in listed.unusable {
            self.names.insert(api_name, (now, Known::Unusable));
        }
        for id in listed.excluded {
            self.excluded.insert(id, now);
        }
        self.read_at = Some(now);
    }

    fn resolved(&self, api_name: &str) -> Resolution {
        match self.names.get(api_name) {
            Some(&(at, Known::Id(ref id))) if Self::fresh(at) => Resolution::Id(id.clone()),
            Some(&(at, Known::Unusable)) if Self::fresh(at) => Resolution::Unusable,
            Some(_) | None => Resolution::Unlisted,
        }
    }

    fn excludes(&self, id: &ResourceId) -> bool {
        self.excluded.get(id).is_some_and(|&at| Self::fresh(at))
    }
}

/// What one lookup answered: each asked API name's resolution, and which asked RIDs are excluded.
#[derive(Debug, Default)]
pub(crate) struct Lookup {
    names: BTreeMap<String, Resolution>,
    excluded: BTreeSet<ResourceId>,
}

impl Lookup {
    pub(crate) fn resolved(&self, api_name: &str) -> Resolution {
        self.names.get(api_name).cloned().unwrap_or(Resolution::Unlisted)
    }

    pub(crate) fn excludes(&self, id: &ResourceId) -> bool {
        self.excluded.contains(id)
    }
}

/// The object-type list and the two maps over it - see the module header.
#[derive(Debug)]
pub(crate) struct Directory {
    lister: Box<dyn ObjectTypeDirectory>,
    questions: parking_lot::Mutex<Held>,
    boot: parking_lot::Mutex<Held>,
}

impl Directory {
    pub(crate) fn new(lister: Box<dyn ObjectTypeDirectory>) -> Self {
        Self {
            lister,
            questions: parking_lot::Mutex::new(Held::default()),
            boot: parking_lot::Mutex::new(Held::default()),
        }
    }

    const fn held(&self, whose: Whose) -> &parking_lot::Mutex<Held> {
        match whose {
            Whose::Question => &self.questions,
            Whose::Boot => &self.boot,
        }
    }

    /// Resolves `api_names` and checks `ids` against the exclusions, from `whose` map while it holds
    /// them fresh, else after one read under `token` merged into it.
    pub(crate) fn lookup(
        &self,
        whose: Whose,
        api_names: &[&str],
        ids: &[&ResourceId],
        token: &Secret,
        deadline: Deadline,
    ) -> Result<Lookup, DirectoryFailed> {
        let held = self.held(whose);
        let answered = held.lock().answers(api_names.iter().copied(), !ids.is_empty());
        if !answered {
            let listed = self.lister.listed(token, deadline)?;
            held.lock().merge(listed);
        }
        let held = held.lock();
        Ok(Lookup {
            names: api_names
                .iter()
                .map(|&api_name| (String::from(api_name), held.resolved(api_name)))
                .collect(),
            excluded: ids.iter().copied().filter(|id| held.excludes(id)).cloned().collect(),
        })
    }

    /// Drops `api_names` from `whose` map, so the next lookup reads them again.
    pub(crate) fn forget(&self, whose: Whose, api_names: &[String]) {
        let mut held = self.held(whose).lock();
        for api_name in api_names {
            held.names.remove(api_name);
        }
    }
}

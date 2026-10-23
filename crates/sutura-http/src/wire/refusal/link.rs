//! The sentences for a crossing into a second data system whose join key cannot be built, each
//! answered as a `409`.

use sutura_domain::model::RelationshipName;
use sutura_domain::plan::QuotedColumnType;

pub(super) fn link_type_mismatch(relationship: &RelationshipName, fact: &QuotedColumnType, lookup: &QuotedColumnType) -> String {
    format!(
        "`{relationship}` joins a column declared {fact} to one declared {lookup}, two kinds of value \
         that can never match; report it to a person"
    )
}

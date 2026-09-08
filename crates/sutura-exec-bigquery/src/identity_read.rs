//! Read the endpoint's identity under the leg's presented credential.

use sutura_domain::identity::Presented;

use crate::transport::{Cell, JobRequest, JobTransport};
use crate::{BigQueryError, BigQueryWarehouse, Mapped};

/// The one statement this adapter issues that asks the endpoint about the CALLER rather than about
/// data: who does this data system believe is executing this job?
///
/// **A constant and not a parameter, which is what keeps *no arbitrary SQL entry point* true of this
/// crate.** `crate::transport`'s header states that rule and [`BigQueryWarehouse::load_fixture`]
/// keeps it by taking a table name and a path; this keeps it by taking nothing at all. There is
/// exactly one thing [`BigQueryWarehouse::session_user`] can ask, and it is written here.
///
/// `SESSION_USER()` is `GoogleSQL`'s own reading of the authenticated identity, so it reads the
/// identity WITHOUT changing it - which is the property that makes it usable as the observable for
/// *this source executed as that principal*. `docs/adr/0008` already reasons about it as the
/// identity-reading primitive.
///
/// **What it cannot do, said where the constant is:** it reports the identity the ENDPOINT resolved
/// the job's bearer to. It says nothing about how that bearer was obtained, so a bearer minted from
/// a key on disk and one exchanged for a caller are indistinguishable here. That distinction is the
/// composition's, and `docs/where-identity-is-proven.md` is where it is kept.
const SESSION_USER: &str = "SELECT SESSION_USER() AS session_user";

pub(super) fn session_user<T>(warehouse: &BigQueryWarehouse<T>, presented: &Presented) -> Mapped<String, T::Error>
where
    T: JobTransport,
{
    warehouse.deliverable(presented)?;
    // No parameters, for `load_fixture`'s reason inverted: there is no question to carry a
    // value FROM. And the subject's bearer where the leg has one, which is the entire point -
    // an identity read submitted under the transport's own credential would answer the
    // transport, every time, and pass.
    let request = JobRequest::new(
        SESSION_USER,
        &[],
        &warehouse.billing_project,
        &warehouse.default_dataset,
        BigQueryWarehouse::<T>::subject_bearer(presented),
    );
    let answered = warehouse
        .transport
        .run(&request)
        .map_err(|cause| BigQueryError::Endpoint { cause })?;
    // The SAME comparison `BigQueryWarehouse::rows` makes, asked of the same type rather than written a
    // second time: a delivered count that is not the reported total is a partial answer, and a
    // partial answer to *who am I* is a wrong identity.
    if answered.rows().len() != answered.total_rows() {
        return Err(BigQueryError::Incomplete {
            delivered: answered.rows().len(),
            total: answered.total_rows(),
        });
    }
    let shape = || BigQueryError::NoIdentityInTheAnswer {
        rows: answered.rows().len(),
        columns: answered.fields().len(),
    };
    // Slice patterns rather than indexing, because `clippy::indexing_slicing` is denied here and
    // is right to be: a shape this adapter did not expect must be a refusal and never a panic.
    let ([row], [_]) = (answered.rows(), answered.fields()) else {
        return Err(shape());
    };
    let [Cell::Text(who)] = row.as_slice() else {
        return Err(shape());
    };
    Ok(who.clone())
}

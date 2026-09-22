//! Read the endpoint's identity under the leg's presented credential.

use sutura_domain::identity::Presented;
use sutura_domain::warehouse::Value;

use crate::transport::{JobDeadline, JobRequest, JobTransport};
use crate::{BigQueryError, BigQueryWarehouse, Mapped};

/// An endpoint identity answer whose `Debug` never renders its contents.
///
/// [`Self::as_str`] and `Display` expose the unchanged answer. This is a `Debug` boundary,
/// not a restriction on intentional logging, nor validation or authentication of the identity.
pub struct SessionUser(String);

impl SessionUser {
    /// Retains an identity answer unchanged, without validating its spelling or provenance.
    #[must_use]
    pub const fn new(answer: String) -> Self {
        Self(answer)
    }

    /// Borrows the answer for a caller that has decided it may inspect or render it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for SessionUser {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for SessionUser {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SessionUser(<redacted>)")
    }
}

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

pub(super) fn session_user<T>(warehouse: &BigQueryWarehouse<T>, presented: &Presented) -> Mapped<SessionUser, T::Error>
where
    T: JobTransport,
{
    warehouse.deliverable(presented)?;
    // No parameters, for `load_fixture`'s reason inverted: there is no question to carry a
    // value FROM. And the leg's own identity where it names one, which is the entire point -
    // an identity read submitted under the transport's own identity would answer the
    // transport, every time, and pass.
    // No port `Deadline`: this read is not part of the `Warehouse` port and has no caller's request
    // timeout to answer to, so it is the boot path's own shape - `submit` opens a fresh window from
    // this transport's configured job bounds, exactly as it did before this parameter existed.
    let request = JobRequest::new(
        SESSION_USER,
        &[],
        &warehouse.billing_project,
        &warehouse.default_dataset,
        BigQueryWarehouse::<T>::job_identity(presented, &warehouse.source)?,
        JobDeadline::Boot,
    );
    let answered = warehouse
        .transport
        .run(&request)
        .map_err(|cause| BigQueryError::Endpoint { cause })?;
    // ONE CELL, read through the interior's own `scalar`, which answers only for a result that is
    // exactly one row of one column. `docs/adr/0039` put the Arrow decode there, so this reads the
    // same rows the port's `execute` path reads rather than a second shape.
    //
    // **The delivered-versus-reported comparison that used to be here is gone, not relaxed.** It
    // compared a page's row count against the endpoint's own `totalRows`, which only the deleted
    // HTTP wire transport ever reported; an ADBC read streams the whole result and `run` consumes
    // the reader to exhaustion, so a truncated stream is an `Err` rather than a short answer. What
    // remains is the shape check, which is the one that matters here: a partial answer to *who am
    // I* would be a wrong identity, and any shape but 1x1 is refused.
    let rows = answered.to_rows().map_err(|cause| BigQueryError::Unreadable { cause })?;
    let shape = || BigQueryError::NoIdentityInTheAnswer {
        rows: rows.rows().len(),
        columns: rows.columns().len(),
    };
    let Some(Value::Text(who)) = rows.scalar() else {
        return Err(shape());
    };
    Ok(SessionUser::new(who.clone()))
}

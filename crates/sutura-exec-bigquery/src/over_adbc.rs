//! The surface only the ADBC transport has: opening a dataset over it, and a split leg's two
//! halves - the job run once as the asking subject at the coordinating node, and one stream of its
//! result read with the forwarded token at another.
//!
//! **Nothing in production calls either half yet** (`github.com/telekom/sutura#828`): no
//! composition root offers a `BigQuery` leg for splitting, and no reading worker reads a `BigQuery`
//! partition, so every `BigQuery` leg is still read whole.

use core::num::NonZeroUsize;
use std::time::SystemTime;

use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{Executable, LegPlan};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::labels::JobLabels;

use crate::transport::JobDeadline;
use crate::{BigQueryError, BigQueryWarehouse, JobBilling, Mapped, adbc, billing};
#[cfg(doc)]
use sutura_domain::warehouse::Warehouse;

/// A warehouse whose transport is the ADBC driver (telekom/sutura#913).
///
/// Reaching this constructor is what makes the `adbc` module first-party:
/// `BigQueryWarehouse<adbc::AdbcBigQuery>` is a concrete, constructible transport
/// the composition root can pick. Default-off (`adbc` feature), for the reason
/// any out-of-process transport is: the native driver is a per-triple addition.
impl BigQueryWarehouse<adbc::AdbcBigQuery> {
    /// Opens a dataset over the ADBC transport.
    ///
    /// `driver` is where this process reaches the self-built driver: linked into a release
    /// artefact's own binary, or a `.so` a deployment mounted. [`adbc::DriverLocation`] carries why
    /// that is a parsed value and not a path, and `nix/bigquery-adbc.nix` builds both shapes from
    /// one pinned source.
    ///
    /// `impersonation` is whether this source impersonates and how - through the pool of the
    /// source's declared `workload_identity.audience`, with the subject's own Google access token
    /// for `google_login`, or [`adbc::Impersonation::Disabled`] for a shared one, as a property of
    /// the source. A pool source also needs [`adbc::AdbcBigQuery::exchanging_through`], which this
    /// constructor does not take: without it every subject there is refused as uncovered.
    ///
    /// `max_bytes_billed` is the source's own `sources.<alias>.max_bytes_billed`, already parsed:
    /// every job this transport submits carries it as `BigQuery`'s `maximumBytesBilled`, so the bound
    /// on bytes scanned is enforced by the service. [`adbc::BytesBilledCeiling`] states what it does
    /// NOT bound - it is per JOB, and `governance.per_replica_spend_ceiling` is a different key that
    /// this adapter still does not reach.
    #[must_use]
    pub const fn over_adbc(
        source: SourceName,
        posture: SourcePosture,
        billing: JobBilling,
        driver: adbc::DriverLocation,
        impersonation: adbc::Impersonation,
        max_bytes_billed: adbc::BytesBilledCeiling,
    ) -> Self {
        Self::new(
            source,
            posture,
            billing,
            adbc::AdbcBigQuery::new(driver, impersonation, max_bytes_billed),
        )
    }

    /// `leg` run once at this source as the asking subject, its result split into at most `readers`
    /// streams for other nodes to read with the forwarded token. It reports when what it forwards
    /// ends ([`adbc::SplitLeg::until`]): the earlier of `ends` and the forwarded token's expiry.
    /// **Computed, not applied** - no grant reads it yet.
    ///
    /// Checked as [`Warehouse::execute`] checks a leg before anything is sent, billed to the
    /// project it bills, and labelled with `labels` as a billed leg is - the port's `split_lookup`
    /// carries no labels yet, so its caller has to bring them. Then both exchanges, before the
    /// driver loads: the job runs on a token
    /// asked for the `BigQuery` client's own scope, so it can still be cancelled, and only a second
    /// token, asked for `bigquery.readonly`, is forwarded ([`adbc::SplitLeg::into_forwarded`]).
    ///
    /// # Errors
    ///
    /// What [`Warehouse::execute`] refuses before it sends a statement; then, as
    /// [`BigQueryError::Endpoint`], [`adbc::AdbcError::Uncovered`] for anything but a subject at a
    /// source that federates at a pool through a composed exchange, decided before either exchange,
    /// either exchange's failure, and the driver's.
    pub fn split_leg(
        &self,
        leg: &LegPlan,
        presented: &Presented,
        deadline: Deadline,
        readers: NonZeroUsize,
        ends: SystemTime,
        labels: &JobLabels,
    ) -> Mapped<adbc::SplitLeg, adbc::AdbcError> {
        let executable = Executable::Leg(leg);
        let payer = self.unstated_payer(&billing::executable_projects(executable)?)?;
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        let identity = self.job_identity(presented)?;
        self.transport
            .split(
                &Self::request(&query, &payer, identity, JobDeadline::Port(deadline)).labelled(Some(labels)),
                readers,
                ends,
            )
            .map_err(|cause| BigQueryError::Endpoint { cause })
    }

    /// The one stream of `leg`'s split result that `descriptor` names, read with `token` as the
    /// only credential the transport sets, under the project this node's own billing resolves for
    /// `leg` - nothing compares it with the project the split job billed.
    ///
    /// **The limits**: it takes no presented identity and cannot tell whose token it holds - the
    /// token is opaque, so the caller decides that it is the forwarded one, and a `deadline` that
    /// ends no later than the token does. `clippy.toml` bans the call, so each caller is an
    /// `#[expect]` a reviewer sees. Whether `BigQuery` lets a principal other than the one that ran
    /// the job read a stream of its result is not measured here.
    ///
    /// # Errors
    ///
    /// A leg that names no project, or no payer; then, as [`BigQueryError::Endpoint`],
    /// [`adbc::AdbcError::Uncovered`] at a source that federates at no pool, decided before the
    /// driver loads, and the driver's failure - among them a descriptor that is not its shape, and
    /// a token `BigQuery` refuses or that has expired.
    pub fn read_partition(
        &self,
        leg: &LegPlan,
        descriptor: &[u8],
        token: &Secret,
        deadline: Deadline,
    ) -> Mapped<ResultBatches, adbc::AdbcError> {
        let payer = self.unstated_payer(&billing::executable_projects(Executable::Leg(leg))?)?;
        self.transport
            .read_partition(&payer, token, descriptor, JobDeadline::Port(deadline))
            .map_err(|cause| BigQueryError::Endpoint { cause })
    }
}

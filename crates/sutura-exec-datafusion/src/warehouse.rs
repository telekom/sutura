use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, UnannouncedBatch, Warehouse};

use crate::{DataFusionError, DataFusionWarehouse, pool};

impl Warehouse for DataFusionWarehouse {
    type Error = DataFusionError;

    /// **This engine cannot impersonate anybody, and saying so is the point of the declaration.** It
    /// is one process reading local files under one operating-system identity, and there is no place
    /// in that path for a subject to arrive - not a connection to authenticate, not a session to
    /// switch, not a token to present. A file engine is the easiest source in the world to assume
    /// nothing about, and "nobody declared anything for the engine" is how a deployment ends up
    /// believing its whole surface impersonates because its *network* source does.
    ///
    /// The boot check reads this against the configured posture, so a source declared
    /// `impersonation-at-source` on this adapter does not start.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    /// **This engine runs one source's share of a two-source answer, and it is the one adapter every
    /// release links that does** - `sutura-exec-postgres` does too, behind a feature. Declared rather
    /// than defaulted, and the default it overrides is
    /// documented on the port as *a missed-optimisation default rather than a missed-security one:
    /// the cost of being wrong is a refused question, never a wrong number* - which is precisely
    /// what makes opting ONE adapter in an ordinary capability statement and opting every adapter in
    /// a change of that argument.
    ///
    /// **What the port's default asks for, and why this adapter can answer it:** the default's own
    /// condition is *a combiner above it to hand a leg's rows to*, and there is one - the combine is
    /// `sutura_domain::plan::FederatedPlan::combine`, a pure domain function that no adapter is on
    /// the path of. So the engine's role here is the data source's, not the combiner's, and
    /// `docs/adr/0007`'s *the engine is also a data source* is the sentence that permits it.
    ///
    /// **The limit, next to the claim.** This is single-player federation. Two sources are not two
    /// identities: [`Warehouse::IMPERSONATION`] above is
    /// [`ImpersonationCapability::NoPlaceForASubject`], so every leg this adapter runs runs under
    /// one operating-system identity and none of them runs as the asker. A two-source answer
    /// records both legs' postures; both are the shared one.
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    // No `dry_run`, and the omission is the point. This adapter is the engine, in this process:
    // checking a plan means building the logical plan and running the analyzer and the optimizer,
    // and executing it means doing all of that and then reading. So a pre-flight here is not a
    // cheaper question than the answer - it is most of the answer, run twice, once per question.
    // `Warehouse::dry_run` is defaulted for exactly this case, and taking the default is how an
    // adapter says so. Nothing is given up: every name and every argument type is still resolved
    // during analysis, so a plan naming a table that was never attached is an error out of
    // `execute` before a single row comes back - which is what the pre-flight was for.

    /// Deadline-aware at cooperative yield points: `tokio::time::timeout` on a runtime built with
    /// `enable_time()` drops the rows future when the budget is spent. [`DataFusionError::DeadlineExceeded`]'s
    /// doc has the mechanism, `docs/adr/0029` the limits.
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        // What this leg runs as, matched exhaustively before anything is executed. There is exactly
        // one shape this adapter can honour, and the other two are a wiring defect rather than a
        // question anybody may retry - see `DataFusionError::NoPlaceForASubject`.
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } | Presented::SubjectSignIn { .. } => {
                return Err(DataFusionError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        // And the second check, which is a different one: does the leg AGREE with the posture this
        // adapter was handed? The match above compares what arrived against what this code can carry
        // and reads `self.posture` not at all - so a shared leg carrying another source's
        // acknowledgement got past it. The capability check runs first deliberately: it is the one
        // whose message names what a broker did wrong, and after it the only shape left is the shared
        // one, so this call reduces to comparing the two witnesses.
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| DataFusionError::PresentedDisagreesWithPosture { cause })?;
        let budget = deadline.budget().duration();
        let Some(remaining) = deadline.remaining_at(std::time::Instant::now()) else {
            return Err(DataFusionError::DeadlineExceeded { budget });
        };
        // **Both shapes, one path, and the match stays exhaustive.** It would read more simply as a
        // single call now that `rows` takes the `Executable` - and that is exactly what it must not
        // be: `Executable` is the port's whole vocabulary, and the arm is what makes a third plan
        // shape a compile error in this adapter rather than something it silently ran as one of
        // these two.
        let rows = match executable {
            Executable::Query(_) | Executable::Leg(_) => self.rows(executable),
        };
        // Built INSIDE this `async move`, not handed to `block_on` directly: a `Sleep` registers on
        // CONSTRUCTION, before `block_on`'s own argument is even evaluated - measured as a panic.
        self.runtime()?
            .block_on(async move { tokio::time::timeout(remaining, rows).await })
            .map_err(|_elapsed| DataFusionError::DeadlineExceeded { budget })?
    }

    /// Re-runs an anchor's plan, under this process's own identity.
    ///
    /// The same execution path `execute` takes for a whole plan, and it takes no credential because
    /// there is none at boot - which for this adapter is not a limitation but the only truth
    /// available: one process, one operating-system identity, nowhere for a subject to arrive.
    /// [`AnchorRows`] is what keeps the result from being handed back to a caller as an answer.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        // The boot path still reads rows: an anchor is a certified scalar compared against a
        // definition, and `AnchorRows` is the type that keeps it off the answer path. So the decode
        // happens here rather than at a presentation edge this call has none of.
        self.runtime()?
            .block_on(self.rows(Executable::Query(plan.plan())))?
            .to_rows()
            .map(AnchorRows::of)
            .map_err(|cause| DataFusionError::Unreadable { cause })
    }

    /// Counts a declared join key's values and its distinct values, in this process.
    ///
    /// **Overridden rather than defaulted, and this is the adapter where it matters most:** it is
    /// what a released binary links, so without it the check would exist on nothing a deployment
    /// runs. The cost argument the missing `dry_run` makes does not apply - a probe is one aggregate
    /// over one column, not most of an answer computed twice.
    ///
    /// No credential, for [`Warehouse::verify_anchor`]'s reason: there is no caller at boot, and for
    /// this adapter that is not a limitation but the only truth available.
    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        self.runtime()?.block_on(self.key_uniqueness(&key))
    }

    /// The one question the domain asks about this adapter's error, answered from the one variant
    /// that means it. [`pool::refused_a_reservation`] is the exhaustive match; this is the ceiling.
    ///
    /// `u64` because the refusal is a domain value and the domain does not know how wide this
    /// target's pointers are. `try_from` cannot fail on any target this ships to; the fallback is
    /// `u64::MAX` rather than `None`, because losing the refusal would put the caller back on the
    /// `503` this whole variant exists to get them off.
    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64> {
        pool::refused_a_reservation(error).then(|| u64::try_from(self.working_set.bytes()).unwrap_or(u64::MAX))
    }

    /// The fourth predicate: `execute`'s own [`DataFusionError::DeadlineExceeded`] and nothing else.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, DataFusionError::DeadlineExceeded { .. })
    }

    /// `true` for the MATERIALISATION BUDGET alone, which is the one failure here that is a result
    /// not fitting.
    ///
    /// **This used to be the default, and the comment that took it said there was no size a reply
    /// had to fit.** That was true of a reply and false of this process: `collected` holds the
    /// engine's own batches and `ResultBatches::to_rows` copies them again, and round 7 of
    /// `telekom/sutura#929`'s review measured that neither was bounded. Now
    /// `WorkingSet::result_budget` bounds both, and a result refused for crossing it is exactly
    /// *the result did not fit* - a governance outcome the caller cannot retry past, rather than an
    /// outage. Leaving the default would reach that caller as a `503` inviting a retry that spends
    /// the same budget in the same place.
    ///
    /// Every other [`UnannouncedBatch`](sutura_domain::warehouse::UnannouncedBatch) is `false`,
    /// exhaustively and by NAME, for the reason the `BigQuery` transport's twin gives: a mislabelled
    /// or mis-width batch is the engine disagreeing with its own announced schema, which no narrower
    /// question fixes. `OverBound` is unreachable here - `collected` passes `usize::MAX` - and is
    /// still named rather than wildcarded, so a row ceiling arriving later has to choose.
    ///
    /// **The limit, next to the claim:** this answers for the materialisation budget and not for
    /// the working-set ceiling, which keeps its own predicate and its own refusal. A caller whose
    /// operators were refused is told resources were exhausted; one whose result was refused is
    /// told the result was too large. The two are different numbers' worth of the same bytes.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        let DataFusionError::Unannounced { ref cause } = *error else {
            return false;
        };
        match *cause {
            UnannouncedBatch::OverBudget { .. } => true,
            UnannouncedBatch::OverBound { .. } | UnannouncedBatch::Width { .. } | UnannouncedBatch::Mislabelled { .. } => false,
        }
    }
}

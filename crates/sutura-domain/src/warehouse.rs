//! The execution port: the plan that goes out, and the rows that come back.
//!
//! The trait is named `Warehouse`, which is the port's name and says nothing about what sits behind
//! it. A file read by an in-process engine and a cluster with a login are both implementations.
//!
//! **No statement appears in this module, and its absence is the decision rather than an omission.**
//! A rendered statement used to live here, on the argument that the port had to hand one to
//! something. The port takes a [`crate::plan::QueryPlan`] now - [`Warehouse`] below says why that is
//! what makes a second kind of adapter possible - so nothing in the domain constructs or reads a
//! statement, and the type that carries one moved out to `sutura-sql`, beside the code that renders
//! it. A domain holding a rendered statement has acquired a concept no domain operation uses.
//!
//! What stays is [`ParamValue`], and it stays because the type the port *does* take is built out of
//! it: a [`crate::plan::QueryPlan`] carries a vector of them. It is also where the rule lives - a
//! value is a closed set of typed variants an adapter binds, never text somebody concatenated.
//! [`crate::query`] is the *tool* surface, where SQL must be unrepresentable because the text would
//! come from a caller; here there is no text for a value to reach at all.

use crate::calendar::Date;
use crate::identity::Presented;
use crate::model::SourceName;
use crate::plan::{AnchorPlan, Executable};
use crate::source::{ImpersonationCapability, SourcePosture};

/// A value bound to a placeholder.
///
/// A closed set rather than a string, because the whole point is that these never become text on
/// our side. An adapter binds them with whatever its driver offers, and the driver is what decides
/// how a date is written on the wire.
///
/// **There is no `Integer`, and its absence is the decision rather than an omission.** The variant
/// was here and nothing in the workspace constructed one: every caller value and every required
/// filter binds as [`Text`](ParamValue::Text), because that is the type both of them are. Both
/// adapters carried an arm for it and the goldens carried a rendering, so it read as covered while
/// no question could reach it - and the dead arm was the lesser half of the cost. The real half is
/// that a *numeric* definitional filter cannot be expressed safely here: `equals: { column:
/// amount_cents, value: "500" }` compares an integer column against a text parameter, `DuckDB`
/// casts it and answers, a driver that sends an explicitly-typed text parameter does not, and
/// nothing refuses the definition because a [`crate::catalog::Model`] declares only column NAMES -
/// there is no column type to check the value against. Adding the variant back without one would
/// mean guessing the type from the value's own text, which makes a text column whose allowed value
/// is `"500"` compare as a number: the same wrong comparison, arrived at from the other side.
///
/// So it goes when a typed column model does, and not before. The reasoning is the one
/// `sutura_exec_datafusion`'s `cell` gives for leaving `Date64` unmapped: an unreachable arm holding
/// a semantic choice nobody reviewed is worse than not having the arm.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ParamValue {
    Text(String),
    Date(Date),
}

impl ParamValue {
    /// A human-readable form, for showing a plan to a person.
    ///
    /// **Display only.** It is deliberately not the SQL literal for the value: a function that
    /// produced one would be the thing somebody reaches for the day they want to inline a parameter,
    /// and inlining a parameter is the one move this type exists to prevent. Text is quoted the way
    /// `Debug` quotes it, which makes an empty or space-padded value visible rather than SQL-shaped.
    pub fn render(&self) -> String {
        match *self {
            Self::Text(ref v) => format!("{v:?}"),
            Self::Date(d) => d.to_iso(),
        }
    }
}

/// Why a floating-point cell was refused.
///
/// Two variants rather than one, because the two faults have different causes and a reader chasing
/// one is not chasing the other: an infinity is a non-zero quantity divided by zero, and a `NaN` is
/// zero divided by zero. The variant carries the value rather than a formatted sentence, for the
/// reason every error in this crate does.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NotFinite {
    /// Infinite, in either direction.
    #[error("{value} is not a finite number")]
    Infinite { value: f64 },
    /// Not a number at all. Its own variant rather than a value on the one above, because `NaN`
    /// compares unequal to itself: an [`Infinite`] carrying one would make two of these errors
    /// unequal for a reason that has nothing to do with what happened.
    ///
    /// [`Infinite`]: NotFinite::Infinite
    #[error("NaN is not a number")]
    NotANumber,
}

/// A real number a result may carry: finite, and nothing else.
///
/// **Parsed rather than validated, and the class it closes is larger than the bug that found it.**
/// A cell used to be a raw `f64`, so `inf`, `-inf` and `NaN` were all representable, and
/// [`Value::render`] turned the first of them into the string `"inf"` - an answer under a metric's
/// own certified name that reads as data and is not a number. The route in was a ratio measure
/// declaring `zero_denominator: fails`: both adapters cast the numerator to a floating type before
/// dividing, so the division is IEEE float division, and IEEE float division by zero does not fail.
/// It answers `inf`, or `NaN` when both halves are zero.
///
/// Making the domain type refuse a non-finite value closes all three at once, at the one boundary
/// every adapter has to cross, rather than guarding the one variant that exposed it. An adapter that
/// gets one back has an error naming the column, which is what `fails` was always claiming to mean.
///
/// Construct it with [`parse`]. The field is private, so a non-finite value is unrepresentable
/// rather than merely rejected. There is deliberately no `Deref` and no arithmetic: two finite
/// numbers divide to a non-finite one, so a type that let the result back in without passing
/// [`parse`] again would be the hole this closes. [`Value`] is `Serialize` only today - if it ever
/// gains `Deserialize`, this needs `#[serde(try_from = ..)]` routing through [`parse`], because a
/// derived one writes straight into the private field.
///
/// [`parse`]: Real::parse
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Real(f64);

impl Real {
    /// Parses a real number, rejecting a non-finite one.
    pub const fn parse(value: f64) -> Result<Self, NotFinite> {
        if value.is_nan() {
            return Err(NotFinite::NotANumber);
        }
        if value.is_infinite() {
            return Err(NotFinite::Infinite { value });
        }
        Ok(Self(value))
    }

    /// The number, for a caller that has to do arithmetic on it.
    ///
    /// Named rather than reached through `Deref`, so the point at which the invariant stops applying
    /// is a call somebody wrote.
    #[inline]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Shortest round-trip formatting, so a value that came back as an exact decimal is rendered as one
/// rather than as its binary expansion. Delegated rather than reimplemented, and this is the one
/// definition [`Value::render`] uses.
impl core::fmt::Display for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Exponent form, forwarding the formatter's precision.
///
/// It exists because comparing two engines' floats is done at a fixed number of significant digits -
/// summing the same rows in a different order changes the last place of an `f64` - and `{:.12e}` is
/// how that comparison is written. A formatting trait rather than `get`, so the comparison does not
/// have to leave the type to be expressed.
impl core::fmt::LowerExp for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerExp::fmt(&self.0, f)
    }
}

/// One cell of a result.
///
/// [`Real`] is deliberately last on the list of things to reach for. A measure over integer minor
/// units stays exact, and an anchor comparison over a float would depend on how two languages print
/// the same bits. It exists because `avg` has to land somewhere - and it is a checked type rather
/// than an `f64`, so the one thing a float can be that a number cannot does not fit in a cell.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Value {
    Null,
    Integer(i64),
    Real(Real),
    Text(String),
}

impl Value {
    /// The canonical text form, which is what an anchor is compared against.
    ///
    /// One function so there is one answer. An anchor comparison that formatted the value at the
    /// call site would compare differently in two places, and the failure would look like a data
    /// problem rather than a formatting one.
    pub fn render(&self) -> String {
        match *self {
            Self::Null => String::from("null"),
            Self::Integer(v) => v.to_string(),
            // One definition of what a real number looks like, on the type that carries one.
            Self::Real(v) => v.to_string(),
            Self::Text(ref v) => v.clone(),
        }
    }
}

/// A result set: the column labels, and the rows.
///
/// Labels are `String` rather than [`crate::model::ColumnName`] because a generated projection names
/// things a model did not: the truncated time bucket, and the measure under the metric's own name.
/// Constraining them to model column names would mean either lying about what they are or refusing
/// to name them.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RowSet {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

/// Why a result set could not be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MalformedRowSet {
    /// A row has a different number of cells than there are columns.
    ///
    /// Checked once here rather than trusted, because every consumer downstream indexes by column
    /// position, and the `indexing_slicing` ban means each of them would otherwise need its own
    /// fallback for a case that must not exist.
    #[error("row {row} has {cells} cells, and there are {columns} columns")]
    RowWidth { row: usize, cells: usize, columns: usize },
}

impl RowSet {
    /// Builds a result set, rejecting a ragged one.
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Result<Self, MalformedRowSet> {
        for (index, row) in rows.iter().enumerate() {
            if row.len() != columns.len() {
                return Err(MalformedRowSet::RowWidth {
                    row: index,
                    cells: row.len(),
                    columns: columns.len(),
                });
            }
        }
        Ok(Self { columns, rows })
    }

    #[inline]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[inline]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }

    /// Where a column with this label sits, if there is exactly one.
    ///
    /// `None` for a label that appears twice, not the first match. Two columns with one label means
    /// the projection is not what we think it is, and returning either of them would answer with a
    /// number from a column nobody chose. `Definitions::assemble` refuses the catalog shapes that
    /// could cause it, so this is the second line rather than the first.
    pub fn column_index(&self, label: &str) -> Option<usize> {
        let mut found = None;
        for (index, name) in self.columns.iter().enumerate() {
            if name == label {
                if found.is_some() {
                    return None;
                }
                found = Some(index);
            }
        }
        found
    }

    /// One cell, by row and column position.
    ///
    /// `Option` rather than indexing, because `indexing_slicing` is denied for library crates here
    /// and because a caller that has a position from `column_index` still should not be able to
    /// panic on a result set that came back a different shape than expected.
    pub fn cell(&self, row: usize, column: usize) -> Option<&Value> {
        let cells = self.rows.get(row)?;
        cells.get(column)
    }

    /// The single cell of a single-row, single-column result, which is what an anchor check reads.
    ///
    /// `None` for any other shape rather than a panic or a silent first-cell: an anchor query that
    /// came back with three rows means the statement is not the one we thought, and reading its
    /// first cell would turn that into a wrong number.
    pub const fn scalar(&self) -> Option<&Value> {
        match (self.columns.as_slice(), self.rows.as_slice()) {
            ([_], [row]) => match row.as_slice() {
                [cell] => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }
}

/// What a pre-flight established.
///
/// **[`Self::NotAsked`] is not [`Self::Accepted`], and no caller can read it as one.** Before
/// `dry_run` took a credential, a default of `Ok(())` was defensible: with nothing to be wrong
/// about, "nothing went wrong" is honest. With a subject in the signature it stops being honest,
/// because `Ok(())` from an adapter that did not look is indistinguishable from `Ok(())` from an
/// adapter that asked the data system as that subject and was told yes - so a defaulted pre-flight
/// would read as "this subject may run this plan" for every adapter that declined to implement one.
///
/// The shape is the one the row cap already uses, where `row_limit()` is `max_rows + 1` so a result
/// *at* the cap is distinguishable from one cut off *by* it. `docs/adr/0008` part 1 is the decision.
///
/// **The limit, stated with the claim:** [`Self::Accepted`] is the data system's opinion at
/// pre-flight time and not a guarantee about `execute`, so it is worth a round trip and is not an
/// authorization decision. Nothing in the plan path may treat it as one, and there is no mechanism
/// that would stop it - skipping a check on the strength of `Accepted` is a review question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreFlight {
    /// The adapter did not ask. The default, and the honest answer for an adapter where checking
    /// costs what running costs.
    NotAsked,
    /// The data system was asked, as this subject, and accepted the plan.
    Accepted,
}

/// The rows one anchor's plan produced at boot.
///
/// **A wrapper with a private field, so a boot result cannot be handed back to a caller as an
/// answer without a named conversion somebody wrote.** The anchor path and the request path are two
/// ways into a data system and they run as different identities: `execute` takes the asking
/// subject's credential and cannot be called without one, and [`Warehouse::verify_anchor`] takes no
/// credential at all - it runs as whatever identity the deployment configured that adapter with,
/// which is what `docs/adr/0008` part 1 decides for a path that has no caller.
///
/// Two types rather than one so the separation is visible at a call site rather than in a comment.
/// [`Self::verified_at_boot`] is named to be conspicuous in review and in a grep, the way
/// `crate::identity::Secret::expose` is.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorRows(RowSet);

impl AnchorRows {
    /// What an adapter returns from a verification run.
    #[inline]
    #[must_use]
    pub const fn of(rows: RowSet) -> Self {
        Self(rows)
    }

    /// The rows, for the boot path that compares them against what an author certified.
    #[inline]
    #[must_use]
    pub const fn verified_at_boot(&self) -> &RowSet {
        &self.0
    }
}

/// Where a plan runs.
///
/// **The port takes a [`crate::plan::QueryPlan`], not a statement, and that is what makes a second
/// kind of adapter possible.** Taking a rendered statement said that every data system speaks SQL.
/// An in-process engine does not: it executes a logical plan over Arrow and generates no SQL at all.
/// So the plan is the contract and rendering is one adapter's private business.
///
/// `dry_run` exists separately from `execute` because "would this be accepted" is worth being able
/// to ask before committing to the cost of an answer - **where asking is cheaper than answering.**
/// It is defaulted rather than required for exactly that reason: an adapter for which it is not
/// cheaper has no way to say so if the port demands an implementation, and the honest thing for it to
/// do is nothing.
///
/// # Nothing here executes without saying whose credential it holds
///
/// [`Self::execute`] takes a [`Presented`] and has no default, so there is no code path into a data
/// system that runs as whatever the process happens to be. **Today's signature IS the fallback:** an
/// adapter with no credential parameter runs as the process, and nothing anywhere had to decide
/// that. `docs/adr/0008` part 1 is the decision, and the mechanism is the absence of a signature
/// rather than a rule somebody follows.
///
/// The boot path is the other caller of this port and it has no subject, so it gets its own method:
/// [`Self::verify_anchor`] takes no credential and returns [`AnchorRows`] rather than a [`RowSet`].
/// **Which is narrower than the record asked for, deliberately.** `docs/adr/0008` gave that method a
/// `VerificationIdentity` parameter so the two credentials could not be confused at a call site, and
/// then named a `compile_fail` test asserting that answering a question cannot pass one. That test
/// could not have held: `crate::source::VerificationIdentity::parse` is `pub`, so any crate can
/// construct one. A method that takes NO credential has no parameter to pass one to, which is the
/// property the record wanted, reached by removing the argument instead of by typing it.
///
/// **What bounds a method with no credential is WHERE it is called from, and that is a lint here
/// rather than a type - which is a second review's correction to a claim this comment used to make.**
/// The first correction gave [`Self::verify_anchor`] an [`AnchorPlan`] instead of a bare
/// [`QueryPlan`](crate::plan::QueryPlan) and said the method could no longer be handed a question. A
/// reviewer disproved that in one function: the constructor is `pub`, every value it read was
/// publicly constructible, and a fabricated tuple passed all four guards. That is not a hole a fifth
/// guard closes - a shape check over caller-constructible values can only ever be a shape check, and
/// Rust has no cross-crate friend visibility to hide the constructor behind.
///
/// So the two mechanisms are named separately, because they do different jobs:
///
/// - `clippy.toml` bans `sutura_domain::warehouse::Warehouse::verify_anchor`, verified to resolve by
///   writing the call and watching clippy reject it. `sutura_app::verify_anchors` holds the single
///   `#[expect]`, so a second call site is an error under `-D warnings` until somebody writes a
///   second expectation a reviewer sees in the diff. **That is what makes the path boot-only**, and
///   its limit is that a lint reaches this workspace and an `#[allow]` walks past it.
/// - [`AnchorPlan`] checks that the boot path compiled the question it meant to, reading the metric's
///   definition, its anchor's range and its coarsest grain off the pinned bundle rather than taking
///   them as arguments. It is a **self-check on that one caller and not an authority**, and the type
///   says so at length.
///
/// # The two identity declarations, and why they are two
///
/// [`Self::IMPERSONATION`] is a property of the **code**: whether this adapter has anywhere for a
/// subject's own credential to arrive. It is an associated constant with no default, so an adapter
/// cannot omit it, and it cannot vary per instance - which is what lets a boot check mean anything.
///
/// [`Self::posture`] is a property of the **deployment**: which identity a query is to reach this
/// source as. It is a method, because the composition root hands it to the adapter at construction,
/// and it is required rather than defaulted because a default posture is a posture nobody chose.
///
/// The two are compared at boot by `SourcePosture::deliverable_by`. Conflating them was the tempting
/// mistake and it gives the mode two owners: an adapter cannot declare a mode it does not own,
/// because the same adapter is correct in either posture and only the deployment knows which one it
/// is being asked for.
///
/// **An adapter that declares no impersonation capability does not compile:**
///
/// ```compile_fail
/// use sutura_domain::identity::Presented;
/// use sutura_domain::model::SourceName;
/// use sutura_domain::plan::{AnchorPlan, Executable};
/// use sutura_domain::source::SourcePosture;
/// use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};
///
/// struct Undeclared {
///     source: SourceName,
///     posture: SourcePosture,
/// }
///
/// // No `const IMPERSONATION`, so this impl is incomplete: the trait declares it with no default.
/// impl Warehouse for Undeclared {
///     type Error = core::fmt::Error;
///
///     fn source(&self) -> &SourceName {
///         &self.source
///     }
///
///     fn posture(&self) -> &SourcePosture {
///         &self.posture
///     }
///
///     fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
///         Err(core::fmt::Error)
///     }
///
///     fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
///         Err(core::fmt::Error)
///     }
/// }
/// ```
///
/// The compiling twin, so the block above cannot be passing on a typo - the only difference between
/// the two is the one line that declares the capability:
///
/// ```
/// use sutura_domain::identity::Presented;
/// use sutura_domain::model::SourceName;
/// use sutura_domain::plan::{AnchorPlan, Executable};
/// use sutura_domain::source::{ImpersonationCapability, SourcePosture};
/// use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};
///
/// struct Declared {
///     source: SourceName,
///     posture: SourcePosture,
/// }
///
/// impl Warehouse for Declared {
///     type Error = core::fmt::Error;
///
///     const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
///
///     fn source(&self) -> &SourceName {
///         &self.source
///     }
///
///     fn posture(&self) -> &SourcePosture {
///         &self.posture
///     }
///
///     fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
///         Err(core::fmt::Error)
///     }
///
///     fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
///         Err(core::fmt::Error)
///     }
/// }
///
/// assert_eq!(
///     <Declared as Warehouse>::IMPERSONATION,
///     ImpersonationCapability::NoPlaceForASubject
/// );
/// ```
pub trait Warehouse {
    /// Why this data system could not answer. Typed per adapter: a connection failure, a rejected
    /// statement and a permission denial are not the same thing to whoever responds to them.
    type Error: core::error::Error + 'static;

    /// Whether this adapter can carry a per-subject credential **at all**.
    ///
    /// **Required, with no default, and that is the whole mechanism.** A defaulted capability would
    /// mean an adapter that said nothing got the benefit of the doubt in whichever direction the
    /// default pointed - and both directions are wrong. Defaulted to *can*, a file engine would
    /// silently satisfy an impersonation cross-check it cannot honour. Defaulted to *cannot*, a real
    /// network adapter that forgot the line would be refused for a capability it has, and somebody
    /// would fix that by deleting the check.
    ///
    /// A constant rather than a method because it is fixed for the life of the process: it is a fact
    /// about what was linked, nothing at run time can widen it, and a source that gains a capability
    /// is a deployment change.
    ///
    /// **The cost, stated where the decision is:** an associated constant makes this trait not
    /// object-safe. Nothing in the workspace holds a `dyn Warehouse` today, and a heterogeneous set
    /// of adapters behind one port wants a closed enum over the registered adapters rather than
    /// dynamic dispatch - which is the same *pluggable by declaration* argument this constant comes
    /// from. If that ever changes, it is an architecture decision and not a signature tweak.
    const IMPERSONATION: ImpersonationCapability;

    /// The name a plan uses to select this adapter.
    fn source(&self) -> &SourceName;

    /// Which identity a query is to reach this source as, as the deployment declared it.
    ///
    /// **Required, and what an answer's provenance is read off.** The composition root hands the
    /// posture to the adapter at construction and provenance asks the adapter, so the record says
    /// what the thing that executed was actually holding. A provenance field read from the settings
    /// tree instead would report a leg as impersonated on the strength of a file, which is the one
    /// thing that field exists to stop.
    ///
    /// The limit is worth naming with the claim: today this is still the value the root handed over,
    /// so what it proves is that configuration reached the adapter - not that the data system
    /// evaluated anybody's authorization. That becomes a stronger claim when the execution port takes
    /// a credential per leg and the adapter matches exhaustively on what it received.
    fn posture(&self) -> &SourcePosture;

    /// Checks the plan is executable here, without producing rows.
    ///
    /// Takes an [`Executable`] for [`execute`](Warehouse::execute)'s reason, so the two cannot
    /// disagree about what this adapter accepts.
    ///
    /// **Defaulted to doing nothing, and the default is a statement rather than a stub.** A data
    /// system across a network can prepare a statement for a fraction of what running it costs, so
    /// there the pre-flight is worth a round trip: a plan naming a column that is not there is
    /// rejected before any data is read. An in-process engine cannot make that trade - checking means
    /// building the logical plan and running the analyzer and the optimizer, which is most of
    /// executing it - so a required `dry_run` bought that guarantee at the price of planning every
    /// question twice. Not implementing this is how such an adapter says "checking is not cheaper
    /// than running here"; `execute` is then the only pass, and it still fails before returning rows
    /// for the same reasons the check would have.
    ///
    /// An adapter that overrides it must not read data: the contract is a plan that resolves, not a
    /// result.
    ///
    /// **It takes the credential too, and not for symmetry.** A pre-flight asks "would this be
    /// accepted", and the answer depends on who is asking: under the process identity it would report
    /// a plan as executable that the subject may not execute, or prepare a statement against tables
    /// the subject cannot see. The check has to be asked as the same principal as the question, or it
    /// answers a different question - which is also why the return type is [`PreFlight`] rather than
    /// `()`. See that type for what its two variants keep apart.
    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    /// Runs the plan and returns its rows.
    ///
    /// **One method for both plan shapes, and the exhaustive match is why.** An
    /// [`Executable`] is a whole [`QueryPlan`](crate::plan::QueryPlan) or one
    /// [`LegPlan`](crate::plan::LegPlan) of a federated answer, so an adapter that reads what it was
    /// handed has to say what it does with each. A second port method for legs was considered and
    /// rejected: a second method invites a default body, a default that errors lets an adapter be
    /// silently non-federating, and *adding a data system is a registration* would then stop being
    /// true in the one direction nobody would notice.
    /// `docs/adr/0007-federating-across-different-data-systems.md` is the decision.
    ///
    /// **Nothing hands any adapter a leg today**, because there is no splitter and no combiner. An
    /// adapter that cannot execute one says so with a typed error of its own rather than with a
    /// default it inherited.
    ///
    /// # The credential is a parameter, and it cannot be omitted
    ///
    /// `presented` is what this leg executes as, minted for THIS source by a
    /// [`CredentialBroker`](crate::identity::CredentialBroker). An adapter matches on it
    /// exhaustively and returns its own typed error for a shape it is not configured for -
    /// `docs/adr/0008` part 4 states both directions and says which is the dangerous one: an adapter
    /// that quietly accepted subject material it cannot use would report a leg as impersonated that
    /// ran shared.
    ///
    /// Calling it without one does not compile:
    ///
    /// ```compile_fail
    /// use sutura_domain::plan::Executable;
    /// use sutura_domain::warehouse::{RowSet, Warehouse};
    ///
    /// fn _as_the_process<W: Warehouse>(warehouse: &W, executable: Executable<'_>) -> Result<RowSet, W::Error> {
    ///     warehouse.execute(executable)
    /// }
    /// ```
    ///
    /// The compiling twin, so the block above cannot be passing for a typo - the only difference is
    /// the argument that says whose credential this runs under:
    ///
    /// ```
    /// use sutura_domain::identity::Presented;
    /// use sutura_domain::plan::Executable;
    /// use sutura_domain::warehouse::{RowSet, Warehouse};
    ///
    /// fn _as_the_asker<W: Warehouse>(
    ///     warehouse: &W,
    ///     executable: Executable<'_>,
    ///     presented: &Presented,
    /// ) -> Result<RowSet, W::Error> {
    ///     warehouse.execute(executable, presented)
    /// }
    /// ```
    fn execute(&self, executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error>;

    /// Re-runs one anchor's plan, under the identity this adapter was configured with.
    ///
    /// **Separate from [`execute`](Warehouse::execute) because the boot path has no caller.** Every
    /// anchor in a bundle is executed against the data system before a listener is bound, so the one
    /// thing this method cannot be handed is an asking subject's credential - there is none in
    /// scope, and inventing one is the service-identity fallback arriving through the back door.
    /// `docs/adr/0008` part 1 decides that it gets its own method, and the trait's own header says
    /// where this shape is narrower than that record asked for.
    ///
    /// Required, with no default. A defaulted body could not execute anything - it has no credential
    /// to pass `execute` - so the only default available is one that lies about having verified
    /// something, which is the shape [`PreFlight`] exists to avoid one level up.
    ///
    /// It takes an [`AnchorPlan`] rather than an [`Executable`]: an anchor is asked with no dimensions
    /// and resolves to one model on one source, so there is no leg for it to be - and the plan is
    /// checked against the pinned bundle as a declared anchor's own rather than taken on trust, which
    /// catches a boot path that compiled a question where an anchor was meant. **It does not make the
    /// method unreachable, and the trait header says why**: what keeps this method to the boot path is
    /// the `clippy.toml` ban on it. Returns [`AnchorRows`], which is what stops a boot result being
    /// handed back as an answer.
    ///
    /// **What an executed anchor proves, precisely:** that these statements reproduced the numbers
    /// their author certified *for the identity this adapter holds*. Under row-level security that is
    /// not necessarily any caller's - a per-subject anchor is a function rather than a number, and
    /// there is no subject at boot to evaluate it at.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error>;

    /// Was this failure the working-set ceiling refusing a reservation, and what was the ceiling?
    ///
    /// **The domain naming what it needs, because it cannot look.** `Self::Error` is the adapter's
    /// own type, so nothing above this port can tell "the pool would not grow" from "the connection
    /// dropped" - and those two are a governance refusal and a transport failure respectively. This
    /// is the one question the domain has to be able to ask about an adapter's error, and it is a
    /// predicate rather than a conversion so that an adapter cannot mint an arbitrary
    /// [`RefusalReason`](crate::query::RefusalReason) from a failure of its own.
    ///
    /// `Some(bytes)` is the ceiling in bytes that the reservation was refused against, which is a
    /// number an operator configured; `None` is every other failure, including a failure whose cause
    /// happens to mention memory. An adapter that cannot tell the difference must answer `None`,
    /// because the cost of the two mistakes is not symmetric: a transport failure reported as
    /// exhaustion tells a caller not to retry something a retry would have answered.
    ///
    /// Defaulted to `None`, which is the honest answer for an adapter with no pool to bound - and
    /// for one whose engine has an unbounded one, since a ceiling that cannot be exceeded cannot be
    /// the thing that refused.
    ///
    /// Takes `&self` because the ceiling belongs to the adapter rather than to the error, and the
    /// error's own text is not where a bound belongs.
    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{MalformedRowSet, NotFinite, ParamValue, Real, RowSet, Value};
    use crate::calendar::Date;

    fn real(value: f64) -> Real {
        Real::parse(value).expect("a test literal is finite")
    }

    #[test]
    fn a_parameter_renders_for_a_reader_and_not_as_sql() {
        // The rendering exists so `sutura compile` can show a plan. It must not look like something
        // to paste into a statement: text keeps its quotes so an empty or padded value is visible,
        // and nothing here escapes anything, because escaping is what a bind parameter replaces.
        assert_eq!(
            ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date")).render(),
            "2026-06-01"
        );
        assert_eq!(ParamValue::Text(String::from("north")).render(), "\"north\"");
        assert_eq!(ParamValue::Text(String::new()).render(), "\"\"");
        // A value that would be an injection if it were text in a statement renders visibly as a
        // value rather than as syntax.
        assert_eq!(ParamValue::Text(String::from("a' OR '1'='1")).render(), "\"a' OR '1'='1\"");
    }

    #[test]
    fn a_ragged_result_set_is_rejected_once_rather_than_handled_everywhere() {
        // Every consumer reads cells by column position. Without this check each of them needs its
        // own fallback for a shape that must not exist, and under the `indexing_slicing` ban those
        // fallbacks are where a wrong value gets substituted for a missing one.
        assert_eq!(
            RowSet::new(vec![String::from("a"), String::from("b")], vec![vec![Value::Integer(1)]],).unwrap_err(),
            MalformedRowSet::RowWidth {
                row: 0,
                cells: 1,
                columns: 2,
            }
        );
    }

    #[test]
    fn scalar_refuses_any_shape_that_is_not_one_cell() {
        // An anchor check reads this. If it returned the first cell of a three-row result, an
        // anchor would silently pass against a statement that grouped when it should not have.
        let one = RowSet::new(vec![String::from("v")], vec![vec![Value::Integer(7)]]).expect("one cell is a valid result");
        assert_eq!(one.scalar(), Some(&Value::Integer(7)));

        let two_rows = RowSet::new(
            vec![String::from("v")],
            vec![vec![Value::Integer(7)], vec![Value::Integer(8)]],
        )
        .expect("two rows is a valid result");
        assert_eq!(two_rows.scalar(), None);

        let two_columns = RowSet::new(
            vec![String::from("a"), String::from("b")],
            vec![vec![Value::Integer(7), Value::Integer(8)]],
        )
        .expect("two columns is a valid result");
        assert_eq!(two_columns.scalar(), None);

        let empty = RowSet::new(vec![String::from("v")], vec![]).expect("no rows is a result");
        assert_eq!(empty.scalar(), None);
    }

    #[test]
    fn rendering_is_one_function_so_an_anchor_compares_the_same_way_everywhere() {
        assert_eq!(Value::Integer(197_122).render(), "197122");
        assert_eq!(Value::Text(String::from("north")).render(), "north");
        assert_eq!(Value::Null.render(), "null");
        // Shortest round-trip: an exact decimal comes back as one rather than as 0.30000000000000004.
        assert_eq!(Value::Real(real(0.3_f64)).render(), "0.3");
    }

    #[test]
    fn a_cell_cannot_hold_a_number_that_is_not_one() {
        // THE BUG THIS EXISTS FOR. `Real` used to be a raw `f64`, so a ratio measure declaring
        // `zero_denominator: fails` answered the string "inf" under its own certified metric name:
        // both adapters cast the numerator to a floating type before dividing, so the division is
        // IEEE float division, and IEEE float division by zero does not fail. Nothing between the
        // data system and the caller looked at the value, because nothing had a place to.
        //
        // Asserted over all three of the class rather than over the one variant that exposed it: a
        // guard on the division would have left `-inf` and `NaN` representable.
        assert_eq!(
            Real::parse(f64::INFINITY).unwrap_err(),
            NotFinite::Infinite { value: f64::INFINITY }
        );
        assert_eq!(
            Real::parse(f64::NEG_INFINITY).unwrap_err(),
            NotFinite::Infinite {
                value: f64::NEG_INFINITY
            }
        );
        assert_eq!(Real::parse(f64::NAN).unwrap_err(), NotFinite::NotANumber);
        // The messages an adapter's error chain ends in, so the reader is told which of the three.
        assert_eq!(
            Real::parse(f64::INFINITY).unwrap_err().to_string(),
            "inf is not a finite number"
        );
        assert_eq!(
            Real::parse(f64::NEG_INFINITY).unwrap_err().to_string(),
            "-inf is not a finite number"
        );
        assert_eq!(Real::parse(f64::NAN).unwrap_err().to_string(), "NaN is not a number");

        // And what a real number still does, so this is not a test that would pass with every float
        // refused. Zero and the subnormals are finite, and a metric that legitimately answers zero
        // must not be caught by a check aimed at a division by it.
        for finite in [0.0_f64, -0.0_f64, 0.3_f64, f64::MIN, f64::MAX, f64::MIN_POSITIVE] {
            // Compared as bits rather than with `==`, which `float_cmp` bans for the reason it exists:
            // the assertion here is that the value came through UNCHANGED, and bit equality is that
            // claim exactly. It also keeps negative zero distinguishable from zero.
            assert_eq!(real(finite).get().to_bits(), finite.to_bits(), "{finite} is a finite number");
        }
    }

    #[test]
    fn a_real_number_renders_the_same_way_wherever_it_is_formatted() {
        // `Value::render` is what an anchor is compared against and `{:.12e}` is what a differential
        // comparison between two engines uses. Both go through this one type, so neither can drift
        // into its own idea of what the number looks like.
        assert_eq!(format!("{}", real(0.3_f64)), "0.3");
        assert_eq!(format!("{:.12e}", real(190_007.333_333_333_34_f64)), "1.900073333333e5");
    }
}
